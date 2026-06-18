//! Env-gated live AI round-trip. Required-green in CI (OPENROUTER_API_KEY set);
//! lenient-skip locally when the secret is absent (mirrors P9b's MD live test).
//!
//! `#[ignore]`d so the default `cargo test --workspace` Test step does NOT hit
//! the live OpenRouter endpoint — the dedicated CI step runs them via
//! `--include-ignored --test-threads=1` (mirrors the MotherDuck `md_attach`
//! gate). This keeps the workspace test step hermetic and runs the live calls
//! exactly once, serially: firing them twice (workspace + dedicated) and in
//! parallel previously tripped a per-key rate window → empty stream.

use dat0_app::ai::{Provider, settings::AiSettings, transport};

#[tokio::test]
#[ignore = "live OpenRouter API; run via the dedicated CI step (--include-ignored)"]
async fn openrouter_test_connection_round_trip() {
    let Ok(key) = std::env::var("OPENROUTER_API_KEY") else {
        eprintln!("skip: OPENROUTER_API_KEY not set");
        return;
    };
    let cfg = AiSettings {
        enabled: true,
        provider: Some("openrouter".into()),
        // Cheap paid model (~$0.00001/run); reliable for a required-green gate.
        model: "deepseek/deepseek-v4-flash".into(),
        ..Default::default()
    };
    let out = transport::test_connection(Provider::OpenRouter, &key, &cfg).await;
    assert!(out.ok, "live round-trip failed: {}", out.message);
}

#[tokio::test]
#[ignore = "live OpenRouter API; run via the dedicated CI step (--include-ignored)"]
async fn openrouter_stream_collects_deltas() {
    let Ok(key) = std::env::var("OPENROUTER_API_KEY") else {
        eprintln!("OPENROUTER_API_KEY unset; skipping live stream test");
        return;
    };
    use dat0_app::ai::request::AiRequest;
    let cfg = AiSettings {
        enabled: true,
        provider: Some("openrouter".into()),
        model: "deepseek/deepseek-v4-flash".into(),
        ..Default::default()
    };
    let mut req = AiRequest::ping(&cfg.model);
    req.max_tokens = 32;
    let mut deltas = 0usize;
    let full = transport::send_stream(Provider::OpenRouter, &key, &cfg, &req, |_d| deltas += 1)
        .await
        .expect("stream ok");
    assert!(deltas >= 1, "expected ≥1 delta");
    assert!(!full.trim().is_empty(), "expected non-empty completion");
}
