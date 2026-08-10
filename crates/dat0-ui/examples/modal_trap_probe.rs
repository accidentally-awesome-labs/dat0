//! Windowed modal focus-trap probe.
//!
//! Three of the GPUI trap's guarantees are the browser's own focus model, and a
//! `WriteMutations` mirror has no focus model at all — there is no
//! `document.activeElement`, `inert` does nothing, and `HTMLElement.focus()`
//! does not exist. `tests/modal_trap_nav.rs` therefore proves everything
//! *upstream* of focus (which stops the ring is offered, and that Tab and
//! Escape are taken by the trap rather than by the shell); this proves the
//! three that need a real document:
//!
//! 1. **Containment** — `CAPTURE_JS` marks every sibling of the scrim `inert`
//!    and `aria-hidden`, so the background is out of the tab order *and* out of
//!    the accessibility tree. GPUI's `modal_host` could not do this: its
//!    `occlude` blocked the mouse only, so a screen reader still walked the
//!    shell behind the dialog.
//! 2. **Wrap-around, both ways** — `tab_wraps_from_last_stop_to_first` and
//!    `shift_tab_wraps_from_first_stop_to_last`, plus the `at < 0` arm that
//!    pulls focus back in when it has escaped
//!    (`tab_snaps_focus_back_into_the_modal`). All three are `CYCLE_JS`'s index
//!    arithmetic, reproducing `overlay.rs::next_index`.
//! 3. **Restoration** — `dismiss_restores_focus_to_the_pre_open_stop`.
//!    `RELEASE_JS` hands the keyboard back to whatever opened the dialog;
//!    without it, Escape drops a keyboard user at the top of the document.
//!
//! The scripts are not retyped here: they are the exported constants, put
//! through the same substitution `modals::cycle_focus` performs, so a change to
//! either is a change to what this probe runs.
//!
//! ```text
//! cd crates/dat0-ui && cargo run --example modal_trap_probe
//! ```
//!
//! Exits 0 when every check passes, 1 otherwise.

use dioxus::prelude::*;

use dat0_ui::components::modals::{
    CYCLE_JS, FOCUSABLE_SELECTOR, ModalHost, ModalOutcome, ModalReply,
};
use dat0_ui::state::{Modal, Workspace};

fn main() {
    dioxus::LaunchBuilder::desktop().launch(Probe);
}

/// The prompt: three stops, an inert scrim and no ✕, so the keyboard is its
/// only exit — which is what makes the ring load-bearing rather than a
/// convenience.
fn name_prompt() -> Modal {
    Modal::NamePrompt {
        title: "Save query as…".to_string(),
        initial: "q2".to_string(),
        placeholder: None,
        confirm_label: None,
        secret: false,
        reply: ModalReply::new(|_: ModalOutcome| {}),
    }
}

/// The probe body, with `CYCLE_FWD` / `CYCLE_BACK` spliced in as the real
/// constant.
const PROBE: &str = r#"
const settle = () => new Promise((r) => setTimeout(r, 4));
async function waitFor(pred, label, tries = 500) {
  for (let i = 0; i < tries; i++) {
    if (pred()) return;
    await settle();
  }
  throw new Error("timed out waiting for " + label);
}
const q = (id) => document.querySelector('[data-a11y-id="' + id + '"]');
const cycleForward = () => { __CYCLE_FWD__ };
const cycleBack = () => { __CYCLE_BACK__ };
const id = (n) => (n ? n.getAttribute("data-a11y-id") : null);

const guard = setTimeout(() => {
  dioxus.send({ error: "the probe did not finish within 30s" });
}, 30000);

try {
  await waitFor(() => q("open-modal"), "the host to mount");

  // Open it the way a user does, from a control that then owns the focus the
  // dismissal has to hand back.
  const opener = q("open-modal");
  opener.focus();
  opener.click();
  await waitFor(() => q("modal"), "the dialog to mount");
  // The host's `use_effect` runs CAPTURE_JS after the render commits.
  await waitFor(() => q("shell-behind").inert, "the background to be inerted");

  // 1. Containment.
  const behind = q("shell-behind");
  const contained =
    behind.inert === true && behind.getAttribute("aria-hidden") === "true";

  // 2. The ring.
  const dialog = q("modal");
  const stops = [...dialog.querySelectorAll(__SELECTOR__)];
  const ring = stops.map(id);

  // Forward off the last stop wraps to the first.
  stops[stops.length - 1].focus();
  cycleForward();
  const wrapped = id(document.activeElement);

  // Backwards off the first wraps to the last.
  stops[0].focus();
  cycleBack();
  const wrappedBack = id(document.activeElement);

  // Focus that has escaped — the `indexOf` -1 arm — is pulled back to the
  // first stop rather than left to wander. Staged with a direct blur, which
  // models the real hazard: async code moving focus while a dialog is up.
  document.activeElement.blur();
  cycleForward();
  const snappedBack = id(document.activeElement);

  // 3. Restoration. A real keystroke, on a real stop, through the dialog's own
  // handler — not a Rust-side call.
  stops[stops.length - 1].focus();
  stops[stops.length - 1].dispatchEvent(
    new KeyboardEvent("keydown", { key: "Escape", bubbles: true })
  );
  await waitFor(() => !q("modal"), "the dialog to dismiss");
  await waitFor(() => !q("shell-behind").inert, "the background to be released");

  clearTimeout(guard);
  dioxus.send({
    contained,
    released:
      document.querySelectorAll("[inert]").length === 0 &&
      q("shell-behind").getAttribute("aria-hidden") === null,
    ring,
    wrapped,
    wrappedBack,
    snappedBack,
    restored: id(document.activeElement),
  });
} catch (e) {
  clearTimeout(guard);
  dioxus.send({ error: String(e) });
}
await new Promise((r) => setTimeout(r, 0));
"#;

