//! Wizard trigger: confident CSV bypasses; ambiguous CSV opens drawer.

use dat0_app::error_ux::banner::drain_pending;
use dat0_app::file_drop::{DropOutcome, handle_drop};
use dat0_app::import_wizard::{SniffSummary, should_show_wizard, sniff};
use dat0_app::session::Session;
use parking_lot::Mutex;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn confident_csv_bypasses_wizard() {
    let s = SniffSummary {
        top_delimiter: ',',
        top_score: 0.98,
        next_score: 0.05,
        encoding_supported: true,
        any_low_confidence_column: false,
    };
    assert!(!should_show_wizard(&s));
}

#[test]
fn ambiguous_delimiter_opens_wizard() {
    let s = SniffSummary {
        top_delimiter: ',',
        top_score: 0.55,
        next_score: 0.53,
        encoding_supported: true,
        any_low_confidence_column: false,
    };
    assert!(should_show_wizard(&s));
}

#[test]
fn non_utf8_encoding_opens_wizard() {
    let s = SniffSummary {
        top_delimiter: ',',
        top_score: 0.95,
        next_score: 0.1,
        encoding_supported: false,
        any_low_confidence_column: false,
    };
    assert!(should_show_wizard(&s));
}

#[test]
fn low_column_confidence_opens_wizard() {
    let s = SniffSummary {
        top_delimiter: ',',
        top_score: 0.95,
        next_score: 0.1,
        encoding_supported: true,
        any_low_confidence_column: true,
    };
    assert!(should_show_wizard(&s));
}

/// Sanity: a well-formed UTF-8 CSV produces a confident summary (top=1.0,
/// next=0.0) — both sample sizes agree on the delimiter. This guards against
/// regressions in the dual-sniff agreement path documented in PD-011 §5.
#[test]
fn sniff_well_formed_csv_is_confident() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("a.csv");
    std::fs::write(&p, "x,y\n1,a\n2,b\n").unwrap();
    let s = sniff(&p).expect("sniff");
    assert!(
        !should_show_wizard(&s),
        "well-formed csv should be confident"
    );
    assert!(s.encoding_supported, "ascii bytes are valid utf-8");
}

/// Integration: a non-UTF-8 first 8 KB makes the sniff `encoding_supported`
/// false, which routes the drop to [`DropOutcome::OpenWizard`].
///
/// This exercises the wired branch in `file_drop::handle_one` (the dual-sniff
/// path can also force ambiguity, but UTF-8 head detection is cheap + safe to
/// construct in a unit test without needing a delimiter-tricky CSV body).
#[tokio::test]
async fn non_utf8_csv_drop_routes_to_open_wizard() {
    const BUDGET: u64 = 128 * 1024 * 1024;
    let _ = drain_pending();
    let tmp = TempDir::new().unwrap();
    let sess = Session::new(tmp.path(), BUDGET).await.unwrap();
    let arc = Arc::new(Mutex::new(sess));

    // 16 KB Latin-1 bytes (0xff) — fails str::from_utf8 in the first 8 KB
    // window. Extension is .csv so the sniff branch engages.
    let path = tmp.path().join("ambiguous.csv");
    std::fs::write(&path, vec![0xff_u8; 16 * 1024]).unwrap();

    let outcomes = handle_drop(vec![path.clone()], Arc::clone(&arc)).await;
    assert!(
        matches!(outcomes[0], DropOutcome::OpenWizard { path: ref p, .. } if p == &path),
        "expected OpenWizard outcome, got {:?}",
        outcomes[0]
    );
    assert!(
        arc.lock().tabs().is_empty(),
        "no tab should be added when wizard is opened",
    );
}
