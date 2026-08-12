//! Phase 4 acceptance — the SQL console, in a real window.
//!
//! The plan marks this flow "manual, because CodeMirror lives in the webview
//! and the headless harness cannot run it". It does not have to be: the bundle
//! exposes test seams, so the whole thing can be driven and asserted in Rust.
//!
//! What it proves, against the **real** `SqlConsole` component over a **real**
//! DuckDB catalogue:
//!
//! 1. the editor mounts and CodeMirror is themed from dat0's tokens;
//! 2. typing `SELECT * FROM ` offers the table name that Rust pushed;
//! 3. a DuckDB function from the catalogue completes;
//! 4. `Mod-Enter` inside CodeMirror surfaces as a `run` intent carrying the
//!    document — the path the ⌘⏎ chord actually takes;
//! 5. **the editor is not a keyboard trap.** `indentWithTab` makes Tab indent
//!    rather than move focus — the same trap gpui-component's `Input` had — so
//!    Escape has to be the way out, and it has to land on the Run control. This
//!    is `view/sql_console.rs`'s Escape ladder, last rung;
//! 6. **switching tabs re-inits the editor** with the other tab's document. An
//!    effect only re-runs when a signal it read changed, and props are not
//!    signals, so this is one `use_reactive` away from silently showing the
//!    wrong query under the right title;
//! 7. **the transient bars manage focus.** A streaming answer takes the
//!    keyboard when it appears, re-homes it across the Stop→Insert swap, and
//!    hands it back to the editor when it closes; a failed-run strip does none
//!    of that, because a failed Run must not yank the caret out of the
//!    statement you are fixing.
//!
//! 5–7 are here rather than in `tests/sql_console_transient_nav.rs` for one
//! reason: they are about **focus and the editor's own keymap**, and the
//! headless harness has neither a focus ring nor a webview. It asserts which
//! control is *marked* to take focus (`autofocus`); this asserts that the
//! browser then does it.
//!
//! ```text
//! cd crates/dat0-ui && cargo run --example console_probe
//! ```
//!
//! Exits 0 on PASS, 1 on FAIL.

use std::sync::Arc;

use dioxus::prelude::*;
use parking_lot::Mutex;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_core::query::completion::{SchemaSnapshot, TableEntry, new_shared_snapshot};
use dat0_ui::components::ai::{StreamKind, StreamPhase, StreamView};
use dat0_ui::components::sql_console::{ConsoleIntent, SqlConsole, Tab};
use dat0_ui::launch::Boot;
use dat0_ui::theme::{Theme, ThemeStyle};

const TABLE: &str = "nyc_taxi_trips";
/// The second tab's document. Nothing else in the window contains it, so
/// finding it in the editor proves the switch re-initialised the view.
const SECOND_DOC: &str = "SELECT 42 AS answer";

/// One short, **non-awaiting** step of the probe.
///
/// `document::eval` executes scripts on a single queue, and a script that
/// awaits holds it: a wait loop inside one eval deadlocks against the editor's
/// own boot script, which then never loads the bundle the wait is waiting for.
/// So every step returns immediately and Rust does the polling between them.
const STEP: &str = r#"
const step = STEP_NAME;
const ID = "console-0";
const cm = window.dat0cm;

// What has the keyboard: dat0's own handle when the element carries one, and
// the class list otherwise — CodeMirror's content element is a bare
// contenteditable div with no `data-a11y-id`.
function focused() {
  const a = document.activeElement;
  if (!a) return "";
  return (a.getAttribute("data-a11y-id") || "") + " " + (a.className || "");
}

