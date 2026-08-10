//! The modal slot's keyboard contract: what a dialog can reach, and what it
//! cannot.
//!
//! Ported from `modal_b2_nav.rs`, whose twelve tests were built around GPUI's
//! focus machinery — `FocusHandle`, `focus_stop`, `tab_stop(false)`, and a
//! render-drain that existed because `view_actions::dispatch_export` reached
//! the shell from a bare `&mut App` with no `Window` to focus anything with.
//! None of that survives; the guarantees it protected all do.
//!
//! Most of the suite is already proven elsewhere and is not repeated here:
//! `views_b.rs` covers the export dialog's arrow keys, its scope/format
//! request and the saved-query picker's arrows, Enter and Delete;
//! `modals.rs` covers Escape closing the slot, Escape reporting exactly one
//! `Cancelled`, and Tab never reaching the shell. What is left — and what
//! this file is — is the part of the trap that lives in *markup and
//! selectors* rather than in a Rust list of stops:
//!
//! * a dialog holds the keyboard from the instant it opens;
//! * a roving-tabindex group is one stop, not one per child;
//! * the export modal's ring is its own six controls;
//! * a dismissed modal hands the keyboard back where it found it.
//!
//! # The one thing a headless harness cannot do
//!
//! Containment (`inert` on the background) and the wrap itself (`CYCLE_JS`)
//! are the browser's, and there is no browser here. So the ring is asserted
//! two ways that together bracket it: against the *selector constant* the
//! browser will actually evaluate, and against the *markup* it will evaluate
//! it over. A test that walked a Rust reimplementation alone could agree with
//! itself while the shipped selector disagreed with both.

mod support;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use dioxus::prelude::*;

use dat0_core::session::queries::SavedQuery;
use dat0_core::telemetry::crash::StagedCrash;

use dat0_ui::components::modals::{
    CAPTURE_JS, CLOSE_ID, DIALOG_ID, FOCUSABLE_SELECTOR, ModalHost, ModalOutcome, ModalReply,
    RELEASE_JS, title,
};
use dat0_ui::components::update_ui::UpdateState;
use dat0_ui::components::workspace_in_use::InUse;
use dat0_ui::state::{Modal, Workspace};
use support::Harness;
use support::dom::{Dom, NodeKey};

// ── mounting ─────────────────────────────────────────────────────────────────

thread_local! {
    /// What the slot holds at mount.
    static INITIAL: RefCell<Option<Modal>> = const { RefCell::new(None) };
    /// Everything the opener was told, so a safety assertion can be about the
    /// absence of a message and not merely the absence of a crash.
    static REPLIES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn reply() -> ModalReply {
    ModalReply::new(|o: ModalOutcome| REPLIES.with(|r| r.borrow_mut().push(format!("{o:?}"))))
}

fn replies() -> Vec<String> {
    REPLIES.with(|r| r.borrow().clone())
}

/// The host: one background stop the trap must never let focus reach, and the
/// slot.
#[component]
fn Host() -> Element {
    let ws = Workspace::provide();
    use_hook(move || {
        let mut ws = ws;
        ws.modal.set(INITIAL.with(|c| c.borrow_mut().take()));
    });

    rsx! {
        div { "data-a11y-id": "shell",
            button { "data-a11y-id": "background", tabindex: "0", "aria-label": "background", "bg" }
            ModalHost {}
        }
    }
}

fn mount(modal: Modal) -> Harness {
    INITIAL.with(|c| *c.borrow_mut() = Some(modal));
    REPLIES.with(|r| r.borrow_mut().clear());
    Harness::new(Host, ())
}

// ── the tab ring, computed from the mirror ───────────────────────────────────

/// Whether the browser would put this node in the sequential tab order.
///
/// The rules are [`FOCUSABLE_SELECTOR`]'s, restated over the mirror because
/// the mirror is not a browser and cannot run a selector. They are kept
/// honest by [`no_branch_of_the_focus_ring_selector_admits_a_negative_tabindex`],
/// which asserts the same `-1` exclusion against the shipped constant — so
/// this function drifting from it is a test failure rather than a silent
/// disagreement.
fn is_stop(dom: &Dom, key: NodeKey) -> bool {
    let node = dom.get(key);
    // `tabindex="-1"` is focusable by script and never by Tab, whatever the
    // tag. This is the clause the roving groups depend on.
    if node.attr("tabindex") == Some("-1") {
        return false;
    }
    if node.attr("tabindex").is_some() {
        return true;
    }
    // Dioxus renders a false boolean attribute as `"false"` rather than
    // dropping it, so presence is not disabledness.
    let disabled = node.attr("disabled") == Some("true");
    match node.tag() {
        Some("button" | "input" | "select" | "textarea") => !disabled,
        Some("a") => node.attr("href").is_some(),
        _ => false,
    }
}

fn descendants(dom: &Dom, root: NodeKey) -> Vec<NodeKey> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(k) = stack.pop() {
        let node = dom.get(k);
        if node.removed {
            continue;
        }
        out.push(k);
        // Reversed so the pop order is document order.
        for c in node.children.iter().rev() {
            stack.push(*c);
        }
    }
    out
}

