//! The AI panel's key and model entry (PD-023, step 5.6a).
//!
//! The panel's Save key and Save model wrote their request to its outbox
//! (`AiState::entry`) for the host to open a prompt, and nothing read the
//! outbox: a key could not be typed, so AI could never become ready. [`use_entry`]
//! reads it, opens the prompt, hands the typed value back to the panel and
//! puts the panel back in the modal slot.
//!
//! [`use_deps`] builds what the panel needs once per window, where the shell
//! built it on every render: a keychain opened per frame, and a warning per
//! frame when it would not open. A test provides its own through context, so
//! it never touches the OS keychain or the network.

use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::ai::key_store::{KeyStore, KeychainKeyStore, MemoryKeyStore};
use dat0_core::error_ux::Banner;
use dat0_core::settings::store::SettingsStore;
use dat0_i18n::t;

use crate::components::ai::{AiController, AiDeps, AiEntry, AiPanelEvent, LiveProbe};
use crate::components::modals::{ModalOutcome, ModalReply};
use crate::state::{Modal, Workspace};

/// What the AI panel needs, and whether a key typed into it lasts only as long
/// as the process: the keychain would not open, so keys are held in memory.
#[derive(Clone)]
pub struct Deps {
    pub ai: AiDeps,
    pub volatile: bool,
}

/// This window's AI dependencies: the ones a test provided through context,
/// else the settings file, the OS keychain and the live transport.
pub fn use_deps() -> Deps {
    use_hook(|| {
        if let Some(deps) = try_consume_context::<Deps>() {
            return deps;
        }
        let store = Arc::new(SettingsStore::with_path(
            dat0_core::platform::config_dir()
                .unwrap_or_default()
                .join("settings.toml"),
        ));
        // A keychain that will not open is not a reason to refuse the window:
        // keys are held in memory instead, and the user is told when one is
        // saved that it will not outlive dat0.
        let (keys, volatile): (Arc<dyn KeyStore>, bool) = match KeychainKeyStore::new() {
            Ok(k) => (Arc::new(k), false),
            Err(e) => {
                tracing::warn!("keychain unavailable: {e:#}");
                (Arc::new(MemoryKeyStore::default()), true)
            }
        };
        Deps {
            ai: AiDeps {
                store,
                keys,
                probe: Arc::new(LiveProbe),
            },
            volatile,
        }
    })
}

/// Open the prompt the AI panel asks for, and give the panel the answer.
pub fn use_entry(ws: Workspace, ai: AiController, volatile: bool) {
    use_effect(move || {
        let Some(entry) = *ai.state.entry.read() else {
            return;
        };
        ai.take_entry();
        let (title, secret) = match entry {
            AiEntry::Key => (t("ai.key.prompt"), true),
            AiEntry::Model => (t("ai.model.prompt"), false),
        };
        let answer = ai.clone();
        let mut modal = ws.modal;
        modal.set(Some(Modal::NamePrompt {
            title,
            initial: String::new(),
            placeholder: None,
            confirm_label: Some(t("prompt.save")),
            secret,
            reply: ModalReply::new(move |outcome| {
                // Never logged: a key's value passes through here.
                if let ModalOutcome::Named(value) = outcome {
                    let value = value.trim().to_string();
                    if !value.is_empty() {
                        answer.handle(match entry {
                            AiEntry::Key => AiPanelEvent::SetKey(value),
                            AiEntry::Model => AiPanelEvent::SetModel(value),
                        });
                        if entry == AiEntry::Key && volatile && answer.state.draft.peek().key_set {
                            ws.push_banner(Banner::warning(t("ai.key.volatile")));
                        }
                    }
                }
                back_to_panel(ws, answer.clone());
            }),
        }));
    });
}

/// Put the AI panel back in the slot the prompt took. Deferred: the prompt
/// clears the slot after its reply returns, which would clear the panel too.
fn back_to_panel(ws: Workspace, controller: AiController) {
    let mut modal = ws.modal;
    spawn(async move {
        if modal.peek().is_none() {
            modal.set(Some(Modal::Ai { controller }));
        }
    });
}