function run() {
  switch (step) {
    case "loaded":
      return { ok: cm != null && cm.doc(ID) !== null };
    case "type_table":
      cm.type(ID, "SELECT * FROM ");
      cm.complete(ID);
      return { ok: true };
    case "tables":
      return { ok: cm.completions(ID) !== null, list: cm.completions(ID) || [] };
    case "type_fn":
      cm.type(ID, "\nSELECT date_tr");
      cm.complete(ID);
      return { ok: true };
    case "fns":
      return { ok: cm.completions(ID) !== null, list: cm.completions(ID) || [] };
    case "run":
      return { ok: cm.key(ID, "Enter", { meta: true }) };
    case "final": {
      const content = document.querySelector(".cm-content");
      const gutter = document.querySelector(".cm-gutters");
      const cs = content ? getComputedStyle(content) : null;
      return {
        ok: true,
        doc: cm.doc(ID),
        font: cs ? cs.fontFamily : "",
        font_size: cs ? cs.fontSize : "",
        gutter_bg: gutter ? getComputedStyle(gutter).backgroundColor : "",
      };
    }
    // Tab inside the editor: `cm.key` reports whether CodeMirror cancelled the
    // event, i.e. whether it indented instead of letting focus move on.
    case "tab_key":
      cm.type(ID, "");
      return { ok: true, handled: cm.key(ID, "Tab", {}) };
    // ...and Escape, which must therefore be the way out. CodeMirror's own
    // Escape bindings (close the completion popup, close the search panel)
    // come first by design, so press until focus leaves the editor and report
    // how many it took — one press when nothing is open, two when the
    // completion popup from the steps above still is.
    case "escape_key": {
      cm.type(ID, "");
      let presses = 0;
      let handled = false;
      for (let i = 0; i < 3; i++) {
        handled = cm.key(ID, "Escape", {});
        presses++;
        if (!focused().includes("cm-content")) break;
      }
      return { ok: true, handled, presses, focused: focused() };
    }
    case "second_doc": {
      const doc = cm.doc("console-1");
      return { ok: doc !== null && doc.length > 0, doc: doc || "" };
    }
    case "focused":
      return { ok: true, focused: focused() };
    default:
      return { ok: false };
  }
}

dioxus.send(run());
// Hold the script open for a tick after the send.
//
// `DesktopEvaluator` is owned by its query slot, and the slot is dropped the
// moment the script finishes — so a script that sends and immediately returns
// races the reader and usually loses, surfacing as
// `EvalError::Finished: eval has already ran`. A zero-delay timer defers
// completion past the send without holding the queue meaningfully.
await new Promise((r) => setTimeout(r, 0));
"#;

#[derive(serde::Deserialize, Debug, Default)]
struct Step {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    list: Vec<String>,
    #[serde(default)]
    doc: String,
    #[serde(default)]
    font: String,
    #[serde(default)]
    font_size: String,
    #[serde(default)]
    gutter_bg: String,
    /// Did CodeMirror's own keymap consume the keystroke?
    #[serde(default)]
    handled: bool,
    /// `data-a11y-id` and class of `document.activeElement`.
    #[serde(default)]
    focused: String,
    /// How many Escapes it took to leave the editor.
    #[serde(default)]
    presses: u32,
}

/// Run one step. Each is its own eval, so the queue is never held.
async fn step(name: &str) -> Step {
    let script = STEP.replace("STEP_NAME", &format!("{name:?}"));
    let mut eval = document::eval(&script);
    match eval.recv::<Step>().await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("probe step {name} failed: {e}");
            std::process::exit(2);
        }
    }
}

