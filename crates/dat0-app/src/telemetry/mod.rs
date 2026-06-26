//! Sentry telemetry initialization.
//!
//! Telemetry is opt-in: callers must pass `submission_enabled = true` for
//! events to be transmitted. The `before_send` hook applies
//! [`redaction::redact_event`] as a last-line defense against PII leakage
//! (absolute paths, locals, server hostname, user info).
//!
//! The Sentry DSN is baked at compile time via the `DAT0_GLITCHTIP_DSN_PUBLIC`
//! environment variable, supplied by `.cargo/config.toml` for local builds
//! and by CI for release builds. Compile-time embedding lets us avoid
//! per-invocation environment configuration in the desktop app.

pub mod crash;
pub mod redaction;

use anyhow::Result;
use sentry::ClientOptions;

const SENTRY_DSN_PUBLIC: &str = env!("DAT0_GLITCHTIP_DSN_PUBLIC");

/// Holds the Sentry client guard so the SDK flushes on drop.
pub struct Telemetry {
    _guard: Option<sentry::ClientInitGuard>,
}

impl Telemetry {
    /// Initialize Sentry. When `submission_enabled` is false, returns a
    /// no-op handle (no DSN parsed, no client started). When enabled,
    /// installs the `before_send` redaction hook.
    pub fn init(submission_enabled: bool) -> Result<Self> {
        if !submission_enabled {
            tracing::info!("telemetry submission disabled (opt-in off)");
            return Ok(Self { _guard: None });
        }
        let opts = ClientOptions {
            dsn: Some(SENTRY_DSN_PUBLIC.parse()?),
            release: Some(env!("CARGO_PKG_VERSION").into()),
            before_send: Some(std::sync::Arc::new(redaction::redact_event)),
            ..Default::default()
        };
        let guard = sentry::init(opts);
        Ok(Self {
            _guard: Some(guard),
        })
    }
}
