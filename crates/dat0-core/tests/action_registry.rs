//! `ActionRegistry` + built-in action descriptors (P3b T3 + T8 + P4b T9 + P5a T11).
//!
//! Verifies the registry shape and the baseline `register_all` built-ins.
//! T3 shipped seven descriptors; T8 added `sample_data.retry_taxi` (banner
//! Retry button for the offline fetch-failed UX), bringing the count to
//! eight. P4a T13 added `view.undo` + `view.redo` → ten. P4b T9 adds seven
//! edit / clipboard / bulk-op actions → seventeen. P4c T8 added
//! `view.delete_column` → eighteen; P4c T11 added `view.export` → nineteen.
//! P5a T11 adds five SQL Console entry points (console.toggle / sql.run /
//! sql.cancel / sql.new_tab / sql.close_tab) → twenty-four. P5b T12 adds five
//! SQL Console reuse/promotion descriptors (sql.save_query / sql.load_query /
//! sql.history / sql.save_as_table / view.save_as_table) → twenty-nine. P7a T7
//! adds two workspace actions (workspace.open / workspace.save) → thirty-one.
//! Banner action_ids (T2) reference these stable strings. Downstream tasks
//! (T5 recovery panel, T7 empty-state hero, T10 import cancel, T11 file dialog,
//! T12 theme toggle) will replace stub dispatch bodies with real wiring —
//! registry shape itself is frozen here.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dat0_core::actions::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry};

#[test]
fn register_then_iter() {
    let reg = ActionRegistry::new();
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    reg.register(ActionDescriptor {
        id: ActionId::from("test.noop"),
        title: "Test Noop".into(),
        group: ActionGroup::Navigation,
        dispatch: Arc::new(move |_app| {
            c.fetch_add(1, Ordering::SeqCst);
        }),
    })
    .unwrap();

    let items: Vec<_> = reg.iter().collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id.as_str(), "test.noop");
}

#[test]
fn duplicate_id_rejected() {
    let reg = ActionRegistry::new();
    let desc = || ActionDescriptor {
        id: ActionId::from("test.noop"),
        title: "Test".into(),
        group: ActionGroup::Navigation,
        dispatch: Arc::new(|_| {}),
    };
    reg.register(desc()).unwrap();
    let err = reg.register(desc()).unwrap_err();
    assert!(matches!(
        err,
        dat0_core::actions::registry::RegisterError::DuplicateId(_)
    ));
}

#[test]
fn lookup_returns_descriptor() {
    let reg = ActionRegistry::new();
    reg.register(ActionDescriptor {
        id: ActionId::from("file.open"),
        title: "Open File".into(),
        group: ActionGroup::File,
        dispatch: Arc::new(|_| {}),
    })
    .unwrap();
    let d = reg.get(&ActionId::from("file.open")).unwrap();
    assert_eq!(d.title, "Open File");
}