fn script() -> String {
    let selector = format!("{FOCUSABLE_SELECTOR:?}");
    let subst = |delta: &str| {
        CYCLE_JS
            .replace("SELECTOR", &selector)
            .replace("DELTA", delta)
    };
    PROBE
        .replace("__CYCLE_FWD__", &subst("1"))
        .replace("__CYCLE_BACK__", &subst("-1"))
        .replace("__SELECTOR__", &selector)
}

/// What the script reported, read out of the JSON by hand.
///
/// The keys are camelCase on the JS side and snake_case here; mapping them in
/// one place is shorter than a `serde` rename per field.
#[derive(Debug)]
struct Report {
    error: Option<String>,
    contained: bool,
    released: bool,
    ring: Vec<String>,
    wrapped: Option<String>,
    wrapped_back: Option<String>,
    snapped_back: Option<String>,
    restored: Option<String>,
}

#[component]
fn Probe() -> Element {
    let mut ws = Workspace::provide();

    use_effect(move || {
        spawn(async move {
            let mut ev = document::eval(&script());
            match ev.recv::<serde_json::Value>().await {
                Ok(v) => check(v),
                Err(e) => fail(&format!("the probe script never reported: {e}")),
            }
        });
    });

    rsx! {
        div {
            // Everything beside the scrim is what CAPTURE_JS inerts, so the
            // background has to be a *sibling* of the modal host's output.
            div { "data-a11y-id": "shell-behind",
                button {
                    "data-a11y-id": "open-modal",
                    onclick: move |_| ws.modal.set(Some(name_prompt())),
                    "open"
                }
                button { "data-a11y-id": "background", "background" }
            }
            ModalHost {}
        }
    }
}

fn check(v: serde_json::Value) {
    // The JS object uses camelCase; map it once rather than annotating.
    let r = Report {
        error: v.get("error").and_then(|x| x.as_str()).map(str::to_string),
        contained: v
            .get("contained")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        released: v.get("released").and_then(|x| x.as_bool()).unwrap_or(false),
        ring: v
            .get("ring")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .map(|s| s.as_str().unwrap_or("<unnamed>").to_string())
                    .collect()
            })
            .unwrap_or_default(),
        wrapped: v
            .get("wrapped")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        wrapped_back: v
            .get("wrappedBack")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        snapped_back: v
            .get("snappedBack")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        restored: v
            .get("restored")
            .and_then(|x| x.as_str())
            .map(str::to_string),
    };

    println!("--- dat0 modal trap probe ---");
    if let Some(e) = r.error {
        fail(&e);
    }
    println!("  ring          {:?}", r.ring);
    println!("  contained     {}", r.contained);
    println!("  wrap forward  {:?}", r.wrapped);
    println!("  wrap back     {:?}", r.wrapped_back);
    println!("  snap back     {:?}", r.snapped_back);
    println!("  released      {}", r.released);
    println!("  restored      {:?}", r.restored);

    let want_ring = ["name-prompt-field", "name-prompt-ok", "name-prompt-cancel"];
    let mut bad = Vec::new();
    if r.ring != want_ring {
        bad.push(format!("the ring is {:?}, want {want_ring:?}", r.ring));
    }
    if !r.contained {
        bad.push("the background is not inert + aria-hidden".to_string());
    }
    if r.wrapped.as_deref() != Some("name-prompt-field") {
        bad.push(format!(
            "Tab past the last stop went to {:?}, not back to the first",
            r.wrapped
        ));
    }
    if r.wrapped_back.as_deref() != Some("name-prompt-cancel") {
        bad.push(format!(
            "Shift-Tab off the first stop went to {:?}, not to the last",
            r.wrapped_back
        ));
    }
    if r.snapped_back.as_deref() != Some("name-prompt-field") {
        bad.push(format!(
            "Tab with focus outside the dialog went to {:?}, not back to the \
             first stop",
            r.snapped_back
        ));
    }
    if !r.released {
        bad.push("the background is still inert after the dismissal".to_string());
    }
    if r.restored.as_deref() != Some("open-modal") {
        bad.push(format!(
            "the dismissal left focus on {:?}, not on the control that opened \
             the dialog",
            r.restored
        ));
    }

    if bad.is_empty() {
        println!("PASS: the trap contains, wraps both ways, snaps back and restores");
        std::process::exit(0);
    }
    for b in &bad {
        println!("FAIL: {b}");
    }
    std::process::exit(1);
}

fn fail(why: &str) -> ! {
    println!("FAIL: {why}");
    std::process::exit(1);
}
