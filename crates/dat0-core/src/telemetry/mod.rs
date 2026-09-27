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
//!
//! [`egress`] is the odd one out and deliberately so: it is not Sentry, not
//! opt-in, and never leaves the process. It counts what dat0 puts on the wire
//! so the status bar's "0 B egress" is a measurement rather than a promise.

pub mod crash;
pub mod egress;
pub mod redaction;
pub mod report_logic;

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

/// Whether crash-report submission is on now, as the settings file says.
///
/// Read when a report is sent rather than taken from launch, where the client
/// is bound: an opt-out made mid-session used to go on sending until the next
/// launch, and an opt-in made mid-session sent nothing until then. Off when
/// the settings cannot be read: the privacy-safe answer.
pub fn submission_allowed() -> bool {
    crate::platform::config_dir()
        .ok()
        .and_then(|dir| {
            crate::settings::store::SettingsStore::with_path(dir.join("settings.toml"))
                .load_or_default()
                .ok()
        })
        .is_some_and(|s| s.telemetry.crash_submission_enabled)
}

/// What became of a report the user chose to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submission {
    /// Handed to the transport, and counted as egress.
    Sent,
    /// Crash reports are off: nothing left this machine.
    Off,
}

/// A client bound after launch, when the opt-in came mid-session. Kept for
/// the rest of the run, as the launch's own guard is.
static LATE: std::sync::Mutex<Option<Telemetry>> = std::sync::Mutex::new(None);

/// Send a report the user chose to send, if the settings allow it now.
fn submit_user(
    level: Level,
    kind: &str,
    message: String,
    note: Option<&str>,
    release: Option<&str>,
    backtrace: Option<&str>,
) -> Submission {
    if !submission_allowed() {
        return Submission::Off;
    }
    if !is_active() {
        match Telemetry::init(true) {
            Ok(t) => {
                if let Ok(mut late) = LATE.lock() {
                    *late = Some(t);
                }
            }
            Err(e) => {
                tracing::warn!("telemetry: could not start the client: {e:#}");
                return Submission::Off;
            }
        }
    }
    if capture(level, kind, message, note, release, backtrace) {
        Submission::Sent
    } else {
        Submission::Off
    }
}

/// Capture a structured event, and count it as egress. No-op when inactive;
/// true when the event went to the transport.
fn capture(
    level: Level,
    kind: &str,
    message: String,
    note: Option<&str>,
    release: Option<&str>,
    backtrace: Option<&str>,
) -> bool {
    if !is_active() {
        return false;
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
    if let Some(bt) = backtrace {
        // Staged backtraces are already redacted at panic time (crash.rs); the
        // before_send hook re-applies redact_event to every extra String as a
        // second pass, so there is no new PII surface here.
        event
            .extra
            .insert("backtrace".into(), Value::String(bt.to_string()));
    }
    // egress-seam: the event, as JSON, is what the request carries; the
    // envelope's framing and the transport's own headers are not counted,
    // as at every seam (`egress`). Sentry's client sends it, over its own
    // connection, so nothing else here would see these bytes leave.
    let body = serde_json::to_vec(&event).map_or(0, |b| b.len() as u64);
    let url = SENTRY_DSN_PUBLIC
        .parse::<sentry::types::Dsn>()
        .map(|d| d.envelope_api_url().to_string())
        .unwrap_or_default();
    sentry::capture_event(event); // before_send redaction still applies
    egress::record_request("POST", &url, 0, body);
    if let Some(c) = sentry::Hub::current().client() {
        c.flush(Some(Duration::from_secs(5)));
    }
    true
}

/// Submit a staged crash (with optional user note), when the settings allow
/// it now.
pub fn submit_staged(crash: &crash::StagedCrash, note: Option<&str>) -> Submission {
    submit_user(
        Level::Error,
        "crash",
        crash.message.clone(),
        note,
        Some(&crash.version),
        Some(&crash.backtrace),
    )
}

/// Submit a user-initiated bug report, when the settings allow it now.
pub fn submit_report(note: &str) -> Submission {
    submit_user(
        Level::Info,
        "report-a-bug",
        "User bug report".to_string(),
        Some(note),
        None,
        None,
    )
}

/// Submit an operator/CI test event. The identifier is the event MESSAGE so it
/// becomes the GlitchTip issue title (searchable), unlike a bug-report note
/// which lands only in `extra.user_note`. No-op when inactive.
pub fn submit_test_event(message: &str) {
    let _ = capture(
        Level::Info,
        "e2e-test",
        message.to_string(),
        None,
        None,
        None,
    );
}
