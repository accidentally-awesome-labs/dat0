//! Dock layout persistence — the real round trip through `session.json`.
//!
//! Ported from `dat0-app/tests/dock_layout_persist.rs` (B9). What that suite
//! proved is unchanged: a layout the user establishes reaches disk, comes back
//! verbatim, and costs nothing else in the file. What it walked to prove it is
//! gone twice over.
//!
//! * **No `DumpMirror`.** B9 captured layout by serializing
//!   `gpui_component::dock::DockArea::dump()` and parsing three slots out of
//!   it, because `Dock` emitted nothing on resize and the shell had to poll.
//!   The Dioxus shell owns `DockLayout` outright and a splitter drag *is* the
//!   event (`dat0-ui/src/components/dock.rs`), so there is no dump to mirror.
//! * **No `LeftPanel`.** S1 replaced the three-way left-rail mode switch with
//!   one always-present sidebar of three simultaneously-visible sections, so
//!   "which panel is showing" became `sidebar_open` + `sections_collapsed`.
//!   The three rail tests are ported onto those fields.
//!
//! Every assertion reads the layout back off DISK, never out of the live
//! session — re-reading a value the test just wrote would prove only that
//! assignment works, which was B9's own rule and is kept.
//!
//! The UI half — a toggle flipping the bit, the column collapsing to zero, a
//! drag writing a size — is `dat0-ui/tests/{right_dock,bottom_dock}.rs`. This
//! file is the file format and the disk.

use std::collections::BTreeSet;

use dat0_core::session::Session;
use dat0_core::session::dock_layout::{DOCK_MIN_SIZE, DockLayout, clamped_size};

const BUDGET: u64 = 128 * 1024 * 1024;

/// A layout with every field set to something distinguishable from its default,
/// so a dropped field shows up as a value that did not come back.
fn a_full_layout() -> DockLayout {
    DockLayout {
        left_panel: None,
        left_size: None,
        inspector_visible: true,
        charts_visible: true,
        right_size: Some(412),
        console_open: true,
        bottom_size: Some(377),
        sidebar_open: false,
        sidebar_size: Some(291),
        sections_collapsed: ["connections".to_string(), "packages".to_string()]
            .into_iter()
            .collect(),
    }
}

/// Read the layout back off DISK — never out of the live session.
///
/// Goes through `migrate::load_str` rather than a bare `serde_json::from_str`
/// so the read exercises the same version-probing path a real launch takes.
fn layout_on_disk(session: &Session) -> Option<DockLayout> {
    let raw = std::fs::read_to_string(session.home.root_dir().join("session.json"))
        .expect("session.json exists");
    dat0_core::session::migrate::load_str(&raw)
        .expect("session.json parses")
        .dock_layout
}

fn raw_on_disk(session: &Session) -> String {
    std::fs::read_to_string(session.home.root_dir().join("session.json"))
        .expect("session.json exists")
}

// ── capture ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn an_open_console_reaches_the_session_file() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");

    sess.set_dock_layout(Some(DockLayout {
        console_open: true,
        bottom_size: Some(320),
        ..DockLayout::default()
    }))
    .expect("set_dock_layout");

    let layout = layout_on_disk(&sess).expect("a layout was persisted");
    assert!(layout.console_open, "an open console is persisted");
    assert_eq!(
        layout.bottom_size,
        Some(320),
        "and so is the height it was opened at"
    );
}

#[tokio::test]
async fn closing_the_console_persists_the_closed_state() {
    // The bidirectional half. A capture path that only ever wrote `true` would
    // pass every other test in this file.
    let tmp = tempfile::tempdir().unwrap();
    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");

    sess.set_dock_layout(Some(DockLayout {
        console_open: true,
        ..DockLayout::default()
    }))
    .unwrap();
    assert!(layout_on_disk(&sess).unwrap().console_open, "seed: open");

    sess.set_dock_layout(Some(DockLayout {
        console_open: false,
        ..DockLayout::default()
    }))
    .unwrap();

    assert!(
        !layout_on_disk(&sess).unwrap().console_open,
        "closing the console must reach disk as closed, not merely stop being written"
    );
}

#[tokio::test]
async fn each_right_pane_persists_independently_of_the_other() {
    // S5: the right column is a stack of two independently collapsible panes,
    // not one reserved split. Two bools, and either alone opens the column —
    // so a layout that could only record "the right dock is open" would lose
    // which pane the user actually left showing.
    let tmp = tempfile::tempdir().unwrap();
    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");

    sess.set_dock_layout(Some(DockLayout {
        charts_visible: true,
        ..DockLayout::default()
    }))
    .unwrap();

    let charts_only = layout_on_disk(&sess).unwrap();
    assert!(charts_only.charts_visible);
    assert!(
        !charts_only.inspector_visible,
        "showing charts must not drag the inspector open with it"
    );
    assert!(
        charts_only.right_open(),
        "either pane alone means the column has width"
    );

    sess.set_dock_layout(Some(DockLayout {
        inspector_visible: true,
        ..DockLayout::default()
    }))
    .unwrap();

    let inspector_only = layout_on_disk(&sess).unwrap();
    assert!(inspector_only.inspector_visible);
    assert!(!inspector_only.charts_visible);
    assert!(inspector_only.right_open());
}

