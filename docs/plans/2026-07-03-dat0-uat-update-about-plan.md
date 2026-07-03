# Update + About dialogs UAT-automation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Headless-test the About box and in-app updater *dialog* UI (content +
`is_manual` gating + safe dismissal) via the AccessKit capture harness, closing
the P10a/P10a-2 UI slice of the UAT-backlog automation.

**Architecture:** Call the main-thread render helpers (`about::present`,
`update::ui::show_*`) DIRECTLY from a plain `&mut App` over a minimal
`gpui_component::Root` host window — bypassing the off-thread fetch, threads, and
dispatcher entirely. Annotate each dialog body with a test-only `.a11y_label`
(identity no-op in release) so `A11ySnapshot::capture` reads the real rendered
content. Expose the private render helpers via `#[cfg(feature = "a11y-capture")]`
public shims. This mirrors `tests/onboarding_gpui.rs`, which already tests the
same `cx.active_window()` + `window.open_dialog` path end to end (green).

**Tech Stack:** Rust, gpui 0.2.2 (`TestAppContext`/`VisualTestContext`),
gpui-component `Dialog`/`Root` (pinned rev `0f0ab35`), kittest 0.3.0 + accesskit
0.21.1 (via the `a11y-capture` feature), dat0's custom a11y collector
(`src/a11y/mod.rs`).

## Global Constraints

- **No new dependencies.** NOTICE / `Cargo.lock` must stay unchanged.
- **Zero release footprint.** All test surface is `#[cfg(feature = "a11y-capture")]`;
  `.a11y_label` compiles to an identity no-op without the feature
  (`a11y/mod.rs:187`). D-015 stays open (no gpui fork).
- **Never fire an `on_ok` that reaches `platform::open_url` or the installer.**
  `open_url` shells out to `open`/`xdg-open` (no seam → real browser in CI); the
  update prompt's Install & Restart `on_ok` spawns a real installer thread.
  Dismiss alerts with `enter` (harmless `on_ok`), confirm-variants with `escape`
  (harmless `on_cancel` = "Later"/Cancel). `Dialog` binds `escape→Cancel`,
  `enter→Confirm` (gpui-component `dialog.rs:24-25`).
- **Feature-flagged focused test command** (implementers run ONLY this,
  synchronously): `cargo test -p dat0-app --test update_about_window --features a11y-capture`.
  The CONTROLLER runs the workspace gate (`cargo test --workspace` +
  `cargo clippy --workspace --all-targets`).
- **Assert dialog content with `has_label_contains` (substring), not `has_label`
  (exact/unique).** The About body is one multi-line node; settle animations with
  `advance_clock(1s)` before capture (mid-animation frames duplicate nodes).
- **Never assert the git SHA** (non-deterministic across builds). Use
  `BuildInfo::current().version` for the version substring.
- Commits: DCO `-s` + `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File structure

- **Create** `crates/dat0-app/tests/update_about_window.rs` — the entire new test
  binary (`mod support;`), all ~9 tests + local harness (`DialogHost`,
  `open_dialog_host`, `dialog_open`, `fake_available_update`).
- **Modify** `crates/dat0-app/src/about/mod.rs` — wrap the `present` dialog body
  in `div().a11y_label(...)`; add `present_for_test` shim.
- **Modify** `crates/dat0-app/src/update/ui.rs` — annotate `show_alert_dialog` and
  `show_update_prompt` bodies; add four `*_for_test` shims.
- No other files change. `tests/support/mod.rs` is reused unchanged.

---

### Task 0: Spike hard-gate — About dialog opens, captures content, dismisses

**Files:**
- Create: `crates/dat0-app/tests/update_about_window.rs`
- Modify: `crates/dat0-app/src/about/mod.rs` (imports; `present` body; add `present_for_test`)
- Test: the spike test in the new file

**Interfaces:**
- Consumes: `dat0_app::about::present_for_test(cx: &mut App, newer: Option<String>)`
  (added here); `support::A11ySnapshot`; `gpui_component::{Root, WindowExt}`.
- Produces (for later tasks): the harness fns `open_dialog_host(cx) -> &mut VisualTestContext`,
  `dialog_open(cx: &mut VisualTestContext) -> bool`, and the `DialogHost` view.

**Gate:** If Step 5's content assertion (c) is RED after trying the settle bracket,
STOP and report — the a11y content approach is not viable and the slice must fall
back to presence + gating only (design §7 D1-fallback). Record the verdict in
`.superpowers/sdd/task-0-report.md`.

- [ ] **Step 1: Add the About source seam.** In `crates/dat0-app/src/about/mod.rs`,
  change the imports line

```rust
use gpui::{AnyView, App, ParentElement as _, Window};
```

to

```rust
use gpui::{AnyView, App, ParentElement as _, Window, div};