/// The dialog's tab ring, by `data-a11y-id`, in document order.
fn ring(h: &Harness) -> Vec<String> {
    let dialog = h.by_a11y_id(DIALOG_ID).expect("a dialog is mounted");
    let dom = h.dom();
    descendants(dom, dialog)
        .into_iter()
        // The dialog itself is `tabindex="-1"`: a container, not a stop.
        .filter(|k| *k != dialog && is_stop(dom, *k))
        .map(|k| {
            dom.get(k)
                .attr("data-a11y-id")
                .unwrap_or("<unnamed>")
                .to_string()
        })
        .collect()
}

// ── variant builders ─────────────────────────────────────────────────────────

fn export() -> Modal {
    Modal::Export {
        // With a destination the Export button is enabled, so the ring is the
        // dialog's full complement rather than one short.
        destination: Some(PathBuf::from("/tmp/out")),
        reply: reply(),
    }
}

fn picker() -> Modal {
    Modal::SavedQueries {
        queries: (0..3)
            .map(|i| SavedQuery {
                id: uuid::Uuid::now_v7(),
                name: format!("q{i}"),
                sql: format!("select {i}"),
                saved_at: i as i64,
            })
            .collect(),
        reply: reply(),
    }
}

// ── the dialog takes the keyboard immediately ────────────────────────────────

#[test]
fn a_dialog_holds_the_keyboard_the_moment_it_opens() {
    // GATE A, restated. GPUI's export dialog opened from a path that held no
    // `Window`, so it could not focus anything; a flag was set and
    // `WorkspaceShell::render` — which does hold one — drained it into a
    // `focus()` on the modal's first stop. Miss that drain and NOTHING was
    // focused, the dispatch path was the window root alone, and Tab was inert.
    //
    // There is no drain because there is nothing to drain into: the dialog
    // element is itself focusable and carries the trap's own handler, so the
    // keys work before the user has reached a control. Both halves are
    // asserted, because either alone is satisfiable by an accident.
    let h = mount(export());
    let dialog = h.by_a11y_id(DIALOG_ID).expect("a dialog is mounted");

    assert_eq!(
        h.attr(dialog, "tabindex").as_deref(),
        Some("-1"),
        "the dialog must be focusable so it can hold the keyboard, and out of \
         the tab order so it is not itself a stop"
    );
    assert!(
        h.has_listener(dialog, "keydown"),
        "the trap has to be on the dialog, not on a control inside it"
    );
}

// ── roving tabindex ──────────────────────────────────────────────────────────

