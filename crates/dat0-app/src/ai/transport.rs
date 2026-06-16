//! Single network seam: build + send one request through SSRF + Wire + reqwest.

use std::time::Duration;

use anyhow::{Result, anyhow, bail};

use crate::ai::provider::Provider;
use crate::ai::request::AiRequest;
use crate::ai::settings::AiSettings;
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

pub async fn send(provider: Provider, key: &str, cfg: &AiSettings, req: &AiRequest) -> Result<String> {
    let base = resolve_base_url(provider, cfg).await?;
    let w = wire::for_kind(provider.wire_kind());
    let body = w.build_body(&req.model, req);
    // Bounded so a black-holed Custom endpoint can't hang the Test-connection button.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let mut rb = client
        .post(w.endpoint(&base))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&body)?);
    for (name, value) in w.auth_headers(key) {
        rb = rb.header(name, value);
    }
    // OpenRouter courtesy identity headers (optional, harmless elsewhere).
    rb = rb.header("HTTP-Referer", "https://dat0.app").header("X-Title", "dat0");
    let resp = rb.send().await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        // Surface the raw body (lossy) — non-JSON errors (proxy 502 HTML,
        // plaintext) are exactly what the user needs to diagnose a bad key/URL.
        bail!("provider returned {}: {}", status, String::from_utf8_lossy(&bytes));
    }
    let json: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| anyhow!("invalid JSON from provider: {e}"))?;
    w.parse_response(&json)
}

/// Trivial round-trip used by the AI panel's Test-connection button.
pub async fn test_connection(provider: Provider, key: &str, cfg: &AiSettings) -> TestOutcome {
    let req = AiRequest::ping(&cfg.model);
    match send(provider, key, cfg, &req).await {
        Ok(_) => TestOutcome { ok: true, message: "Connected".into() },
        Err(e) => TestOutcome { ok: false, message: e.to_string() },
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