use crate::a11y::{A11yExt as _, AccessRole};
```

Then in `present`, change the dialog body line

```rust
                let dialog = dialog.title(title.clone()).child(body.clone());
```

to

```rust
                // Test-only content seam: wrap the body text in `.a11y_label`
                // so the headless UAT harness (`A11ySnapshot::capture`) can read
                // the rendered dialog content by its text. Identity no-op in
                // release (the `div` is an inert single child; `.a11y_label`
                // compiles away without the `a11y-capture` feature).
                let dialog = dialog.title(title.clone()).child(
                    div()
                        .child(body.clone())
                        .a11y_label(AccessRole::Label, body.clone()),
                );
```

At the end of the file, add the shim:

```rust
/// Test-only: drive `present` directly (bypassing the off-thread release check
/// in [`open`]) so the a11y harness can assert the dialog's content and
/// dismissal. Feature-gated → zero release footprint.
#[cfg(feature = "a11y-capture")]
pub fn present_for_test(cx: &mut App, newer: Option<String>) {
    present(cx, newer);
}
```

- [ ] **Step 2: Write the new test file with the harness + the spike test.**
  Create `crates/dat0-app/tests/update_about_window.rs`:

```rust
//! UAT "Update + About dialogs" slice (P10a / P10a-2 UI).
//!
//! Tests the About box and in-app updater DIALOGS: real rendered content,
//! the `is_manual` silent-background gating, and safe dismissal. Calls the
//! main-thread render helpers (`about::present`, `update::ui::show_*`) DIRECTLY
//! from a plain `&mut App` over a minimal `gpui_component::Root` host window —
//! no network, no `std::thread::spawn`, no dispatcher (unlike `about::open` /
//! `run_update_flow`, which do all three). Mirrors `tests/onboarding_gpui.rs`,
//! which proves the same `cx.active_window()` + `window.open_dialog` path and
//! that `.a11y_label`-annotated dialog bodies are read by `A11ySnapshot::capture`
//! (the dialog builder re-runs each frame, so the construction-time push()
//! re-fires under `capture`'s forced refresh).
//!
//! SAFETY: never dismiss a confirm-variant with `enter` — its OK button is
//! Download (About, newer) or Install & Restart (update prompt), whose `on_ok`
//! reaches `platform::open_url` (real browser) or the installer. Alerts →
//! `enter` (harmless `on_ok`); confirm-variants → `escape` (harmless `on_cancel`
//! = "Later"/Cancel).

mod support;

use std::time::Duration;

use gpui::{AppContext as _, TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};

use dat0_app::about::build_info::BuildInfo;
use support::A11ySnapshot;

/// A minimal host view that mounts gpui-component's DIALOG overlay layer (via
/// `Root::render_dialog_layer`) but nothing of its own. This layer is
/// LOAD-BEARING: `Root::render` paints ONLY `self.view`, so a host that does not
/// itself paint the dialog layer leaves `open_dialog` setting `active_*` state
/// while painting NOTHING — the dialog subtree (and thus its `.a11y_label`
/// content push) never renders, so `A11ySnapshot::capture` sees zero nodes even
/// though `has_active_dialog` is true. Production mirrors this exactly
/// (`window.rs:6566-6573`, `settings_ui/panel.rs:540`). We mount only the dialog
/// layer (not sheets), so the captured a11y frame is the dialog's own content.
struct DialogHost;
impl gpui::Render for DialogHost {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        gpui::div().children(Root::render_dialog_layer(window, cx))
    }
}