#[tokio::test]
async fn a_hidden_sidebar_persists_as_hidden_rather_than_defaulting_open() {
    // ⚠ The one field whose default is `true`. `sidebar_open` is
    // `#[serde(default = "yes")]`, so a serializer that skipped `false` — the
    // shape every other optional field in this struct uses — would read back
    // as OPEN and silently undo every ⌘B the user ever pressed.
    let tmp = tempfile::tempdir().unwrap();
    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");

    sess.set_dock_layout(Some(DockLayout {
        sidebar_open: false,
        ..DockLayout::default()
    }))
    .unwrap();

    let raw = raw_on_disk(&sess);
    assert!(
        raw.contains("\"sidebar_open\""),
        "the closed state must be WRITTEN, not omitted and re-defaulted: {raw}"
    );
    assert!(
        !layout_on_disk(&sess).unwrap().sidebar_open,
        "a hidden sidebar comes back hidden"
    );
}

#[tokio::test]
async fn a_resized_sidebar_persists_its_width() {
    // The GPUI suite's `left_size`, on the surface that replaced the left dock.
    let tmp = tempfile::tempdir().unwrap();
    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");

    sess.set_dock_layout(Some(DockLayout {
        sidebar_size: Some(291),
        ..DockLayout::default()
    }))
    .unwrap();

    assert_eq!(layout_on_disk(&sess).unwrap().sidebar_size, Some(291));
}

#[tokio::test]
async fn collapsed_sidebar_sections_persist_by_name() {
    // S1's replacement for `left_panel`: three sections visible at once, each
    // independently collapsible, stored as named data so a fourth section is
    // not a schema change.
    let tmp = tempfile::tempdir().unwrap();
    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");

    sess.set_dock_layout(Some(DockLayout {
        sections_collapsed: ["packages".to_string()].into_iter().collect(),
        ..DockLayout::default()
    }))
    .unwrap();

    let one = layout_on_disk(&sess).unwrap();
    assert_eq!(
        one.sections_collapsed,
        BTreeSet::from(["packages".to_string()])
    );

    sess.set_dock_layout(Some(DockLayout {
        sections_collapsed: ["files".to_string(), "packages".to_string()]
            .into_iter()
            .collect(),
        ..DockLayout::default()
    }))
    .unwrap();

    assert_eq!(
        layout_on_disk(&sess).unwrap().sections_collapsed,
        BTreeSet::from(["files".to_string(), "packages".to_string()]),
        "collapsing a second section adds to the set rather than replacing it"
    );
}

#[tokio::test]
async fn every_layout_field_survives_the_session_file() {
    // Field-by-field is how a dropped `skip_serializing_if`, a renamed key or a
    // new field with no wire coverage shows up. The three sidebar fields are
    // the ones with no GPUI ancestor, and therefore the ones nothing else
    // pins.
    let tmp = tempfile::tempdir().unwrap();
    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");

    let want = a_full_layout();
    sess.set_dock_layout(Some(want.clone())).unwrap();

    assert_eq!(
        layout_on_disk(&sess).expect("a layout was persisted"),
        want,
        "every field round-trips, including sidebar_open / sidebar_size / \
         sections_collapsed"
    );
}

#[tokio::test]
async fn a_layout_written_twice_keeps_only_the_last() {
    // The GPUI suite proved this of the left rail ("replaces rather than
    // accumulates"). The claim generalises: the file holds one layout, so a
    // second write cannot leave the first one's values behind.
    let tmp = tempfile::tempdir().unwrap();
    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");

    sess.set_dock_layout(Some(a_full_layout())).unwrap();
    sess.set_dock_layout(Some(DockLayout::default())).unwrap();

    assert_eq!(
        layout_on_disk(&sess).unwrap(),
        DockLayout::default(),
        "nothing of the first layout survives the second write"
    );
}

#[tokio::test]
async fn the_persisted_layout_never_contains_the_centre() {
    // Structural in the new build — `DockLayout` has no field a centre could
    // occupy — but the claim is about the FILE, and the file is what a future
    // "persist the whole dock tree" change would widen. Under GPUI a restored
    // centre came back as `DockItem::tabs` and regained the 30px title bar; in
    // the Dioxus shell the grid is not a pane at all, and a centre on disk
    // would be a restore path with nothing to restore into.
    let tmp = tempfile::tempdir().unwrap();
    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");
    sess.set_dock_layout(Some(a_full_layout())).unwrap();

    let raw = raw_on_disk(&sess);
    assert!(
        !raw.contains("GridPanel") && !raw.contains("\"center\""),
        "session.json must carry the layout only, never the centre: {raw}"
    );
}

