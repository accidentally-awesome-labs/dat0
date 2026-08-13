//! What the SQL timing chip says about where a query ran.
//!
//! Ported from `dat0-app/tests/motherduck_window.rs::routing_chip_shows_md_not_local`,
//! which mounted a whole `WorkspaceShell`, seeded a chip with
//! `(1234 ms, Routing::Md)` and asserted the rendered label contained
//! `"ms · md"` and not `"· local"`.
//!
//! The Dioxus console does not carry that chip (Phase 4 rebuilt the console
//! from the editor up; the timing surface moved to the status bar's row
//! counter). What that test actually protected is not the chip's markup: it is
//! that a query which touched MotherDuck is *labelled* as having touched
//! MotherDuck, and that the three routings are told apart in the copy a user
//! reads. Both halves live in `dat0_core::connections::routing`, which has no
//! UI in it at all — so the port lands here rather than in a headless
//! VirtualDom that would have nothing to mount.
//!
//! The classifier's own arms are unit-tested beside it. What is added here is
//! the half those unit tests cannot see: `i18n_key` is a *lookup*, and
//! `dat0_i18n::t` echoes a missing key straight back, so a renamed or deleted
//! string would paint the chip `"sql.md"` and every classifier test would
//! still pass.

use dat0_core::connections::routing::{Routing, classify_routing};

const ALL: [Routing; 3] = [Routing::Local, Routing::Md, Routing::Mixed];

#[test]
fn every_routing_has_copy_of_its_own_that_actually_resolves() {
    let mut seen: Vec<String> = Vec::new();
    for r in ALL {
        let key = r.i18n_key();
        let text = dat0_i18n::t(key);
        assert_ne!(
            text, key,
            "{r:?} paints its raw i18n key {key:?} — the string is missing"
        );
        assert!(!text.trim().is_empty(), "{r:?} paints nothing");
        assert!(
            !seen.contains(&text),
            "{r:?} shares its label {text:?} with another routing, so the chip \
             cannot distinguish them"
        );
        seen.push(text);
    }
}

#[test]
fn a_query_against_an_attached_motherduck_database_is_labelled_md_not_local() {
    // The original assertion, at the level that survived: the label a
    // MotherDuck-only query earns is the `md` one, and it is not the `local`
    // one.
    let attached = vec!["sample_data".to_string()];
    let routing = classify_routing("SELECT * FROM sample_data.main.events", &attached);
    assert_eq!(routing, Routing::Md);

    let label = dat0_i18n::t(routing.i18n_key());
    assert_eq!(label, dat0_i18n::t(Routing::Md.i18n_key()));
    assert_ne!(
        label,
        dat0_i18n::t(Routing::Local.i18n_key()),
        "teeth: the md label must not be the local one"
    );
}

#[test]
fn a_query_that_touches_both_sides_is_labelled_neither_purely() {
    // A join across an attachment and a local table is the case a user most
    // needs told, because it is the one that moves bytes without looking like
    // it does.
    let attached = vec!["sample_data".to_string()];
    let routing = classify_routing(
        "SELECT * FROM sample_data.main.events JOIN local_sales USING (id)",
        &attached,
    );
    assert_eq!(routing, Routing::Mixed);
    let label = dat0_i18n::t(routing.i18n_key());
    assert_ne!(label, dat0_i18n::t(Routing::Md.i18n_key()));
    assert_ne!(label, dat0_i18n::t(Routing::Local.i18n_key()));
}

#[test]
fn nothing_attached_means_nothing_can_be_labelled_md() {
    // The chip's claim is about bytes leaving the machine. With no attachment
    // there is nowhere for them to go, however the SQL is spelled.
    for sql in [
        "SELECT * FROM sample_data.main.events",
        "SELECT * FROM md.t",
        "SELECT * FROM t",
    ] {
        assert_eq!(
            classify_routing(sql, &[]),
            Routing::Local,
            "{sql} was routed off-machine with nothing attached"
        );
    }
}
