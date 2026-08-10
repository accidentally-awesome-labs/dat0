use dat0_core::telemetry::redaction::redact_event;
use sentry::protocol::{Breadcrumb, Event, Frame, Map, Stacktrace, Value};

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

#[test]
fn redacts_message_extra_and_breadcrumbs() {
    let mut extra = Map::new();
    extra.insert(
        "query".into(),
        Value::String("SELECT * FROM /home/bob/t".into()),
    );
    let mut event = Event {
        message: Some("loaded /Users/bob/data/secret.csv".into()),
        extra,
        ..Default::default()
    };
    event.breadcrumbs.values.push(Breadcrumb {
        message: Some("opened /Users/bob/x.parquet".into()),
        ..Default::default()
    });

    let r = redact_event(event).unwrap();
    assert!(!r.message.as_deref().unwrap().contains("/Users/bob"));
    let q = match r.extra.get("query") {
        Some(Value::String(s)) => s.clone(),
        other => panic!("expected redacted query string, got {other:?}"),
    };
    assert!(!q.contains("/home/bob"), "query extra leaked path: {q}");
    assert!(
        !r.breadcrumbs.values[0]
            .message
            .as_deref()
            .unwrap()
            .contains("/Users/bob")
    );
}
