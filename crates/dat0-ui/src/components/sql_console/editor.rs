//! The CodeMirror-backed SQL editor.
//!
//! The editor lives inside the webview; Rust drives it over one long-lived
//! `document::eval` channel per window. The protocol and the bundle are
//! `crates/dat0-ui/vendor/codemirror/` — see its README.
//!
//! # One channel per window, bound once
//!
//! `document::eval` hands a script a **scoped** `dioxus` object that is not
//! published on `window`, so a bundle loaded through `<script src>` has no
//! route back to Rust. The fix, proven by the Phase-0.2 spike, is an explicit
//! handoff: the boot script's first act is `dat0cm.bind(dioxus)`, and every
//! push message afterwards — `ready`, `change`, `cursor`, `run` — travels on
//! that one channel. It must stay alive for the window's lifetime; an eval per
//! message would rebind and drop everything queued.
//!
//! # Why the theme is pushed rather than inherited
//!
//! CodeMirror builds its styles in JS through `EditorView.theme`, outside the
//! CSS cascade, so it cannot read `var(--d0-…)`. Rather than let it fall back
//! to a stock theme and drift the moment a token changes, Rust sends the
//! resolved values from `ThemeTokens::editor_vars`.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use dat0_core::theme::ThemeTokens;

/// A message from the editor.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum EditorMsg {
    /// The instance mounted and is accepting input.
    Ready { id: String },
    /// The document changed.
    Change { id: String, doc: String },
    /// The caret moved. 1-based, as CodeMirror reports it.
    Cursor { id: String, line: usize, col: usize },
    /// Mod-Enter, bound inside CodeMirror's own keymap.
    Run { id: String, doc: String },
}

/// A message to the editor.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum EditorCmd {
    Init {
        id: String,
        /// The element to mount into.
        mount: String,
        doc: String,
        /// `table -> columns`, for schema-aware completion.
        schema: std::collections::BTreeMap<String, Vec<String>>,
        /// DuckDB's function catalogue.
        functions: Vec<String>,
        vars: std::collections::BTreeMap<String, String>,
    },
    SetDoc {
        id: String,
        doc: String,
    },
    Focus {
        id: String,
    },
    Theme {
        id: String,
        vars: std::collections::BTreeMap<String, String>,
    },
}

/// The palette the bundle expects, keyed as `buildTheme` reads it.
pub fn theme_vars(t: &ThemeTokens) -> std::collections::BTreeMap<String, String> {
    t.editor_vars()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The boot script: load the bundle, hand it the channel, announce readiness.
///
/// It returns immediately afterwards, and the channel keeps working. Both
/// halves of that are measured, by `examples/eval_probe.rs`:
///
/// * **`document::eval` scripts run concurrently.** A long-running script does
///   not block others, so commands can be their own short evals.
/// * **A returned script's channel survives.** The bundle holds the `dioxus`
///   object it was handed, and `change` / `cursor` / `run` keep arriving on
///   this `Eval` long after the boot script finished. A relay loop that never
///   returns is therefore unnecessary.
pub const BOOT: &str = r#"
const load = (src) => new Promise((ok, err) => {
  if (window.dat0cm) return ok();
  const s = document.createElement("script");
  s.src = src;
  s.onload = ok;
  s.onerror = () => err(new Error("failed to load " + src));
  document.head.appendChild(s);
});

// Deliberately not `requestAnimationFrame`: an occluded or unfocused window is
// not compositing and rAF stops firing, which would wedge the mount.
const settle = () => new Promise((r) => setTimeout(r, 4));

await load("/dat0/codemirror.js");
for (let i = 0; i < 500 && !window.dat0cm; i++) await settle();
if (!window.dat0cm) throw new Error("codemirror bundle did not define dat0cm");

// The handoff. Without it the bundle can receive but never reply.
window.dat0cm.bind(dioxus);
dioxus.send({ t: "ready", id: "" });
// One tick before returning, so the ping is read rather than racing the
// script's completion. The channel itself outlives this script.
await new Promise((r) => setTimeout(r, 0));
"#;

/// Apply one command, as its own eval.
///
/// `init` waits briefly for its mount point: a tab's `div` may not exist yet on
/// the tick the command is sent. Waiting here is safe because evals run
/// concurrently — this one does not hold anything else up.
const APPLY: &str = r#"
const settle = () => new Promise((r) => setTimeout(r, 4));
const cmd = CMD;
if (!window.dat0cm) throw new Error("dat0cm is not loaded");
if (cmd.t === "init") {
  for (let i = 0; i < 25; i++) {
    if (document.getElementById(cmd.mount)) break;
    await settle();
  }
}
window.dat0cm.handle(cmd);
"#;

/// The window's editor bridge.
#[derive(Clone, Copy)]
pub struct Editor {
    /// True once the bundle has loaded and bound the channel.
    ///
    /// Not "the eval was created": the boot script is queued, so the handle
    /// exists a good deal earlier than `window.dat0cm` does. Commanding on the
    /// former sends `init` into a window with no bundle, where it throws and is
    /// lost — the editor then mounts nothing, forever, with no error anywhere.
    /// The gate is the bundle's own ready ping.
    ready: Signal<bool>,
}

impl Editor {
    /// Open the channel and boot the bundle. Call once per window.
    ///
    /// The returned handle is `Copy` and can be stored in context; the incoming
    /// stream is drained by `on_msg`.
    pub fn use_channel(on_msg: impl FnMut(EditorMsg) + 'static) -> Self {
        let mut ready = use_signal(|| false);
        // A `Callback`, not the closure itself: `use_effect` re-runs its body,
        // so it needs something `Copy`, and the handler must outlive each run.
        let on_msg = use_callback(on_msg);

        use_effect(move || {
            // `use_effect`, not `use_future`: the desktop document provider
            // only exists after mount, and an eval created during the first
            // render pass is queued against nothing and never runs — silently.
            spawn(async move {
                let mut eval = document::eval(BOOT);
                loop {
                    match eval.recv::<EditorMsg>().await {
                        Ok(msg) => {
                            if matches!(&msg, EditorMsg::Ready { id } if id.is_empty()) {
                                ready.set(true);
                            }
                            on_msg.call(msg);
                        }
                        Err(e) => {
                            tracing::debug!("editor channel closed: {e}");
                            break;
                        }
                    }
                }
            });
        });

        Self { ready }
    }

    /// Whether the bundle is loaded and ready for commands.
    ///
    /// **Reads the signal**, so an effect that calls this re-runs when the
    /// bundle arrives. That is load-bearing: the first `init` is always
    /// attempted too early, and an effect that did not depend on this would
    /// drop it and never try again.
    pub fn is_open(&self) -> bool {
        *self.ready.read()
    }

    /// Send a command. A no-op before the bundle has loaded.
    pub fn send(&self, cmd: EditorCmd) {
        if !*self.ready.peek() {
            tracing::debug!("editor command dropped: bundle not loaded yet");
            return;
        }
        let json = match serde_json::to_string(&cmd) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!("editor command would not serialize: {e}");
                return;
            }
        };
        // Interpolated rather than sent over the boot channel: that channel is
        // the push direction, and a command is a one-shot.
        document::eval(&APPLY.replace("CMD", &json));
    }
}