/// Open a real, ACTIVATED window whose root is a `gpui_component::Root` wrapping
/// a `DialogHost` — mirrors `onboarding_gpui::open_shell_window`. Activation
/// makes `cx.active_window()` (which `present`/`show_*` rely on) resolve to it.
fn open_dialog_host(cx: &mut TestAppContext) -> &mut VisualTestContext {
    // Required before any gpui-component widget (Dialog) is built.
    cx.update(gpui_component::init);
    let (_root, vcx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        let host = cx.new(|_| DialogHost);
        Root::new(host, window, cx)
    });
    vcx
}

/// True iff a dialog is currently on the window's `Root` stack.
fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, app| window.has_active_dialog(app))
}

// ----------------------------------------------------------------------------
// Task 0 — SPIKE HARD-GATE.
// ----------------------------------------------------------------------------

/// Proves, against the REAL `about::present`, that (a) the host window mounts
/// and activates so `active_window()` resolves; (b) `present_for_test(cx, None)`
/// opens a dialog (`has_active_dialog`); (c) the `.a11y_label`-annotated body is
/// read by the standard `A11ySnapshot::capture`; (d) `enter` dismisses the
/// alert. If (c) fails, STOP-and-report (design §7).
#[gpui::test]
fn spike_about_dialog_opens_captures_content_and_dismisses(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "(a) clean baseline: no dialog before open");

    // (b) Open the About box (up-to-date variant) from a plain App context —
    // `present` re-enters the active window itself, so it must NOT be nested in
    // a `VisualTestContext::update` window closure.
    vcx.cx
        .update(|app| dat0_app::about::present_for_test(app, None));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "(b) present must open the About dialog");

    // (c) Settle the open animation, then read the emitted tree. `has_label_contains`
    // finds the version substring inside the multi-line body node.
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(BuildInfo::current().version),
        "(c) GATE: dialog body content must be captured by A11ySnapshot \
         (version substring {:?} missing)",
        BuildInfo::current().version
    );
    // Teeth: a fabricated string must be absent — proves (c) reads real content.
    assert!(
        !snap.has_label_contains("NOTAREALVERSIONZZZ"),
        "a string the dialog never rendered must not be found"
    );

    // (d) `enter` fires the alert's harmless `on_ok` (|_,_,_| true) and closes it.
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "(d) enter must dismiss the About alert");
}
```

- [ ] **Step 3: Run the spike (source seam present).**

Run: `cargo test -p dat0-app --test update_about_window --features a11y-capture spike_about_dialog_opens_captures_content_and_dismisses -- --nocapture`
Expected: PASS. If assertion (c) fails, STOP — write `.superpowers/sdd/task-0-report.md` with the RED evidence and escalate (D1-fallback re-plan).

- [ ] **Step 4: Confirm release build is unaffected (no-op verification).**

Run: `cargo build -p dat0-app`
Expected: PASS with no warnings from `about/mod.rs` (the `.a11y_label`/`AccessRole`/`div` are all exercised by the unconditional wrapper, so no unused-import warnings off-feature).

- [ ] **Step 5: Commit.**

```bash
git add crates/dat0-app/tests/update_about_window.rs crates/dat0-app/src/about/mod.rs
git commit -s -m "test(uat): T0 spike — About dialog content+dismiss via a11y harness"
```

---

### Task 1: About content — up-to-date and newer-release variants

**Files:**
- Modify: `crates/dat0-app/tests/update_about_window.rs` (add two tests)

**Interfaces:**
- Consumes: `about::present_for_test`, `open_dialog_host`, `dialog_open`,
  `A11ySnapshot` (all from Task 0). No new source.

- [ ] **Step 1: Write the two content tests.** Append to `update_about_window.rs`:

```rust
// ----------------------------------------------------------------------------
// Task 1 — About box content (up-to-date + newer-release variants).
// ----------------------------------------------------------------------------