#[test]
fn builtins_register_the_expected_count() {
    let reg = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&reg).unwrap();
    // Ten from P3b/P4a + seven from P4b T9 (copy/cut/paste/fill_down/set_null/set_value/delete_rows)
    // + one from P4c T8 (delete_column) + one from P4c T11 (view.export)
    // + five from P5a T11 (console.toggle/sql.run/sql.cancel/sql.new_tab/sql.close_tab) = 24.
    // + five from P5b T12 (sql.save_query/sql.load_query/sql.history/sql.save_as_table/view.save_as_table) = 29.
    // + two from P7a T7 (workspace.open/workspace.save) = 31.
    // + one from P7c T5 (live.refresh — the live-data Refresh banner button) = 32.
    // + one from P9a T7 (chart.visualize — the right-dock chart panel toggle) = 33.
    // + one from P9c-1 T9 (ai.panel.open — the left-dock AI panel toggle) = 34.
    // + one from P11a T7 (onboarding.take_tour — tour re-entry for palette) = 35.
    // + two from UI-redesign B6 (chart.export.png/svg — the keyboard path for
    //   export, whose buttons move to the dock title bar where upstream forces
    //   tab_stop(false)) = 37.
    // + one from MX1 (perf.hud.toggle — the frame-interval HUD; palette-only,
    //   because a chord would spend one of the few free ones on a diagnostic) = 38.
    // + one from EN4 (session.retry — the `SessionSlot::Failed` banner's only
    //   way out; the boot deliberately never retries on its own) = 39.
    // + one from the GPUI→Dioxus migration (sidebar.toggle — S1 folds the
    //   three-way left dock into one always-present catalog sidebar, so
    //   "hide it" becomes a command for the first time; ⌘B) = 40.
    //
    // The name deliberately carries no number: it said "thirty_four" while
    // asserting 35, and a count in a test name rots on the next slice that adds
    // an action. The ledger above is the real record.
    assert_eq!(reg.count(), 40);
    let titles: Vec<String> = reg.iter().map(|d| d.title).collect();
    assert!(titles.contains(&"New Window".to_string()));
    assert!(titles.contains(&"Open Settings".to_string()));
    assert!(titles.contains(&"Retry NYC Taxi download".to_string()));
    assert!(titles.contains(&"Undo".to_string()));
    assert!(titles.contains(&"Redo".to_string()));
    // T9 additions
    assert!(titles.contains(&"Copy".to_string()));
    assert!(titles.contains(&"Cut".to_string()));
    assert!(titles.contains(&"Paste".to_string()));
    assert!(titles.contains(&"Fill Down".to_string()));
    assert!(titles.contains(&"Set NULL".to_string()));
    assert!(titles.contains(&"Set Value…".to_string()));
    assert!(titles.contains(&"Delete Row(s)".to_string()));
    // T8 addition
    assert!(titles.contains(&"Delete Column".to_string()));
    // P4c T11 addition
    assert!(titles.contains(&"Export…".to_string()));
    // P5a T11 additions (SQL Console entry points)
    assert!(titles.contains(&"Toggle SQL Console".to_string()));
    assert!(titles.contains(&"Run".to_string()));
    assert!(titles.contains(&"Cancel".to_string()));
    assert!(titles.contains(&"New query tab".to_string()));
    assert!(titles.contains(&"Close query tab".to_string()));
    // P5b T12 additions (SQL Console reuse/promotion descriptors). Checked by
    // id, not title: `sql.save_as_table` + `view.save_as_table` share the
    // "Save as Table…" title, so a title-based check can't distinguish them.
    for id in [
        "sql.save_query",
        "sql.load_query",
        "sql.history",
        "sql.save_as_table",
        "view.save_as_table",
    ] {
        assert!(reg.contains(id), "missing {id}");
    }
    // P7a T7 additions (workspace open/save).
    for id in ["workspace.open", "workspace.save"] {
        assert!(reg.contains(id), "missing {id}");
    }
    // P7c T5 addition (live-data Refresh banner button).
    assert!(reg.contains("live.refresh"), "missing live.refresh");
    assert!(titles.contains(&"Refresh from source".to_string()));
    // P9a T7 addition (right-dock chart panel toggle).
    assert!(reg.contains("chart.visualize"), "missing chart.visualize");
    // P9c-1 T9 addition (left-dock AI panel toggle).
    assert!(reg.contains("ai.panel.open"), "missing ai.panel.open");
}

/// P9c-1 T9: the AI panel toggle action is registered in the built-in set.
#[test]
fn ai_panel_open_action_is_registered() {
    let reg = dat0_core::actions::test_registry();
    assert!(reg.contains(dat0_core::actions::builtin::ids::AI_PANEL_OPEN));
}

#[test]
fn edit_actions_are_registered() {
    let reg = dat0_core::actions::test_registry();
    for id in [
        "view.copy",
        "view.cut",
        "view.paste",
        "view.fill_down",
        "view.delete_rows",
        "view.set_null",
        "view.delete_column",
    ] {
        assert!(reg.contains(id), "missing {id}");
    }
}

// `keybinding_hints_parse_and_cover_exactly_the_bound_actions` lived here until
// SH4. `ActionDescriptor::keybinding` was a hand-typed copy of the `bind_keys`
// calls, so the test could only assert the copy agreed with itself. Its job —
// "every command either has a real chord or is declared chord-less on purpose"
// — is now `tests/keymap.rs::every_action_id_is_bound_or_explicitly_unbound`,
// which reads the table the bindings are actually installed from.
