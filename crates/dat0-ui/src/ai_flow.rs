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
//!
//! [`console`] is the console's half (step 5.6b). NL→SQL and Explain streamed
//! into a strip the console already drew, from a controller that could already
//! stream, and nothing started one: the console had no button for either, and
//! the shell handed it an empty stream. The answer is drawn from the schema
//! alone, names and types, never a row (`AiController::spawn_stream`).

use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::ai::key_store::{KeyStore, KeychainKeyStore, MemoryKeyStore};
use dat0_core::error_ux::Banner;
use dat0_core::settings::store::SettingsStore;
use dat0_i18n::t;

use crate::components::ai::{
    AiController, AiDeps, AiEntry, AiPanelEvent, LiveProbe, StreamKind, StreamPhase, StreamView,
};
use crate::components::modals::{ModalOutcome, ModalReply};
use crate::components::sql_console::ConsoleIntent;
use crate::components::sql_console::host::ConsoleHost;
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

/// Do the console's AI intents; hand any other back to the console's host.
pub fn console(
    ws: Workspace,
    ai: &AiController,
    host: &ConsoleHost,
    intent: ConsoleIntent,
) -> Option<ConsoleIntent> {
    match intent {
        ConsoleIntent::AskAi => ask(ws, ai.clone()),
        ConsoleIntent::Explain { sql } => {
            if !sql.trim().is_empty() {
                stream(ws, ai.clone(), StreamKind::Explain, sql);
            }
        }
        ConsoleIntent::StopStream => stop(ai),
        ConsoleIntent::InsertGenerated { sql } => {
            let mut tabs = host.tabs;
            tabs.write().open_with(unfence(&sql));
            clear(ai);
        }
        ConsoleIntent::DiscardStream | ConsoleIntent::CloseExplain => clear(ai),
        other => return Some(other),
    }
    None
}

/// Ask what the statement is for, then stream one.
fn ask(ws: Workspace, ai: AiController) {
    let mut modal = ws.modal;
    modal.set(Some(Modal::NamePrompt {
        title: t("sql.nl2sql.prompt_title"),
        initial: String::new(),
        placeholder: Some(t("sql.nl2sql.prompt")),
        confirm_label: Some(t("sql.nl2sql.chip")),
        secret: false,
        reply: ModalReply::new(move |outcome| {
            if let ModalOutcome::Named(question) = outcome
                && !question.trim().is_empty()
            {
                stream(ws, ai.clone(), StreamKind::NlToSql, question);
            }
        }),
    }));
}

/// Stream `kind` for `prompt` over the window's tables, into the strip.
fn stream(ws: Workspace, ai: AiController, kind: StreamKind, prompt: String) {
    let Some(engine) = ws.session.peek().ready().map(|s| s.lock().engine.clone()) else {
        return;
    };
    spawn(async move {
        use dat0_engine::QueryEngine as _;
        let tables = engine.get_tables().await.unwrap_or_default();
        if ai.spawn_stream(kind, prompt, &tables).is_none() {
            // The buttons are offered only while AI is ready, so this is a
            // key or a model gone since: say so rather than do nothing.
            ws.push_banner(Banner::warning(t("ai.not_ready")));
        }
    });
}

/// Stop the answer arriving, and keep what came: a partial statement can
/// still be taken or thrown away. Deltas still in flight quote the stream's
/// old id and are dropped.
fn stop(ai: &AiController) {
    let mut id = ai.state.stream_load_id;
    let next = id.peek().wrapping_add(1);
    id.set(next);
    let mut stream = ai.state.stream;
    stream.write().phase = StreamPhase::Done;
}

/// Close the strip.
fn clear(ai: &AiController) {
    let mut stream = ai.state.stream;
    stream.set(StreamView::default());
}

/// The statement in an answer: the first fenced block's body when there is
/// one, the whole answer otherwise, trimmed.
pub fn unfence(answer: &str) -> String {
    let Some(open) = answer.find("```") else {
        return answer.trim().to_string();
    };
    let body = &answer[open + 3..];
    // The fence's language tag runs to the end of its line.
    let body = body.split_once('\n').map_or("", |(_, rest)| rest);
    let body = body.find("```").map_or(body, |close| &body[..close]);
    body.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::unfence;

    #[test]
    fn a_fenced_answer_gives_its_block_and_a_bare_one_itself() {
        assert_eq!(unfence("  SELECT 1  "), "SELECT 1");
        assert_eq!(
            unfence("Here it is:\n```sql\nSELECT 42 AS answer\n```\nEnjoy."),
            "SELECT 42 AS answer"
        );
        assert_eq!(unfence("```\nSELECT 1\n```"), "SELECT 1");
        assert_eq!(unfence("```sql\nSELECT 2"), "SELECT 2", "a stopped answer");
    }
}