/// The up-to-date About box shows version + Apache-2.0 + the NOTICE line + the
/// "latest version" line, and NOT the "update available" line. Dismiss via
/// `enter` (alert OK is harmless `|_,_,_| true`).
#[gpui::test]
fn about_up_to_date_content(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx
        .update(|app| dat0_app::about::present_for_test(app, None));
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "About dialog must be open");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(BuildInfo::current().version),
        "About must show the crate version"
    );
    assert!(
        snap.has_label_contains("Apache-2.0"),
        "About must show the Apache-2.0 license id"
    );
    assert!(
        snap.has_label_contains(&dat0_i18n::t("about.acknowledgements")),
        "About must show the NOTICE acknowledgements line"
    );
    assert!(
        snap.has_label_contains(&dat0_i18n::t("about.update.current")),
        "up-to-date About must show the 'latest version' line"
    );
    // Teeth: the newer-release nudge line must be ABSENT in the up-to-date box.
    assert!(
        !snap.has_label_contains(&dat0_i18n::t("about.update.available")),
        "up-to-date About must NOT show the 'update available' nudge"
    );

    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "enter must dismiss the About alert");
}

/// The newer-release About box shows the "update available" line + the tag, and
/// NOT the "latest version" line. Dismiss via `escape` (Cancel) — NEVER `enter`,
/// whose OK is Download (opens the browser via `platform::open_url`).
#[gpui::test]
fn about_newer_release_content(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx
        .update(|app| dat0_app::about::present_for_test(app, Some("0.2.0".to_string())));
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "About dialog must be open");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(&dat0_i18n::t("about.update.available")),
        "newer-release About must show the 'update available' line"
    );
    assert!(
        snap.has_label_contains("0.2.0"),
        "newer-release About must show the newer tag"
    );
    // Teeth: the up-to-date line must be ABSENT in the newer-release box.
    assert!(
        !snap.has_label_contains(&dat0_i18n::t("about.update.current")),
        "newer-release About must NOT show the 'latest version' line"
    );

    // Dismiss via Cancel (escape) — must NOT fire the Download on_ok.
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "escape must dismiss the newer-release About");
}
```

> Note: `dat0_i18n::t(...)` is referenced by bare crate path — NO import needed.
> `dat0-i18n` is a normal `[dependencies]` entry of `dat0-app` (`Cargo.toml:43`),
> and Cargo makes a package's normal dependencies nameable in its integration
> tests (this is exactly how `settings_window.rs` calls bare `dat0_i18n::t(...)`
> with no `use`). Do NOT add a `use dat0_app::dat0_i18n;` line — it is unnecessary.

- [ ] **Step 2: Run the tests.**

Run: `cargo test -p dat0-app --test update_about_window --features a11y-capture about_up_to_date_content about_newer_release_content`
Expected: PASS (2 tests).

- [ ] **Step 3: Commit.**

```bash
git add crates/dat0-app/tests/update_about_window.rs
git commit -s -m "test(uat): About box content — up-to-date + newer-release variants"
```

---

### Task 2: Update alert seam + "checking" and "up to date" (manual + silent)

**Files:**
- Modify: `crates/dat0-app/src/update/ui.rs` (annotate `show_alert_dialog` body; add shims)
- Modify: `crates/dat0-app/tests/update_about_window.rs` (add three tests)

**Interfaces:**
- Produces: `dat0_app::update::ui::show_alert_dialog_for_test(cx: &mut App, title: String)`,
  `show_up_to_date_for_test(cx: &mut App, is_manual: bool)`.
- Consumes: the harness from Task 0.

- [ ] **Step 1: Annotate `show_alert_dialog` and add shims.** In
  `crates/dat0-app/src/update/ui.rs`, add a module-level import right after
  `use gpui::App;` (top of file):

```rust
use crate::a11y::{A11yExt as _, AccessRole};
```

In `show_alert_dialog`, change its local import

```rust
    use gpui::{AnyView, Window};
```

to

```rust
    use gpui::{AnyView, Window, div};
