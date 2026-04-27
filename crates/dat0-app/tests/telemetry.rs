use dat0_app::telemetry::redaction::redact_event;
use sentry::protocol::{Event, Frame, Stacktrace};

#[test]
fn redacts_absolute_paths() {
    let mut event = Event::default();
    event.exception.values.push(sentry::protocol::Exception {
        ty: "Panic".into(),
        value: Some("at /Users/alice/secret/project/src/foo.rs:42".into()),
        stacktrace: Some(Stacktrace {
            frames: vec![Frame {
                filename: Some("/Users/alice/secret/project/src/foo.rs".into()),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });

    let redacted = redact_event(event).unwrap();
    let frame = &redacted.exception.values[0]
        .stacktrace
        .as_ref()
        .unwrap()
        .frames[0];
    assert_eq!(frame.filename.as_deref(), Some("<redacted>/foo.rs"));
    let val = redacted.exception.values[0].value.as_deref().unwrap();
    assert!(
        !val.contains("/Users/alice"),
        "absolute path leaked into value: {val}"
    );
}