/// Poll a step until it reports success.
async fn until(name: &str, what: &str) -> Step {
    for _ in 0..400 {
        let s = step(name).await;
        if s.ok {
            return s;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    println!("--- dat0 SQL console probe ---");
    println!("FAIL: timed out waiting for {what}");
    std::process::exit(1);
}

/// Poll until the keyboard lands on something matching `want`. Returns what it
/// last saw, so a failure reports where focus actually went.
async fn until_focus(want: &str) -> String {
    let mut last = String::new();
    for _ in 0..160 {
        last = step("focused").await.focused;
        if last.contains(want) {
            return last;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    last
}

/// Poll until a step's `doc` is non-empty, or give up and report what it was.
async fn until_doc(name: &str) -> String {
    for _ in 0..160 {
        let s = step(name).await;
        if s.ok {
            return s.doc;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    step(name).await.doc
}

#[derive(Debug, Default)]
struct Report {
    tables: Vec<String>,
    fns: Vec<String>,
    doc: String,
    run_handled: bool,
    font: String,
    font_size: String,
    gutter_bg: String,
    /// Tab is consumed by the editor — the reason an exit rung is needed.
    tab_handled: bool,
    /// Where Escape from inside the editor left the keyboard.
    escape_focus: String,
    /// How many Escapes it took to leave the editor.
    escape_presses: u32,
    /// The second tab's document, after switching to it.
    second_doc: String,
    /// Focus while the answer is still arriving.
    streaming_focus: String,
    /// Focus after the Stop -> Insert swap.
    drafted_focus: String,
    /// Focus after the bar closed.
    returned_focus: String,
    /// Focus after a failed run raised its strip.
    error_focus: String,
}

fn main() {
    let (events, rx) = AppEvents::channel();
    let registry = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&registry).expect("built-ins register");
    let boot = Boot {
        events,
        rx: Arc::new(Mutex::new(Some(rx))),
        registry,
        cli_paths: std::sync::Arc::new(parking_lot::Mutex::new(Vec::new())),
    };

    dioxus::LaunchBuilder::desktop()
        .with_cfg(dat0_ui::launch::config())
        .with_context(boot)
        .launch(Probe);
}

#[component]
fn Probe() -> Element {
    // The real `components::App` does this; a probe that mounts a component
    // directly has to as well, or every /dat0/* request 404s and the bundle
    // never loads.
    dioxus::desktop::use_asset_handler("dat0", dat0_ui::protocol::serve);
    Theme::provide(None);

    // The schema Rust pushes into the editor. Nothing in CodeMirror could
    // invent this name, so finding it in the popup proves the handoff.
    let schema = use_hook(|| {
        let s = new_shared_snapshot();
        *s.lock() = SchemaSnapshot {
            tables: vec![TableEntry {
                name: TABLE.to_string(),
                columns: vec!["vendor_id".into(), "pickup_at".into(), "fare_amount".into()],
            }],
            functions: dat0_core::query::completion::DUCKDB_FUNCTIONS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };
        s
    });

    let tabs = use_hook(|| {
        vec![
            Tab {
                id: "console-0".into(),
                title: "query".into(),
                doc: "-- dat0 SQL console\n".into(),
            },
            Tab {
                id: "console-1".into(),
                title: "scratch".into(),
                doc: SECOND_DOC.into(),
            },
        ]
    });

    let mut active = use_signal(|| 0usize);
    let mut stream = use_signal(StreamView::default);
    let mut error = use_signal(|| Option::<String>::None);
    let mut ran = use_signal(Vec::<String>::new);

    use_effect(move || {
        spawn(async move {
            until("loaded", "the editor to mount").await;

            step("type_table").await;
            let tables = until("tables", "the table completion popup").await;

            step("type_fn").await;
            let fns = until("fns", "the function completion popup").await;

            let run = step("run").await;
            let last = step("final").await;

            // Let the run intent travel editor -> Rust -> signal.
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;

            // 5. The keyboard trap, and the way out of it.
            let tab = step("tab_key").await;
            let esc = step("escape_key").await;

            // 6. Switching tabs re-initialises the editor.
            active.set(1);
            let second_doc = until_doc("second_doc").await;

            // 7. Transient-bar focus.
            stream.set(StreamView {
                kind: Some(StreamKind::NlToSql),
                prompt: "top users".into(),
                text: "SELECT".into(),
                phase: StreamPhase::Streaming,
                error: None,
            });
            let streaming_focus = until_focus("console-stream-stop").await;

            stream.set(StreamView {
                kind: Some(StreamKind::NlToSql),
                prompt: "top users".into(),
                text: "SELECT user FROM t".into(),
                phase: StreamPhase::Done,
                error: None,
            });
            let drafted_focus = until_focus("console-stream-insert").await;

            stream.set(StreamView::default());
            let returned_focus = until_focus("cm-content").await;

            // A failed run raises its strip without taking the keyboard.
            error.set(Some("Parser Error: near FRM".into()));
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let error_focus = step("focused").await.focused;

            report(
                Report {
                    tables: tables.list,
                    fns: fns.list,
                    doc: last.doc,
                    run_handled: run.ok,
                    font: last.font,
                    font_size: last.font_size,
                    gutter_bg: last.gutter_bg,
                    tab_handled: tab.handled,
                    escape_focus: esc.focused,
                    escape_presses: esc.presses,
                    second_doc,
                    streaming_focus,
                    drafted_focus,
                    returned_focus,
                    error_focus,
                },
                ran.read().clone(),
            );
        });
    });

    rsx! {
        ThemeStyle {}
        div { style: "position:absolute; inset:0; padding: 40px; background: var(--d0-canvas)",
            SqlConsole {
                tabs: tabs.clone(),
                active: active(),
                schema,
                stream: stream(),
                error: error(),
                on_intent: move |i: ConsoleIntent| {
                    if let ConsoleIntent::Run { tab, sql, .. } = i {
                        ran.write().push(format!("{tab}:{sql}"));
                    }
                },
                on_select_tab: move |i: usize| active.set(i),
            }
        }
    }
}

fn report(r: Report, runs: Vec<String>) {
    println!("--- dat0 SQL console probe ---");

    let tokens = dat0_core::theme::builtin_or_default(dat0_core::theme::DEFAULT_ID);
    let mut fails = Vec::new();
    let mut check = |name: &str, ok: bool, detail: String| {
        println!(
            "  {:<34} {:<8} {}",
            name,
            if ok { "ok" } else { "FAIL" },
            detail
        );
        if !ok {
            fails.push(name.to_string());
        }
    };

    check(
        "Rust-supplied table completes",
        r.tables.iter().any(|t| t == TABLE),
        format!("{} offered", r.tables.len()),
    );
    check(
        "DuckDB function completes",
        r.fns.iter().any(|f| f == "date_trunc"),
        format!("{:?}", r.fns.iter().take(3).collect::<Vec<_>>()),
    );
    check(
        "document round-trips to Rust",
        r.doc.contains("SELECT * FROM "),
        format!("{:?}", r.doc),
    );
    check(
        "Mod-Enter reaches CodeMirror's keymap",
        r.run_handled,
        String::new(),
    );
    check(
        "the run intent arrived with its SQL",
        runs.iter().any(|s| s.starts_with("console-0:")),
        format!("{runs:?}"),
    );
    check(
        "editor uses Geist Mono at 12.5px",
        r.font.contains("Geist Mono") && r.font_size == "12.5px",
        format!("{} / {}", r.font, r.font_size),
    );
    check(
        "gutter is themed from dat0's tokens",
        r.gutter_bg == rgb(&tokens.pane_head),
        format!("{} (want {})", r.gutter_bg, rgb(&tokens.pane_head)),
    );
    check(
        "Tab indents rather than escaping",
        r.tab_handled,
        "the trap the rung below exists for".to_string(),
    );
    check(
        "Escape leaves the editor for Run",
        r.escape_focus.contains("console-run") && r.escape_presses <= 2,
        format!("{:?} after {} press(es)", r.escape_focus, r.escape_presses),
    );
    check(
        "switching tabs re-inits the editor",
        r.second_doc.contains("SELECT 42"),
        format!("{:?}", r.second_doc),
    );
    check(
        "a streaming answer takes the keyboard",
        r.streaming_focus.contains("console-stream-stop"),
        format!("{:?}", r.streaming_focus),
    );
    check(
        "focus re-homes across Stop -> Insert",
        r.drafted_focus.contains("console-stream-insert"),
        format!("{:?}", r.drafted_focus),
    );
    check(
        "closing the bar returns the caret",
        r.returned_focus.contains("cm-content"),
        format!("{:?}", r.returned_focus),
    );
    check(
        "a failed run does not steal the caret",
        r.error_focus.contains("cm-content"),
        format!("{:?}", r.error_focus),
    );

    if fails.is_empty() {
        println!("PASS");
        std::process::exit(0);
    }
    println!("FAIL: {fails:?}");
    std::process::exit(1);
}

/// `#rrggbb` as the `rgb(r, g, b)` a computed style reports.
fn rgb(hex: &str) -> String {
    let h = hex.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    format!("rgb({}, {}, {})", p(0), p(2), p(4))
}