```

and change the dialog builder

```rust
            window.open_dialog(cx, move |dialog: Dialog, _w, _cx| {
                dialog
                    .title(title.clone())
                    .alert()
                    .on_ok(|_ev, _w, _cx| true)
            });
```

to

```rust
            window.open_dialog(cx, move |dialog: Dialog, _w, _cx| {
                dialog
                    .title(title.clone())
                    // Test-only content seam: an inert child that emits the
                    // title text as an `.a11y_label` node so the headless UAT
                    // harness can read it. Covers `checking` / `up_to_date` /
                    // `failed` (all route through `show_alert_dialog`). Identity
                    // no-op in release. `has_active_dialog` already asserts
                    // presence; this makes the CONTENT assertable.
                    .child(div().a11y_label(AccessRole::Label, title.clone()))
                    .alert()
                    .on_ok(|_ev, _w, _cx| true)
            });
```

At the end of `crates/dat0-app/src/update/ui.rs` (after the `tests` module, at
file scope), add:

```rust
/// Test-only shims: drive the main-thread render helpers directly (bypassing the
/// off-thread `run_update_flow`/`perform_install`) so the a11y harness can assert
/// each dialog's content, the `is_manual` gating, and dismissal. Feature-gated →
/// zero release footprint.
#[cfg(feature = "a11y-capture")]
pub fn show_alert_dialog_for_test(cx: &mut App, title: String) {
    show_alert_dialog(cx, title);
}

#[cfg(feature = "a11y-capture")]
pub fn show_up_to_date_for_test(cx: &mut App, is_manual: bool) {
    show_up_to_date(cx, is_manual);
}
```

- [ ] **Step 2: Write the three tests.** Append to `update_about_window.rs`:

```rust
// ----------------------------------------------------------------------------
// Task 2 — update "checking…" + "up to date" (manual shows / background silent).
// ----------------------------------------------------------------------------

/// The manual-path "checking…" alert opens with its text and dismisses on enter.
#[gpui::test]
fn update_checking_alert_content(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx.update(|app| {
        dat0_app::update::ui::show_alert_dialog_for_test(app, dat0_i18n::t("update.checking"))
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "checking alert must be open");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(&dat0_i18n::t("update.checking")),
        "checking alert must show its text"
    );

    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "enter must dismiss the checking alert");
}

/// Manual path (`is_manual=true`): "up to date" alert is SHOWN with its text.
#[gpui::test]
fn update_up_to_date_manual_shows(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx
        .update(|app| dat0_app::update::ui::show_up_to_date_for_test(app, true));
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "manual up-to-date must open a dialog");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(&dat0_i18n::t("update.up_to_date")),
        "manual up-to-date must show its text"
    );

    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "enter must dismiss the up-to-date alert");
}

/// Background path (`is_manual=false`): "up to date" is SILENT — no dialog.
/// (Teeth: `update_up_to_date_manual_shows` proves the same helper DOES open a
/// dialog when `is_manual=true`, so this negative is meaningful, not vacuous.)
#[gpui::test]
fn update_up_to_date_background_silent(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "clean baseline");

    vcx.cx
        .update(|app| dat0_app::update::ui::show_up_to_date_for_test(app, false));
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    assert!(
        !dialog_open(vcx),
        "background up-to-date must stay silent (no dialog)"
    );
}
```

- [ ] **Step 3: Run the tests.**

Run: `cargo test -p dat0-app --test update_about_window --features a11y-capture update_checking_alert_content update_up_to_date_manual_shows update_up_to_date_background_silent`
Expected: PASS (3 tests).

- [ ] **Step 4: Commit.**

```bash
git add crates/dat0-app/src/update/ui.rs crates/dat0-app/tests/update_about_window.rs
git commit -s -m "test(uat): update checking + up-to-date (manual shows / background silent)"
```

---

### Task 3: Update error dialog — manual shows, background silent

**Files:**
- Modify: `crates/dat0-app/src/update/ui.rs` (add `show_error_banner_for_test` shim)
- Modify: `crates/dat0-app/tests/update_about_window.rs` (add two tests)

**Interfaces:**
- Produces: `dat0_app::update::ui::show_error_banner_for_test(cx: &mut App, is_manual: bool, msg: &str)`.
- Consumes: the `show_alert_dialog` content seam from Task 2 (`show_error_banner`
  renders via `show_alert_dialog`, so the title text is already annotated).

- [ ] **Step 1: Add the error shim.** In `crates/dat0-app/src/update/ui.rs`,
  after `show_up_to_date_for_test`, add:

```rust
#[cfg(feature = "a11y-capture")]
pub fn show_error_banner_for_test(cx: &mut App, is_manual: bool, msg: &str) {
    show_error_banner(cx, is_manual, msg);
}
```

- [ ] **Step 2: Write the two tests.** Append to `update_about_window.rs`:

```rust
// ----------------------------------------------------------------------------
// Task 3 — update error dialog (manual shows / background silent).
// ----------------------------------------------------------------------------

