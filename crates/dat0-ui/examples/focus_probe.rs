//! Windowed probe: a surface that opens takes the keyboard every time.
//!
//! The command palette, the name prompt, the filter popover, the grid's cell
//! editor and its context menu each took the keyboard with the `autofocus`
//! attribute. A document honours that once, so only the first to open in a
//! window got focus. The palette's second opening left its input unfocused,
//! and what was typed went to the grid; a second cell edit took no typing. The
//! headless harness has no focus model, so this opens each surface twice in a
//! real window and asks the document where the keyboard is.
//!
//! ```text
//! cd crates/dat0-ui && cargo run --example focus_probe
//! ```
//!
//! Exits 0 when every check passes, 1 otherwise.

use dioxus::prelude::*;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_core::grid::selection::CellCoord;
use dat0_core::view::filter_popover::ColumnType;
use dat0_ui::components::command_palette::CommandPalette;
use dat0_ui::components::filter_popover::FilterPopover;
use dat0_ui::components::grid::cell_editor::CellEditor;
use dat0_ui::components::grid::context_menu::ContextMenu;
use dat0_ui::components::modals::{ModalHost, ModalOutcome, ModalReply};
use dat0_ui::state::{Modal, Workspace};

fn main() {
    dioxus::LaunchBuilder::desktop().launch(Probe);
}

fn builtins() -> ActionRegistry {
    let reg = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&reg).expect("builtins register");
    reg
}

const PROBE: &str = r#"
const settle = () => new Promise((r) => setTimeout(r, 4));
async function waitFor(pred, label, tries = 500) {
  for (let i = 0; i < tries; i++) {
    if (pred()) return true;
    await settle();
  }
  return false;
}
const q = (id) => document.querySelector('[data-a11y-id="' + id + '"]');
const escape = (el) =>
  el.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));

const guard = setTimeout(() => {
  dioxus.send({ error: "the probe did not finish within 60s" });
}, 60000);

// Open `opener`'s surface, report whether `target` took the keyboard, close it.
async function opens(opener, target, closer) {
  // Where a user's keyboard is before they open it: somewhere else.
  q("elsewhere").focus();
  q(opener).click();
  if (!(await waitFor(() => q(target), target + " to mount"))) return "never mounted";
  const focused = await waitFor(() => document.activeElement === q(target), target + " focus");
  escape(q(closer));
  if (!(await waitFor(() => !q(target), target + " to close"))) return "never closed";
  return focused;
}

try {
  await waitFor(() => q("elsewhere"), "the host to mount");
  const report = {};
  for (const [name, opener, target, closer] of [
    ["palette", "open-palette", "palette-query", "palette-query"],
    ["prompt", "open-prompt", "name-prompt-field", "name-prompt-field"],
    ["filter", "open-filter", "filter-popover", "filter-popover"],
    ["cell", "open-cell", "cell-editor", "cell-editor"],
    ["bool_cell", "open-bool-cell", "cell-editor", "cell-editor"],
    ["menu", "open-menu", "context-menu", "context-menu"],
  ]) {
    report[name] = [
      await opens(opener, target, closer),
      await opens(opener, target, closer),
    ];
  }
  clearTimeout(guard);
  dioxus.send(report);
} catch (e) {
  clearTimeout(guard);
  dioxus.send({ error: String(e) });
}
// Held open until Rust has read the result (see modal_trap_probe).
await dioxus.recv();
"#;

#[component]
fn Probe() -> Element {
    let mut ws = Workspace::provide();
    use_context_provider(builtins);
    use_context_provider(|| AppEvents::channel().0);
    let mut filter = use_signal(|| false);
    // The cell editor's column type while it is open.
    let mut cell = use_signal(|| Option::<ColumnType>::None);
    let mut menu = use_signal(|| false);
    let at = CellCoord { row: 0, col: 0 };

    use_effect(move || {
        spawn(async move {
            let mut ev = document::eval(PROBE);
            let got = ev.recv::<serde_json::Value>().await;
            let _ = ev.send(true);
            match got {
                Ok(v) => check(v),
                Err(e) => fail(&format!("the probe script never reported: {e}")),
            }
        });
    });

    rsx! {
        button { "data-a11y-id": "elsewhere", "elsewhere" }
        button {
            "data-a11y-id": "open-palette",
            onclick: move |_| ws.palette.set(true),
            "palette"
        }
        button {
            "data-a11y-id": "open-prompt",
            onclick: move |_| {
                ws.modal.set(Some(Modal::NamePrompt {
                    title: "Save query as…".to_string(),
                    initial: "q2".to_string(),
                    placeholder: None,
                    confirm_label: None,
                    secret: false,
                    reply: ModalReply::new(|_: ModalOutcome| {}),
                }))
            },
            "prompt"
        }
        button {
            "data-a11y-id": "open-filter",
            onclick: move |_| filter.set(true),
            "filter"
        }
        button {
            "data-a11y-id": "open-cell",
            onclick: move |_| cell.set(Some(ColumnType::String)),
            "cell"
        }
        button {
            "data-a11y-id": "open-bool-cell",
            onclick: move |_| cell.set(Some(ColumnType::Bool)),
            "bool cell"
        }
        button {
            "data-a11y-id": "open-menu",
            onclick: move |_| menu.set(true),
            "menu"
        }
        if let Some(column_type) = cell() {
            div { style: "position: relative; height: 60px;",
                CellEditor {
                    cell: at,
                    initial: "true".to_string(),
                    widths: vec![120.0],
                    column_type,
                    on_done: move |_| cell.set(None),
                }
            }
        }
        if menu() {
            ContextMenu {
                at: (60.0, 60.0),
                cell: at,
                has_selection: true,
                on_pick: move |_| menu.set(false),
                on_dismiss: move |_| menu.set(false),
            }
        }
        if filter() {
            FilterPopover {
                column: "qty".to_string(),
                column_type: ColumnType::Numeric,
                at: (40.0, 40.0),
                on_outcome: move |_| filter.set(false),
            }
        }
        CommandPalette {}
        ModalHost {}
    }
}

fn check(v: serde_json::Value) {
    if let Some(e) = v.get("error") {
        fail(&format!("probe error: {e}"));
    }
    let mut ok = true;
    for name in ["palette", "prompt", "filter", "cell", "bool_cell", "menu"] {
        let got = &v[name];
        let both = got
            .as_array()
            .is_some_and(|a| a.len() == 2 && a.iter().all(|x| x.as_bool() == Some(true)));
        println!("{name}: first, second opening took the keyboard: {got}");
        ok &= both;
    }
    if ok {
        println!("focus_probe: every surface took the keyboard each time it opened");
        std::process::exit(0);
    }
    fail("a surface opened without the keyboard");
}

fn fail(why: &str) -> ! {
    eprintln!("focus_probe FAILED: {why}");
    std::process::exit(1);
}