#[test]
fn no_branch_of_the_focus_ring_selector_admits_a_negative_tabindex() {
    // GATE B, at the source. A selector *list* is a union, so a branch that
    // forgets the exclusion re-admits every roving child through its tag:
    // `button:not([disabled])` matches `<button tabindex="-1">` perfectly
    // well. That is not hypothetical — it is what the constant said until
    // this test was written, and it made a three-format choice cost three
    // Tabs.
    let branches: Vec<&str> = FOCUSABLE_SELECTOR.split(',').map(str::trim).collect();
    assert!(branches.len() >= 5, "{FOCUSABLE_SELECTOR}");
    for branch in branches {
        assert!(
            branch.contains(r#":not([tabindex="-1"])"#),
            "branch {branch:?} would put a roving child back in the tab order"
        );
    }
}

#[test]
fn a_radio_group_is_one_tab_stop_not_one_per_radio() {
    // GATE B, over the markup. GPUI spelled this `.tab_stop(false)` on each
    // child of a `RadioGroup` wrapped in one dat0 `focus_stop`; here it is
    // the WAI-ARIA roving pattern — the group is `tabindex="0"`, every radio
    // is `tabindex="-1"`, and Left/Right move the selection instead.
    let h = mount(export());

    for group in ["export-format-group", "export-scope-group"] {
        let node = h.by_a11y_id(group).expect(group);
        assert_eq!(h.attr(node, "tabindex").as_deref(), Some("0"), "{group}");
        assert_eq!(
            h.attr(node, "role").as_deref(),
            Some("radiogroup"),
            "{group}"
        );
    }
    for radio in [
        "export-format-csv",
        "export-format-json",
        "export-format-parquet",
        "export-scope-current",
        "export-scope-full",
    ] {
        let node = h.by_a11y_id(radio).expect(radio);
        assert_eq!(
            h.attr(node, "tabindex").as_deref(),
            Some("-1"),
            "{radio} is a stop of its own, so choosing a format costs a Tab per option"
        );
    }

    let ring = ring(&h);
    assert!(
        !ring
            .iter()
            .any(|id| id.starts_with("export-format-c") || id.starts_with("export-scope-c")),
        "a radio reached the tab ring: {ring:?}"
    );
}

#[test]
fn a_list_of_rows_is_one_tab_stop_not_one_per_row() {
    // Same rule, the surface that scales: the saved-query picker is a
    // listbox whose rows are `role="option"` and arrow-driven. GPUI proved
    // this as "never per-row focus handles"; a picker over fifty saved
    // queries with a stop each is unusable by keyboard.
    let h = mount(picker());

    let list = h.by_a11y_id("sql-saved-list").expect("the list container");
    assert_eq!(h.attr(list, "tabindex").as_deref(), Some("0"));
    assert_eq!(h.attr(list, "role").as_deref(), Some("listbox"));

    let ring = ring(&h);
    assert!(
        !ring.iter().any(|id| id.starts_with("saved-row-")),
        "a row reached the tab ring: {ring:?}"
    );
    assert!(
        ring.contains(&"sql-saved-list".to_string()),
        "the container itself must be reachable: {ring:?}"
    );
}

// ── the ring's exact membership ──────────────────────────────────────────────

#[test]
fn the_export_modals_tab_ring_is_its_own_controls_in_order() {
    // GPUI's `export_modal_tab_cycles_four_stops` walked four handles —
    // format, scope, Export, Cancel — because the GPUI dialog had no file
    // name of its own: `run_export` built `export.{ext}` and the native save
    // panel owned both the directory and the name. This dialog owns the name
    // and asks the caller only for a directory, so the ring gained Browse and
    // the name field. Six stops, same guarantee: they are the dialog's own
    // controls, in the order they are read, and nothing else.
    let h = mount(export());

    assert_eq!(
        ring(&h),
        vec![
            "export-format-group",
            "export-scope-group",
            "export-browse",
            "export-name",
            "export-run",
            "export-cancel",
        ]
    );
}

#[test]
fn a_disabled_control_is_not_a_stop() {
    // The export button is disabled until a destination exists. A ring that
    // included it would strand a keyboard user on a control that does
    // nothing — and `:not([disabled])` is the clause that prevents it, so it
    // is asserted rather than assumed.
    let h = mount(Modal::Export {
        destination: None,
        reply: reply(),
    });
    let run = h.by_a11y_id("export-run").expect("the export button");
    assert_eq!(h.attr(run, "disabled").as_deref(), Some("true"));

    let ring = ring(&h);
    assert!(
        !ring.contains(&"export-run".to_string()),
        "a disabled control reached the tab ring: {ring:?}"
    );
    assert!(ring.contains(&"export-cancel".to_string()), "{ring:?}");
}

#[test]
fn the_close_affordance_joins_the_ring_only_when_it_exists() {
    // The ✕ is the host's, not the panel's, and it is rendered only for a
    // dialog whose scrim dismisses. It must be a stop when it is there —
    // upstream `Dialog::close_button` was a real button — and must not leave
    // a hole in the ring when it is not.
    let dismissable = mount(picker());
    assert!(ring(&dismissable).contains(&CLOSE_ID.to_string()));

    let gated = mount(export());
    assert!(!ring(&gated).contains(&CLOSE_ID.to_string()));
}

// ── handing the keyboard back ────────────────────────────────────────────────

#[test]
fn a_dismissed_modal_hands_the_keyboard_back_to_where_it_came_from() {
    // `picker_escape_restores_focus` asserted this against a real
    // `FocusHandle`. The mechanism is now the browser's, so what can be
    // checked without one is the contract between the two halves: capture
    // must record a return target, record it only once (a variant swap
    // re-runs capture with focus already inside the dialog, and recording
    // *that* would hand focus to a node about to be unmounted), and release
    // must both restore and clear it. A capture that recorded twice, or a
    // release that only cleared, would leave a keyboard user at the top of
    // the document on every Escape.
    const SLOT: &str = "__d0ModalReturn";

    assert!(CAPTURE_JS.contains(SLOT), "{CAPTURE_JS}");
    assert!(
        CAPTURE_JS.contains(&format!("{SLOT} == null")),
        "capture must record the return target only when it is unset: {CAPTURE_JS}"
    );
    assert!(
        CAPTURE_JS.contains("document.activeElement"),
        "{CAPTURE_JS}"
    );

    assert!(
        RELEASE_JS.contains(&format!("{SLOT}.focus")),
        "release must actually restore focus: {RELEASE_JS}"
    );
    assert!(
        RELEASE_JS.contains(&format!("{SLOT} = null")),
        "release must clear the target, or the next modal restores the wrong node: {RELEASE_JS}"
    );
    assert!(
        RELEASE_JS.contains("inert"),
        "release must also give the background back: {RELEASE_JS}"
    );
}

#[test]
fn escaping_the_picker_dismisses_it_and_tells_the_opener() {
    // The other half of `picker_escape_restores_focus`, which the harness can
    // see: the slot empties and the opener learns the user backed out, rather
    // than being left waiting for a query that never arrives.
    let mut h = mount(picker());
    assert_eq!(h.by_role("dialog").len(), 1);

    h.key_at(DIALOG_ID, support::Key::Escape, support::Modifiers::empty());

    assert_eq!(h.by_role("dialog").len(), 0);
    assert_eq!(replies(), vec!["Cancelled".to_string()]);
}

// ── every panel is a dialog, and says which one ──────────────────────────────

#[test]
fn every_variant_paints_one_dialog_node_named_by_its_title() {
    // `export_modal_emits_a_named_dialog_node` proved `modal_host` was really
    // in the tree for one panel. The invariant behind it is the one
    // `modal_count_tracks_the_mounted_set` was reaching for: GPUI kept
    // `open_modal_count` and the trapped set as two hand-maintained lists, so
    // a modal could be styled and silently left untrapped. Here there is one
    // host and one slot, so the check that matters is that no variant escapes
    // the host — every one of them paints exactly one `role="dialog"`, named
    // by `title()`, carrying the trap's handler.
    //
    // Three variants are absent because their payloads are `Signal`s or a
    // live controller, which cannot be built outside a running scope:
    // `Connections`, `ImportWizard`, `Ai`. They reach the same `body()` match
    // and the same host.
    let variants = vec![
        Modal::About {
            newer: None,
            // The real check is a blocking `ureq` GET on `spawn_blocking`,
            // which panics outright with no Tokio runtime under it.
            check_latest: false,
        },
        Modal::Onboarding,
        export(),
        picker(),
        Modal::CrashReport {
            staged: Some(StagedCrash {
                message: "boom".into(),
                backtrace: "frame 0".into(),
                version: "0.0.0".into(),
            }),
            data_dir: PathBuf::from("/nonexistent"),
        },
        Modal::NamePrompt {
            title: "Name this view".into(),
            initial: String::new(),
            placeholder: None,
            confirm_label: None,
            secret: false,
            reply: reply(),
        },
        Modal::LiveRefresh {
            dropped_edits: 1,
            dropped_deletes: 0,
            reply: reply(),
        },
        Modal::WorkspaceInUse {
            kind: InUse::SameMachine,
            reply: reply(),
        },
        Modal::QueryLibrary {
            entries: Vec::new(),
            reply: reply(),
        },
        Modal::Recovery {
            scratch_root: PathBuf::from("/nonexistent"),
            recent_roots: Vec::new(),
            reply: reply(),
        },
        Modal::Update {
            state: UpdateState::Available {
                version: "9.9.9".into(),
                artifact: dat0_core::update::manifest::ArtifactEntry {
                    url: "https://example.invalid/dat0.tar.gz".into(),
                    sha256: "a".repeat(64),
                    size: 1,
                },
            },
            is_manual: true,
            reply: reply(),
        },
    ];

    for modal in variants {
        let expected = title(&modal);
        let debug = format!("{modal:?}");
        let h = mount(modal);

        let dialogs = h.by_role("dialog");
        assert_eq!(dialogs.len(), 1, "{debug} did not paint one dialog");
        let dialog = dialogs[0];
        assert_eq!(
            h.attr(dialog, "aria-label").as_deref(),
            Some(&*expected),
            "{debug} is not announced by its title"
        );
        assert_eq!(
            h.attr(dialog, "aria-modal").as_deref(),
            Some("true"),
            "{debug}"
        );
        assert!(
            h.has_listener(dialog, "keydown"),
            "{debug} was mounted outside the trap"
        );
    }
}

// ── the reply is a pointer, not a value ──────────────────────────────────────

#[test]
fn two_openers_replies_are_never_confused_for_one_another() {
    // `ModalReply`'s `PartialEq` is `Rc::ptr_eq`, which is what lets the slot
    // decide "this is a different modal" without demanding a closure be
    // comparable. Two replies with identical behaviour must still be
    // distinct, or re-setting the slot with a new opener would be diffed away
    // as a no-op and the wrong caller would be told the outcome.
    let a = ModalReply::new(|_| {});
    let b = ModalReply::new(|_| {});
    assert_ne!(a, b);
    assert_eq!(a, a.clone());

    // And it is a real dispatch, not a stored value.
    let seen = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let sink = seen.clone();
    ModalReply::new(move |o| sink.borrow_mut().push(format!("{o:?}")))
        .call(ModalOutcome::Confirmed);
    assert_eq!(*seen.borrow(), vec!["Confirmed".to_string()]);
}