/// The mount point for one editor instance.
///
/// An empty div: CodeMirror owns everything inside it, and Dioxus must not
/// diff that subtree. Keeping it childless is what guarantees that.
#[component]
pub fn EditorMount(id: String) -> Element {
    rsx! {
        div {
            class: "d0-editor",
            id: "cm-{id}",
            "data-a11y-id": "sql-editor-{id}",
            role: "textbox",
            "aria-label": dat0_i18n::t("sql.editor"),
            "aria-multiline": "true",
            tabindex: "0",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_map_uses_the_keys_the_bundle_reads() {
        // `buildTheme` looks these up by name; a rename on either side silently
        // falls back to the stock CodeMirror colours.
        let t = dat0_core::theme::builtin("light").unwrap();
        let v = theme_vars(&t);
        for key in [
            "mode",
            "surface",
            "paneHead",
            "fg",
            "ink",
            "muted",
            "accent",
            "activeBg",
            "rowHover",
            "ruleDim",
            "shadowOverlay",
            "sqlKeyword",
            "sqlNumber",
            "sqlString",
            "sqlFn",
            "sqlComment",
        ] {
            assert!(v.contains_key(key), "missing editor var {key}");
        }
    }

    #[test]
    fn the_theme_map_carries_the_resolved_palette_not_var_references() {
        // JS cannot resolve `var(--d0-…)`; sending one would paint literally
        // nothing.
        let t = dat0_core::theme::builtin("dark").unwrap();
        let v = theme_vars(&t);
        assert_eq!(v.get("sqlKeyword").map(String::as_str), Some("#bc8cff"));
        assert_eq!(v.get("mode").map(String::as_str), Some("dark"));
        assert!(v.values().all(|s| !s.contains("var(")));
    }

    #[test]
    fn commands_serialize_with_the_tag_the_bundle_switches_on() {
        let cmd = EditorCmd::SetDoc {
            id: "console-0".into(),
            doc: "SELECT 1".into(),
        };
        let j = serde_json::to_value(&cmd).unwrap();
        assert_eq!(j["t"], "set_doc");
        assert_eq!(j["doc"], "SELECT 1");
    }

    #[test]
    fn messages_deserialize_from_what_the_bundle_pushes() {
        let run: EditorMsg =
            serde_json::from_str(r#"{"t":"run","id":"console-0","doc":"SELECT 1"}"#).unwrap();
        assert!(matches!(run, EditorMsg::Run { .. }));

        let cur: EditorMsg =
            serde_json::from_str(r#"{"t":"cursor","id":"c","line":3,"col":15}"#).unwrap();
        match cur {
            EditorMsg::Cursor { line, col, .. } => {
                assert_eq!((line, col), (3, 15));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn the_boot_script_binds_before_it_announces_readiness() {
        // `ready` is the gate every command waits on, so it must not be sent
        // until the bundle can actually reply — otherwise the first `init`
        // races the bind, throws in the webview, and the editor mounts nothing
        // for the rest of the window's life.
        let load = BOOT.find("load(\"/dat0/codemirror.js\")").expect("loads");
        let bind = BOOT.find("dat0cm.bind(dioxus)").expect("binds the channel");
        let ready = BOOT.find(r#"t: "ready""#).expect("announces readiness");
        assert!(load < bind, "the bundle must load before it is bound");
        assert!(bind < ready, "the bind must precede the ready ping");
    }

    #[test]
    fn the_boot_script_does_not_depend_on_compositing() {
        // rAF fires once in an unfocused window; a mount that waits on it
        // hangs. Matched as a call so the comment naming the hazard does not
        // trip the check.
        assert!(
            !BOOT.contains("requestAnimationFrame("),
            "the boot script must not wait on an animation frame"
        );
    }
}
