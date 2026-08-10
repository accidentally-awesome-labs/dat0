//! Smoke test: `view.undo` + `view.redo` are registered in `ActionRegistry`
//! after `register_all` runs (P4a T7).

use dat0_core::actions::builtin::ids;
use dat0_core::actions::{ActionId, ActionRegistry};

#[test]
fn view_undo_and_redo_are_registered() {
    let reg = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&reg).expect("register_all must not fail");

    assert!(
        reg.get(&ActionId::from(ids::VIEW_UNDO)).is_some(),
        "view.undo must be registered"
    );
    assert!(
        reg.get(&ActionId::from(ids::VIEW_REDO)).is_some(),
        "view.redo must be registered"
    );
}

#[test]
fn view_undo_and_redo_titles_are_correct() {
    let reg = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&reg).expect("register_all must not fail");

    let undo = reg
        .get(&ActionId::from(ids::VIEW_UNDO))
        .expect("view.undo must be present");
    assert_eq!(undo.title, "Undo");

    let redo = reg
        .get(&ActionId::from(ids::VIEW_REDO))
        .expect("view.redo must be present");
    assert_eq!(redo.title, "Redo");
}

#[test]
fn view_undo_and_redo_are_in_edit_group() {
    use dat0_core::actions::ActionGroup;

    let reg = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&reg).expect("register_all must not fail");

    let undo = reg
        .get(&ActionId::from(ids::VIEW_UNDO))
        .expect("view.undo must be present");
    assert_eq!(undo.group, ActionGroup::Edit);

    let redo = reg
        .get(&ActionId::from(ids::VIEW_REDO))
        .expect("view.redo must be present");
    assert_eq!(redo.group, ActionGroup::Edit);
}
