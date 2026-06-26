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
use sentry::protocol::{Event, Level, Value};
use std::time::Duration;

const SENTRY_DSN_PUBLIC: &str = env!("DAT0_GLITCHTIP_DSN_PUBLIC");

/// Holds the Sentry client guard so the SDK flushes on drop.
pub struct Telemetry {
    _guard: Option<sentry::ClientInitGuard>,
}

impl Telemetry {
    /// Initialize Sentry. When `submission_enabled` is false, returns a
    /// no-op handle (no DSN parsed, no client started). When enabled,
    /// installs the `before_send` redaction hook.
    ///
    /// # No-double-send invariant
    ///
    /// We own crash submission via the staging path (`crash::install_panic_hook`
    /// → `last-crash.json` → relaunch dialog → `submit_staged`). Sentry's
    /// default `PanicIntegration` installs its own panic hook during `init` and
    /// would auto-send an event at crash time; our hook chains to the *previous*
    /// hook via `std::panic::take_hook()`, so if `PanicIntegration` is active,
    /// our chain calls into it and produces a second event without the user's
    /// note. To prevent this, we disable `default_integrations` and explicitly
    /// re-add every default integration EXCEPT `PanicIntegration`. The enabled
    /// sentry features in this project are "backtrace" + "panic", so we add
    /// `AttachStacktraceIntegration` and `ProcessStacktraceIntegration`, and
    /// intentionally omit `PanicIntegration`.
    pub fn init(submission_enabled: bool) -> Result<Self> {
        if !submission_enabled {
            tracing::info!("telemetry submission disabled (opt-in off)");
            return Ok(Self { _guard: None });
        }
        let opts = ClientOptions {
            dsn: Some(SENTRY_DSN_PUBLIC.parse()?),
            release: Some(env!("CARGO_PKG_VERSION").into()),
            before_send: Some(std::sync::Arc::new(redaction::redact_event)),
            // IMPORTANT: disable default integrations so PanicIntegration is
            // NOT installed. See doc-comment above for the full rationale.
            // We manually re-add the backtrace integrations below.
            default_integrations: false,
            ..Default::default()
        }
        .add_integration(sentry::integrations::backtrace::AttachStacktraceIntegration)
        .add_integration(sentry::integrations::backtrace::ProcessStacktraceIntegration);
        let guard = sentry::init(opts);
        Ok(Self {
            _guard: Some(guard),
        })
    }
}

/// Returns `true` when a live Sentry client is bound (telemetry opt-in is on).
pub fn is_active() -> bool {
    sentry::Hub::current().client().is_some()
}

/// Capture a structured event. No-op when inactive.
fn capture(level: Level, kind: &str, message: String, note: Option<&str>, release: Option<&str>) {
    if !is_active() {
        return;
    }
    let mut event = Event {
        level,
        message: Some(message),
        ..Default::default()
    };
    event.tags.insert("kind".into(), kind.into());
    if let Some(r) = release {
        event.release = Some(r.to_string().into());
    }
    if let Some(n) = note {
        event
            .extra
            .insert("user_note".into(), Value::String(n.to_string()));
    }
    sentry::capture_event(event); // before_send redaction still applies
    if let Some(c) = sentry::Hub::current().client() {
        c.flush(Some(Duration::from_secs(5)));
    }
}

/// Submit a staged crash (with optional user note). No-op when inactive.
pub fn submit_staged(crash: &crash::StagedCrash, note: Option<&str>) {
    capture(
        Level::Error,
        "crash",
        crash.message.clone(),
        note,
        Some(&crash.version),
    );
}

/// Submit a user-initiated bug report. No-op when inactive.
pub fn submit_report(note: &str) {
    capture(
        Level::Info,
        "report-a-bug",
        "User bug report".to_string(),
        Some(note),
        None,
    );
}
