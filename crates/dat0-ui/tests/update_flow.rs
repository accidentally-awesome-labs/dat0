//! Checking for updates (step 5.7).
//!
//! Help → Check for Updates was built disabled and nothing checked at launch:
//! the check, the prompt and the installer existed with nothing calling them.
//! These tests run the flow in a real modal host with a fetch that answers
//! without the network: a check the user asked for shows every answer, one at
//! launch only a found update, and a dialog already up keeps its place.

mod support;

use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::update::AvailableUpdate;
use dat0_core::update::manifest::ArtifactEntry;
use dat0_i18n::t;
use dat0_ui::components::modals::{ModalHost, ModalOutcome, ModalReply};
use dat0_ui::state::{Modal, Workspace};
use dat0_ui::update_flow::{Found, check_with};
use support::Harness;

fn found() -> Found {
    Ok(Some(AvailableUpdate {
        version: "9.9.9".into(),
        artifact: ArtifactEntry {
            url: "https://example.invalid/dat0.tar.gz".into(),
            sha256: "0".repeat(64),
            size: 1,
        },
    }))
}

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    manual: bool,
    /// What the fetch answers: `none`, `found` or `failed`.
    answer: &'static str,
    /// Another dialog is up before the check runs.
    busy: bool,
}

#[component]
fn Host(props: HostProps) -> Element {
    let mut ws = Workspace::provide();
    let HostProps {
        manual,
        answer,
        busy,
    } = props;
    rsx! {
        button {
            "data-a11y-id": "check",
            onclick: move |_| {
                if busy {
                    ws.modal.set(Some(Modal::NamePrompt {
                        title: "Save query as…".to_string(),
                        initial: String::new(),
                        placeholder: None,
                        confirm_label: None,
                        secret: false,
                        reply: ModalReply::new(|_: ModalOutcome| {}),
                    }));
                }
                check_with(ws, manual, move || match answer {
                    "found" => found(),
                    "failed" => Err(anyhow::anyhow!("offline")),
                    _ => Ok(None),
                });
            },
        }
        div { "data-a11y-id": "banners",
            for b in ws.banners.read().iter() {
                span { "{b.title}" }
            }
        }
        ModalHost {}
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// Check, then let the fetch answer: the modal's text and the banners.
fn run(manual: bool, answer: &'static str, busy: bool) -> (String, String) {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = Harness::new(
        Host,
        HostProps {
            manual,
            answer,
            busy,
        },
    );
    h.settle();
    h.click("check");
    for _ in 0..40 {
        h.settle();
        std::thread::sleep(Duration::from_millis(25));
    }
    let text = |id: &str| h.by_a11y_id(id).map(|k| h.text_of(k)).unwrap_or_default();
    (text("modal"), text("banners"))
}

#[test]
#[serial]
fn a_check_the_user_asked_for_answers_whatever_it_found() {
    let (modal, _) = run(true, "none", false);
    assert!(modal.contains(&t("update.up_to_date")), "{modal}");

    let (modal, _) = run(true, "failed", false);
    assert!(modal.contains(&t("update.failed")), "{modal}");
    assert!(modal.contains("offline"), "and why: {modal}");

    let (modal, _) = run(true, "found", false);
    assert!(modal.contains("9.9.9"), "{modal}");
}

#[test]
#[serial]
fn the_check_at_launch_speaks_only_when_it_found_one() {
    let (modal, banners) = run(false, "none", false);
    assert!(
        modal.is_empty() && banners.is_empty(),
        "{modal} / {banners}"
    );
    let (modal, _) = run(false, "failed", false);
    assert!(modal.is_empty(), "{modal}");

    let (modal, _) = run(false, "found", false);
    assert!(modal.contains("9.9.9"), "{modal}");
}

#[test]
#[serial]
fn a_dialog_already_up_keeps_its_place() {
    let (modal, banners) = run(false, "found", true);
    assert!(modal.contains("Save query as"), "the prompt stays: {modal}");
    assert!(
        banners.contains("9.9.9"),
        "the answer waits in a banner: {banners}"
    );
}
