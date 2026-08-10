//! Gate the *non-empty* `charts` array wire shape of a session (the existing
//! session-format snapshot only proves `[]`). Fixture uses a hardcoded UUID +
//! fixed `saved_at` so the snapshot is deterministic (production builds these
//! from `Uuid::now_v7()` + `now_unix_millis()`).

use dat0_core::session::SessionState;
use dat0_core::session::charts::SavedChart;
use dat0_engine::chart_spec::{ChartSpec, ChartType};

fn state_with_chart() -> SessionState {
    SessionState {
        charts: vec![SavedChart {
            id: uuid::Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001),
            name: "Monthly totals".into(),
            spec: ChartSpec {
                chart_type: ChartType::Bar,
                source: "\"sales\"".into(),
                x: Some("month".into()),
                y: Some("total".into()),
                group: None,
                color: None,
                title: "Monthly totals".into(),
            },
            saved_at: 1_700_000_000_000,
        }],
        ..Default::default()
    }
}

#[test]
fn populated_chart_session_json_wire_format() {
    // NB: `insta::assert_json_snapshot!` requires insta's `json` feature, which
    // is NOT enabled in this workspace (Cargo.lock's `insta` entry carries no
    // serde/serde_json dep edge) — enabling it would touch Cargo.toml +
    // Cargo.lock, violating the zero-new-deps constraint. Mirror the working
    // pattern from `session_migration.rs`'s `session_json_wire_format_is_snapshot_gated`
    // instead: `assert_snapshot!` on the exact on-disk serializer output
    // (`to_string_pretty`, matching `Session::persist`).
    let json = serde_json::to_string_pretty(&state_with_chart()).unwrap();
    insta::assert_snapshot!("populated_chart_session_json", json);
}

#[test]
fn saved_chart_survives_session_json_round_trip() {
    let json = serde_json::to_string_pretty(&state_with_chart()).unwrap();
    let back: SessionState = serde_json::from_str(&json).unwrap();
    assert_eq!(back.charts.len(), 1);
    assert_eq!(back.charts[0].name, "Monthly totals");
    assert_eq!(back.charts[0].spec.chart_type, ChartType::Bar);
    assert_eq!(back.charts[0].spec.x.as_deref(), Some("month"));
    assert_eq!(back.charts[0].spec.y.as_deref(), Some("total"));
    assert_eq!(back.charts[0].saved_at, 1_700_000_000_000);
}