#[tokio::test]
async fn a_fresh_session_persists_no_layout_and_reads_as_the_default() {
    // The default is not "everything closed": S1 opens the sidebar, because a
    // workbench with no visible catalog on first launch looks broken.
    let tmp = tempfile::tempdir().unwrap();
    let sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");

    assert!(
        sess.dock_layout().is_none(),
        "a fresh session writes no layout at all"
    );
    assert!(
        layout_on_disk(&sess).is_none(),
        "and none reaches the file either"
    );

    let fresh = DockLayout::default();
    assert!(!fresh.console_open, "the console starts closed");
    assert!(!fresh.right_open(), "the right column starts collapsed");
    assert!(fresh.sidebar_open, "the sidebar starts open");
    assert!(fresh.sections_collapsed.is_empty());
}

// ── restore ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_layout_survives_a_close_and_reopen() {
    // The whole point of persisting: the shape of reopening a workspace, or of
    // recovering an orphaned session after a crash.
    let tmp = tempfile::tempdir().unwrap();

    let scratch_dir = {
        let mut sess = Session::new(tmp.path(), BUDGET)
            .await
            .expect("Session::new");
        sess.set_dock_layout(Some(a_full_layout())).unwrap();
        sess.home.root_dir().to_path_buf()
        // sess drops here, releasing the engine and the DB file handle.
    };

    let reopened = Session::recover(scratch_dir, BUDGET)
        .await
        .expect("Session::recover");

    assert_eq!(
        reopened.dock_layout().cloned(),
        Some(a_full_layout()),
        "the layout came back through a full close and reopen"
    );
}

#[tokio::test]
async fn a_layout_saved_on_a_bigger_display_is_clamped_when_it_is_mounted() {
    // A layout saved on a 4K display, reopened on a laptop. Restoring the
    // number verbatim would put the centre entirely off screen with the
    // splitter unreachable and no in-app way back, so the FILE keeps what the
    // user chose and the MOUNT clamps it.
    let tmp = tempfile::tempdir().unwrap();

    let scratch_dir = {
        let mut sess = Session::new(tmp.path(), BUDGET)
            .await
            .expect("Session::new");
        sess.set_dock_layout(Some(DockLayout {
            sidebar_size: Some(30_000),
            ..DockLayout::default()
        }))
        .unwrap();
        sess.home.root_dir().to_path_buf()
    };

    let reopened = Session::recover(scratch_dir, BUDGET).await.unwrap();
    let persisted = reopened.dock_layout().unwrap().sidebar_size;
    assert_eq!(
        persisted,
        Some(30_000),
        "the file keeps what the user chose"
    );

    // 1440px laptop: 80% of the axis, never the 30 000 on the wire.
    let mounted = clamped_size(persisted, 238.0, 1440.0);
    assert_eq!(mounted, 1152.0);
    assert!(
        mounted >= DOCK_MIN_SIZE && mounted < 30_000.0,
        "an oversized sidebar is clamped at mount, not obeyed; got {mounted}"
    );
}

// ── blast radius ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn persisting_a_layout_never_writes_settings_toml() {
    // `SettingsWatcher` re-reads settings.toml on every write, and the file is
    // otherwise written only on deliberate user action. A layout write per
    // splitter move would widen the window in which a load-mutate-save
    // clobbers a hand-edit in flight.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    let settings_path = cfg.join("settings.toml");
    let store = dat0_core::settings::store::SettingsStore::with_path(settings_path.clone());
    store.save(&store.load_or_default().unwrap()).unwrap();
    let before = std::fs::read_to_string(&settings_path).expect("settings written");

    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");
    sess.set_dock_layout(Some(a_full_layout())).unwrap();

    assert_eq!(
        before,
        std::fs::read_to_string(&settings_path).expect("settings still there"),
        "a layout write must leave settings.toml byte-identical"
    );
}

#[tokio::test]
async fn a_corrupt_layout_costs_the_layout_and_nothing_else() {
    // Layout is the least valuable thing in the file. A hand-edit, a truncated
    // write or a downgrade must never be able to cost a user their tabs.
    let tmp = tempfile::tempdir().unwrap();
    let mut sess = Session::new(tmp.path(), BUDGET)
        .await
        .expect("Session::new");
    sess.set_dock_layout(Some(a_full_layout())).unwrap();
    let path = sess.home.root_dir().join("session.json");
    drop(sess);

    let raw = std::fs::read_to_string(&path).unwrap();
    let mut doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
    doc["dock_layout"] = serde_json::json!("corrupt");
    std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();

    let state = dat0_core::session::migrate::load_str(&std::fs::read_to_string(&path).unwrap())
        .expect("a corrupt layout must not fail the document");
    assert!(
        state.dock_layout.is_none(),
        "the layout degrades to the default"
    );
    assert_eq!(
        state.schema_version,
        dat0_core::session::SESSION_SCHEMA_VERSION,
        "and the rest of the document loads untouched"
    );
}
