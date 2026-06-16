//! Env-gated live AI round-trip. Required-green in CI (OPENROUTER_API_KEY set);
//! lenient-skip locally when the secret is absent (mirrors P9b's MD live test).

use dat0_app::ai::{Provider, settings::AiSettings, transport};

#[tokio::test]
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
