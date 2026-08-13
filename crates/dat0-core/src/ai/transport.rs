//! Single network seam: build + send one request through SSRF + Wire + reqwest.

use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use futures::StreamExt as _;

use crate::ai::provider::Provider;
use crate::ai::request::AiRequest;
use crate::ai::settings::AiSettings;
use crate::ai::sse::SseDecoder;
use crate::ai::{ssrf, wire};

pub struct TestOutcome {
    pub ok: bool,
    pub message: String,
}

/// Resolve the base URL for a provider, applying SSRF checks to Custom.
async fn resolve_base_url(provider: Provider, cfg: &AiSettings) -> Result<String> {
    if let Some(fixed) = provider.fixed_base_url() {
        return Ok(fixed.to_string());
    }
    // Custom: scheme + literal-IP checks, then resolve-and-recheck.
    let validated = ssrf::validate_url(&cfg.custom_base_url, cfg.advanced_override)?;
    if !cfg.advanced_override {
        if let Some(host) = validated.0.host_str() {
            let port = validated.0.port_or_known_default().unwrap_or(443);
            // DNS-rebinding guard: every resolved IP must pass. NOTE: this is a
            // recheck, not connection-pinning — reqwest re-resolves the host
            // independently at `.send()`, leaving a residual TOCTOU window. Full
            // pinning (a custom reqwest resolver bound to the checked IP) is a
            // deferred hardening follow-up; the recheck closes the practical gap
            // for a key the user themselves supplied.
            let addrs = tokio::net::lookup_host((host, port))
                .await
                .map_err(|e| anyhow!("dns resolution failed: {e}"))?;
            for sa in addrs {
                if ssrf::is_blocked_ip(sa.ip()) {
                    bail!("resolved to a blocked address: {}", sa.ip());
                }
            }
        }
    }
    Ok(cfg.custom_base_url.clone())
}

/// Wire bytes of the provider auth headers.
///
/// Takes the built header vec rather than the key so the secret is never
/// handed to the telemetry layer at all — only its length reaches
/// [`crate::telemetry::egress`], and even that is summed here first.
fn auth_header_bytes(headers: &[(&'static str, String)]) -> u64 {
    headers
        .iter()
        .map(|(n, v)| crate::telemetry::egress::header_line_bytes(n, v))
        .sum()
}

/// Wire bytes of the fixed (non-secret) headers a request sets.
fn egress_fixed_header_bytes(headers: &[(&str, &str)]) -> u64 {
    headers
        .iter()
        .map(|(n, v)| crate::telemetry::egress::header_line_bytes(n, v))
        .sum()
}

pub async fn send(
    provider: Provider,
    key: &str,
    cfg: &AiSettings,
    req: &AiRequest,
) -> Result<String> {
    let base = resolve_base_url(provider, cfg).await?;
    let w = wire::for_kind(provider.wire_kind());
    let body = w.build_body(&req.model, req);
    // Bounded so a black-holed Custom endpoint can't hang the Test-connection button.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let endpoint = w.endpoint(&base);
    let body_bytes = serde_json::to_vec(&body)?;
    let auth = w.auth_headers(key);
    // egress-seam: the JSON prompt body plus every header dat0 sets. The auth
    // header's LENGTH is counted; its value is never stored or logged.
    crate::telemetry::egress::record_request(
        "POST",
        &endpoint,
        auth_header_bytes(&auth)
            + egress_fixed_header_bytes(&[
                ("content-type", "application/json"),
                ("HTTP-Referer", "https://dat0.app"),
                ("X-Title", "dat0"),
            ]),
        body_bytes.len() as u64,
    );
    let mut rb = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body_bytes);
    for (name, value) in auth {
        rb = rb.header(name, value);
    }
    // OpenRouter courtesy identity headers (optional, harmless elsewhere).
    rb = rb
        .header("HTTP-Referer", "https://dat0.app")
        .header("X-Title", "dat0");
    let resp = rb.send().await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        // Surface the raw body (lossy) — non-JSON errors (proxy 502 HTML,
        // plaintext) are exactly what the user needs to diagnose a bad key/URL.
        bail!(
            "provider returned {}: {}",
            status,
            String::from_utf8_lossy(&bytes)
        );
    }
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| anyhow!("invalid JSON from provider: {e}"))?;
    w.parse_response(&json)
}

/// Streaming counterpart of [`send`]. SSRF-gated identically (via
/// `resolve_base_url`). Sets `stream: true` on the wire body, consumes the
/// chunked response through [`SseDecoder`] + `Wire::parse_sse_delta`, invokes
/// `on_delta` per incremental text delta, and returns the full text.
pub async fn send_stream(
    provider: Provider,
    key: &str,
    cfg: &AiSettings,
    req: &AiRequest,
    mut on_delta: impl FnMut(&str),
) -> Result<String> {
    let base = resolve_base_url(provider, cfg).await?;
    let w = wire::for_kind(provider.wire_kind());
    let mut body = w.build_body(&req.model, req);
    // Flip on streaming without touching the schema-only build_body (R17 path
    // unchanged): both wire shapes accept a top-level `stream` flag.
    body["stream"] = serde_json::json!(true);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120)) // streaming runs longer than a ping
        .build()?;
    let endpoint = w.endpoint(&base);
    let body_bytes = serde_json::to_vec(&body)?;
    let auth = w.auth_headers(key);
    // egress-seam: same accounting as `send`. Only the REQUEST is egress — the
    // streamed SSE response is ingress and is deliberately not counted.
    crate::telemetry::egress::record_request(
        "POST",
        &endpoint,
        auth_header_bytes(&auth)
            + egress_fixed_header_bytes(&[
                ("content-type", "application/json"),
                ("accept", "text/event-stream"),
                ("HTTP-Referer", "https://dat0.app"),
                ("X-Title", "dat0"),
            ]),
        body_bytes.len() as u64,
    );
    let mut rb = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .body(body_bytes);
    for (name, value) in auth {
        rb = rb.header(name, value);
    }
    rb = rb
        .header("HTTP-Referer", "https://dat0.app")
        .header("X-Title", "dat0");

    let resp = rb.send().await?;
    let status = resp.status();
    if !status.is_success() {
        let bytes = resp.bytes().await?;
        bail!(
            "provider returned {}: {}",
            status,
            String::from_utf8_lossy(&bytes)
        );
    }

    let mut decoder = SseDecoder::new();
    let mut full = String::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        for payload in decoder.feed(&chunk) {
            if let Some(text) = w.parse_sse_delta(&payload) {
                full.push_str(&text);
                on_delta(&text);
            }
        }
    }
    Ok(full)
}

/// Trivial round-trip used by the AI panel's Test-connection button.
pub async fn test_connection(provider: Provider, key: &str, cfg: &AiSettings) -> TestOutcome {
    let req = AiRequest::ping(&cfg.model);
    match send(provider, key, cfg, &req).await {
        Ok(_) => TestOutcome {
            ok: true,
            message: "Connected".into(),
        },
        Err(e) => TestOutcome {
            ok: false,
            message: e.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::Provider;
    use crate::ai::settings::AiSettings;

    #[tokio::test]
    async fn custom_http_localhost_is_refused_without_network() {
        let cfg = AiSettings {
            provider: Some("custom".into()),
            custom_base_url: "http://127.0.0.1:9".into(),
            advanced_override: false,
            ..Default::default()
        };
        let out = test_connection(Provider::Custom, "k", &cfg).await;
        assert!(!out.ok);
        assert!(out.message.to_lowercase().contains("http") || out.message.contains("127.0.0.1"));
    }
}
