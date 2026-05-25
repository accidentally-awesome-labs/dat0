//! `ActionRegistry` + built-in action descriptors (P3b T3).
//!
//! Verifies the registry shape and the seven baseline `register_all`
//! built-ins. Banner action_ids (T2) reference these stable strings.
//! Downstream tasks (T5 recovery panel, T7 empty-state hero,
//! T10 import cancel, T11 file dialog, T12 theme toggle) will replace
//! stub dispatch bodies with real wiring — registry shape itself is
//! frozen here.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dat0_app::actions::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry};

#[test]
fn register_then_iter() {
    let reg = ActionRegistry::new();
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    reg.register(ActionDescriptor {
        id: ActionId::from("test.noop"),
        title: "Test Noop".into(),
        group: ActionGroup::Navigation,
        keybinding: None,
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
        keybinding: None,
        dispatch: Arc::new(|_| {}),
    };
    reg.register(desc()).unwrap();
    let err = reg.register(desc()).unwrap_err();
    assert!(matches!(
        err,
        dat0_app::actions::registry::RegisterError::DuplicateId(_)
    ));
}

#[test]
fn lookup_returns_descriptor() {
    let reg = ActionRegistry::new();
    reg.register(ActionDescriptor {
        id: ActionId::from("file.open"),
        title: "Open File".into(),
        group: ActionGroup::File,
        keybinding: None,
        dispatch: Arc::new(|_| {}),
    })
    .unwrap();
    let d = reg.get(&ActionId::from("file.open")).unwrap();
    assert_eq!(d.title, "Open File");
}

#[test]
fn builtins_register_seven() {
    let reg = ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&reg).unwrap();
    assert_eq!(reg.count(), 7);
    let titles: Vec<String> = reg.iter().map(|d| d.title).collect();
    assert!(titles.contains(&"New Window".to_string()));
    assert!(titles.contains(&"Open Settings".to_string()));
}
