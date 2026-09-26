//! Every item the menu bar can emit reaches something that happens.
//!
//! `src/menu.rs` has cited this file since the migration, and it did not
//! exist. In its absence seven items shipped dead: Open, Export, Unpack and
//! Replay Package, Toggle Sidebar, Toggle Inspector and Check for Updates each
//! logged "menu item has no handler yet" (PD-023), and Help linked to a domain
//! the project does not use.
//!
//! An item is either a registered action — then the router must be able to
//! perform it (`router::is_wired`) — or a menu-local id, which `menu_local` in
//! `components/mod.rs` must handle. Anything else is built disabled, through
//! `router::UNWIRED` or `menu::UNWIRED_LOCAL`. A `muda::Menu` cannot be built
//! off the platform main thread, so the checks read `menu::emitted_ids` and the
//! handler's source rather than a live bar, and a drift test keeps that list
//! honest against `build`.

use std::collections::{BTreeMap, BTreeSet};

use dat0_core::actions::registry::ActionRegistry;
use dat0_ui::menu::{self, UNWIRED_LOCAL};
use dat0_ui::router;

const MENU_SRC: &str = include_str!("../src/menu.rs");
const HANDLER_SRC: &str = include_str!("../src/components/mod.rs");

fn builtins() -> ActionRegistry {
    let reg = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&reg).expect("builtins register");
    reg
}

/// The body of the item starting at `header`, up to the first closing brace
/// in column 0 (or, for an indented module, in column 0 after it).
fn body<'a>(src: &'a str, header: &str) -> &'a str {
    let start = src
        .find(header)
        .unwrap_or_else(|| panic!("`{header}` not found"))
        + header.len();
    let rest = &src[start..];
    let end = rest.find("\n}").expect("closing brace");
    &rest[..end]
}

/// Every `<prefix>::NAME` path in `text`, by NAME.
fn paths(text: &str, prefix: &str) -> BTreeSet<String> {
    let needle = format!("{prefix}::");
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find(&needle) {
        // Only a whole path segment: `menu_ids::X` must not match `ids::X`.
        let before = rest[..at].chars().next_back();
        rest = &rest[at + needle.len()..];
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.insert(name);
        }
    }
    out
}

/// `menu_ids` constants, NAME → value, read from the module's own source.
fn local_consts() -> BTreeMap<String, String> {
    let module = body(MENU_SRC, "pub mod menu_ids {");
    let mut out = BTreeMap::new();
    for line in module.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let name = rest.split(':').next().unwrap().trim().to_string();
        let value = rest
            .split('"')
            .nth(1)
            .expect("a string literal")
            .to_string();
        out.insert(name, value);
    }
    assert!(out.len() >= 5, "read only {} menu_ids constants", out.len());
    out
}

/// The ids `menu_local` handles: exact ids, and prefixes (`RECENT_PREFIX`).
fn handled_locally(id: &str) -> bool {
    let consts = local_consts();
    paths(body(HANDLER_SRC, "fn menu_local("), "menu_ids")
        .iter()
        .map(|name| {
            consts.get(name).unwrap_or_else(|| {
                panic!("menu_local names menu_ids::{name}, which is not declared")
            })
        })
        .any(|v| id == v || (v.ends_with('.') && id.starts_with(v.as_str())))
}

#[test]
fn every_emitted_id_is_an_action_or_a_menu_local_id() {
    let reg = builtins();
    let local: BTreeSet<String> = menu::local_ids().into_iter().collect();
    for id in menu::emitted_ids() {
        assert!(
            reg.contains(&id) || local.contains(&id),
            "the menu bar can emit {id}, which is neither a registered action nor a menu_ids constant"
        );
    }
}

#[test]
fn every_enabled_item_does_something() {
    let reg = builtins();
    for id in menu::emitted_ids() {
        if !menu::enabled(&id) {
            continue;
        }
        if reg.contains(&id) {
            assert!(
                router::is_wired(&id),
                "{id} is enabled in the menu bar and does nothing"
            );
        } else {
            assert!(
                handled_locally(&id),
                "{id} is enabled in the menu bar and menu_local has no arm for it"
            );
        }
    }
}

#[test]
fn a_local_id_without_a_handler_is_built_disabled() {
    for id in menu::emitted_ids() {
        if menu::local_ids().contains(&id) && !handled_locally(&id) {
            assert!(
                !menu::enabled(&id),
                "{id} has no handler; list it in menu::UNWIRED_LOCAL"
            );
        }
    }
}

#[test]
fn unwired_local_ids_are_real_and_really_unhandled() {
    // A stale entry would disable a working item, or disable nothing.
    let local = menu::local_ids();
    for id in UNWIRED_LOCAL {
        assert!(
            local.iter().any(|l| l.as_str() == *id),
            "{id} is not a menu_ids constant"
        );
        assert!(
            !handled_locally(id),
            "{id} is handled now; take it out of UNWIRED_LOCAL"
        );
    }
}

#[test]
fn emitted_ids_lists_every_id_build_uses() {
    // `emitted_ids` is the list the tests above trust, kept by hand beside
    // `build`. Compare the constants each one names, so an item added to the
    // bar and not to the list fails here rather than escaping every check.
    let built = body(MENU_SRC, "pub fn build() -> Menu {");
    let listed = body(MENU_SRC, "pub fn emitted_ids() -> Vec<String> {");
    for prefix in ["ids", "menu_ids"] {
        let mut in_build = paths(built, prefix);
        let mut in_list = paths(listed, prefix);
        // Recents are appended by index, from one constant, outside `build`.
        in_build.remove("RECENT_PREFIX");
        in_list.remove("RECENT_PREFIX");
        assert_eq!(
            in_build, in_list,
            "`build` and `emitted_ids` disagree about the {prefix}:: items"
        );
    }
}

#[test]
fn the_scrapers_see_what_they_should() {
    // If the parsing above stopped matching the source, every test in this
    // file would pass on empty sets.
    assert!(handled_locally(menu::menu_ids::DOCS));
    assert!(handled_locally("recents.open.3"));
    assert!(handled_locally(menu::menu_ids::OPEN_PACKAGE));
    assert!(!handled_locally(menu::menu_ids::CHECK_UPDATES));
    assert!(paths(body(MENU_SRC, "pub fn build() -> Menu {"), "ids").len() > 10);
}
