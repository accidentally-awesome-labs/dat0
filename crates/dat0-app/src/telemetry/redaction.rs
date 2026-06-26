//! Redaction for outbound Sentry events.
//!
//! Strips absolute filesystem paths and source-context locals from
//! `Event` payloads before they leave the process. We assume `before_send`
//! is the last line of defense and act conservatively: when in doubt,
//! redact.

use sentry::protocol::Event;
use std::path::Path;

/// Redact PII-bearing fields from a Sentry [`Event`].
///
/// Returns `Some(event)` after in-place mutation. We currently never drop
/// events here (returning `None` would suppress submission entirely); if
/// future policy changes require dropping certain events, do it from the
/// caller after inspecting the redacted payload.
pub fn redact_event(mut event: Event<'static>) -> Option<Event<'static>> {
    for ex in &mut event.exception.values {
        if let Some(value) = ex.value.take() {
            ex.value = Some(redact_text(&value));
        }
        if let Some(st) = ex.stacktrace.as_mut() {
            for frame in &mut st.frames {
                if let Some(filename) = frame.filename.take() {
                    frame.filename = Some(redact_path(&filename));
                }
                if let Some(abs) = frame.abs_path.take() {
                    frame.abs_path = Some(redact_path(&abs));
                }
                frame.vars.clear();
                frame.pre_context.clear();
                frame.post_context.clear();
                frame.context_line = None;
            }
        }
    }
    event.user = None;
    event.server_name = None;
    Some(event)
}

fn redact_path(s: &str) -> String {
    let p = Path::new(s);
    let basename = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<redacted>".into());
    format!("<redacted>/{basename}")
}

fn redact_text(s: &str) -> String {
    // Redact absolute path prefixes that commonly leak user identifiers:
    //   - macOS: /Users/<name>/...
    //   - Linux: /home/<name>/...
    //   - Windows: C:\<anything>\...
    // Each match replaces the whole path-like span (prefix + tail) with
    // "<redacted>". The two capture groups exist purely to bound the prefix
    // and consume any trailing path segments before the next whitespace or
    // delimiter.
    let re =
        regex::Regex::new(r#"(/Users/[^/\s]+|/home/[^/\s]+|[A-Z]:\\[^\\\s]+)([\\/][^"'\s,]*)?"#)
            .expect("redaction regex must compile");
    re.replace_all(s, "<redacted>").into_owned()
}

/// Public wrapper over the path-redaction used by both `before_send` and the
/// crash-staging payload builder.
pub fn redact_text_pub(s: &str) -> String {
    redact_text(s)
}
