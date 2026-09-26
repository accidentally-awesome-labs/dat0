//! Opening a workspace folder (PD-023, step 5.4b).
//!
//! A workspace is a folder holding a `.dat0/` directory, where its database,
//! session and manifest live instead of in a scratch directory. Open
//! Workspace, File → Open Recent, the demo and the recovery panel's Resume all
//! end here. They used to post the folder as a file to open, which refused it
//! as an unsupported file type.
//!
//! The checks run in the window that asked, so what they say shows there:
//!
//! 1. The folder must hold a `.dat0/` that finished being made. One a Save
//!    Workspace left half-written is the recovery panel's to resume.
//! 2. A workspace a window of this process holds is brought forward rather
//!    than opened twice: its lock would refuse a second window anyway.
//! 3. A workspace on a sync drive carries a cross-machine lock record. Held by
//!    a live dat0 on another machine, the user is asked before opening it:
//!    two machines editing one workspace can corrupt it.
//!
//! Then a window of its own opens on it (`Opening::Workspace`), and brings
//! back the tabs, views and SQL the workspace was left with.

use std::path::PathBuf;

use dioxus::prelude::*;

use dat0_core::error_ux::Banner;
use dat0_core::events::{AppEvent, AppEvents, Opening};
use dat0_core::workspace::Home;
use dat0_core::workspace::lock_manifest::{self, AcquireOutcome};
use dat0_i18n::t;

use crate::components::modals::{ModalOutcome, ModalReply};
use crate::components::workspace_in_use::{InUse, ModalDecision, decide};
use crate::state::{Modal, Workspace};

/// Open the workspace in `folder`, on behalf of the window `ws`.
pub fn open(ws: Workspace, events: &AppEvents, folder: PathBuf) {
    let root = std::fs::canonicalize(&folder).unwrap_or(folder);
    let dat0 = Home::dat0_dir_for(&root);
    if !dat0.is_dir() {
        ws.push_banner(Banner::warning(t("workspace.open.not_a_workspace")));
        return;
    }
    if dat0_core::workspace::promote::detect_incomplete(&dat0) {
        ws.push_banner(Banner::warning_with_body(
            t("workspace.open.incomplete.title"),
            t("workspace.open.incomplete.body"),
        ));
        return;
    }
    let boot = try_consume_context::<crate::launch::Boot>();
    if let Some(boot) = &boot
        && let Some(window) = boot.windows.holding(&root)
    {
        boot.windows.send(Some(window), AppEvent::Raise);
        return;
    }

    let networked = dat0_core::workspace::networked::is_networked(
        &root,
        &dat0_core::workspace::networked::settings(),
    );
    let outcome = if networked {
        lock_manifest::acquire(
            &dat0.join("lock.json"),
            &dat0_core::workspace::identity::hostname(),
        )
        .unwrap_or_else(|e| {
            // An unreadable record decides nothing: opening claims it anew.
            tracing::warn!(error = %format!("{e:#}"), "lock.json unreadable");
            AcquireOutcome::Available
        })
    } else {
        AcquireOutcome::Available
    };
    // The new window belongs to no workbench window, so it is asked for on
    // the process bus, and still opens if this one closes first.
    let bus = boot.map(|b| b.events).unwrap_or_else(|| events.clone());
    let opening = Opening::Workspace { root, networked };
    match decide(&outcome, false) {
        ModalDecision::Proceed => bus.send(AppEvent::OpenWindow(opening)),
        ModalDecision::ConflictForeign(holder) => {
            let mut modal = ws.modal;
            modal.set(Some(Modal::WorkspaceInUse {
                kind: InUse::conflict(holder),
                reply: ModalReply::new(move |outcome| {
                    if matches!(outcome, ModalOutcome::Confirmed) {
                        bus.send(AppEvent::OpenWindow(opening.clone()));
                    }
                }),
            }));
        }
        // A live dat0 on this machine holds it, yet no window of this one
        // does: another instance, which single-instance should prevent. Its
        // local lock would refuse this window, so say why up front.
        ModalDecision::BlockSameMachine | ModalDecision::SameMachineInUse => {
            ws.push_banner(Banner::warning(t("workspace.in_use.same_machine.title")))
        }
    }
}

/// [`open`], from where no bus is to hand: a folder dropped on the window,
/// or named on the command line.
pub fn open_here(ws: Workspace, folder: PathBuf) {
    match try_consume_context::<AppEvents>() {
        Some(events) => open(ws, &events, folder),
        None => tracing::warn!(folder = %folder.display(), "open workspace: no event bus"),
    }
}

/// File → Open Recent: the `ix`-th recent workspace, as the menu listed it.
pub fn open_recent(ws: Workspace, events: &AppEvents, ix: &str) {
    if let Some(root) = ix.parse().ok().and_then(crate::menu::listed_recent) {
        open(ws, events, root);
    }
}

/// The recovery panel's Resume: finish the Save Workspace that was cut short,
/// then open the workspace.
pub fn resume(ws: Workspace, events: &AppEvents, root: PathBuf) {
    let dat0 = Home::dat0_dir_for(&root);
    let finished =
        dat0_core::workspace::promote::finish_incomplete(&dat0, dat0_core::time::now_epoch_secs());
    match finished {
        Ok(()) => open(ws, events, root),
        Err(e) => ws.push_banner(Banner::warning_with_body(
            t("workspace.open.failed.title"),
            format!("{e:#}"),
        )),
    }
}