/// Manual path: a failed check shows the "Update failed: {msg}" alert with both
/// the failure label and the underlying message.
#[gpui::test]
fn update_error_manual_shows(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx.update(|app| {
        dat0_app::update::ui::show_error_banner_for_test(app, true, "network down")
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "manual error must open a dialog");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(&dat0_i18n::t("update.failed")),
        "manual error must show the 'Update failed' label"
    );
    assert!(
        snap.has_label_contains("network down"),
        "manual error must show the underlying message"
    );

    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "enter must dismiss the error alert");
}

/// Background path: a failed launch-check stays SILENT — no dialog.
/// (Teeth: `update_error_manual_shows` proves the same helper DOES open when
/// `is_manual=true`.)
#[gpui::test]
fn update_error_background_silent(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "clean baseline");

    vcx.cx.update(|app| {
        dat0_app::update::ui::show_error_banner_for_test(app, false, "network down")
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    assert!(
        !dialog_open(vcx),
        "background error must stay silent (no dialog)"
    );
}
```

- [ ] **Step 3: Run the tests.**

Run: `cargo test -p dat0-app --test update_about_window --features a11y-capture update_error_manual_shows update_error_background_silent`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit.**

```bash
git add crates/dat0-app/src/update/ui.rs crates/dat0-app/tests/update_about_window.rs
git commit -s -m "test(uat): update error dialog — manual shows / background silent"
```

---

### Task 4: Update-available prompt — content + safe "Later" dismissal

**Files:**
- Modify: `crates/dat0-app/src/update/ui.rs` (annotate `show_update_prompt` body; add shim)
- Modify: `crates/dat0-app/tests/update_about_window.rs` (add `fake_available_update` + one test)

**Interfaces:**
- Produces: `dat0_app::update::ui::show_update_prompt_for_test(cx: &mut App, update: AvailableUpdate)`.
- Consumes: `dat0_app::update::AvailableUpdate` (pub, `mod.rs:17`) and
  `dat0_app::update::manifest::ArtifactEntry { url, sha256, size }` (pub).

- [ ] **Step 1: Annotate `show_update_prompt` and add the shim.** In
  `crates/dat0-app/src/update/ui.rs`, change `show_update_prompt`'s local import

```rust
    use gpui::{AnyView, ParentElement as _, Window};
```

to

```rust
    use gpui::{AnyView, ParentElement as _, Window, div};
```

and change its body child line

```rust
                    .child(dat0_i18n::t("update.downloading")) // placeholder body
```

to

```rust
                    // Test-only content seam: carry the "Update available:
                    // {version}" line (which otherwise lives only in the
                    // a11y-invisible title) as an `.a11y_label` node so the
                    // headless UAT can assert the version. Identity no-op in
                    // release; the visible "downloading…" placeholder is unchanged.
                    .child(
                        div()
                            .child(dat0_i18n::t("update.downloading"))
                            .a11y_label(AccessRole::Label, title.clone()),
                    )
```

At file scope (with the other shims), add:

```rust
#[cfg(feature = "a11y-capture")]
pub fn show_update_prompt_for_test(cx: &mut App, update: crate::update::AvailableUpdate) {
    show_update_prompt(cx, update);
}
```

- [ ] **Step 2: Write the prompt test + fake-update helper.** Append to
  `update_about_window.rs`:

```rust
// ----------------------------------------------------------------------------
// Task 4 — update-available prompt (content + safe "Later" dismissal).
// ----------------------------------------------------------------------------

/// Build a fake `AvailableUpdate` with NO network — `ArtifactEntry`'s fields are
/// pub and its URL is never fetched here (the prompt's Install & Restart `on_ok`
/// is never fired; we dismiss via "Later"/escape).
fn fake_available_update(version: &str) -> dat0_app::update::AvailableUpdate {
    dat0_app::update::AvailableUpdate {
        version: version.to_string(),
        artifact: dat0_app::update::manifest::ArtifactEntry {
            url: "https://example.invalid/dat0.tar.gz".to_string(),
            sha256: "00".repeat(32),
            size: 0,
        },
    }
}

/// The "Update available {version}" prompt opens with the version content, and
/// dismisses safely via "Later" (escape → `on_cancel`). NEVER fires Install &
/// Restart (`enter`), whose `on_ok` spawns the real installer.
#[gpui::test]
fn update_available_prompt_content_and_later_dismiss(cx: &mut TestAppContext) {
    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "clean baseline");

    vcx.cx.update(|app| {
        dat0_app::update::ui::show_update_prompt_for_test(app, fake_available_update("0.2.0"))
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "update-available prompt must open");

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(&dat0_i18n::t("update.available")),
        "prompt must show the 'Update available' line"
    );
    assert!(
        snap.has_label_contains("0.2.0"),
        "prompt must show the available version"
    );

    // Dismiss via "Later" (escape → Cancel → harmless on_cancel). Must NOT press
    // enter (that fires Install & Restart → spawns the installer).
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(
        !dialog_open(vcx),
        "'Later' (escape) must dismiss the prompt without installing"
    );

    // The window survived (no panic / no relaunch) — a fresh capture still works.
    let _ = A11ySnapshot::capture(vcx);
}
```

- [ ] **Step 3: Run the test.**

Run: `cargo test -p dat0-app --test update_about_window --features a11y-capture update_available_prompt_content_and_later_dismiss`
Expected: PASS (1 test).

- [ ] **Step 4: Run the whole new binary + release build.**

Run: `cargo test -p dat0-app --test update_about_window --features a11y-capture`
Expected: PASS (9 tests total).
Run: `cargo build -p dat0-app`
Expected: PASS, no warnings (release path: `.a11y_label`/`div`/`AccessRole` all exercised by unconditional wrappers).

- [ ] **Step 5: Commit.**

```bash
git add crates/dat0-app/src/update/ui.rs crates/dat0-app/tests/update_about_window.rs
git commit -s -m "test(uat): update-available prompt content + safe Later dismissal"
```

---

## Controller gate (after all tasks)

Not a task — the controller runs this once the per-task work lands, before the
final review:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Both must be clean. Then the final Opus whole-branch review, then open the PR
(`gh pr create`), poll `gh pr checks` (not `gh run watch`). After merge, WATCH the
post-merge main run (macOS grid-scroll bench is push-to-main-only → can redden
main silently).

## Self-review notes (spec coverage)

- About content (up-to-date + newer): Task 1 (+ spike Task 0). ✓
- 4 update states: checking (T2), up-to-date (T2), failed (T3), available-prompt
  (T4). ✓
- `is_manual` gating: up-to-date silent (T2), error silent (T3). ✓
- Never fire browser/installer: alerts→enter, confirm-variants→escape; prompt
  Install & Restart never pressed (Task 4). ✓
- Zero release footprint: `#[cfg(feature="a11y-capture")]` shims + no-op
  `.a11y_label`; release build verified in Task 0 Step 4 and Task 4 Step 4. ✓
- No new deps: only reuses `tests/support`, `dat0_i18n`, `tempfile`-free (no
  temp dirs needed — helpers take deterministic args). ✓
- Auto-check opt-out persistence: OUT OF SCOPE (already covered by
  `settings_window::updates_toggle_click_persists`); not re-tested here. ✓
