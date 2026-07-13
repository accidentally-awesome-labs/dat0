# AI config-panel keyboard-nav — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the AI-dock config buttons and the two SQL-Console AI trigger buttons real keyboard controls (Tab-reachable, Enter/Space-operable, focus-ring), verified by a headless AccessKit nav test.

**Architecture:** Reuse the release-real `focus_stop(id, &fh, tab_index, on_activate)` helper + its `.a11y(id, role, label)` twin (both keyed by the SAME `&'static str` id — the focus-oracle join key). AI dock = per-button hero-clone: `action_button` chains `focus_stop` + `.a11y`, stable handles live on `WorkspaceShell.hero_focus` and are threaded in via a `HeroHandles` map. SQL-Console chip/explain get two stable handles on the `SqlConsole` entity, wired inside their existing `if enabled` branch.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component 0.5.1, the `dat0-app` `a11y-capture` test feature (AccessKit + kittest oracle), tokio (test session), serial_test.

## Global Constraints

- **Rust toolchain pinned 1.97.0** — `cargo clippy --workspace --all-targets` under `-D warnings` must pass on 1.97.0 (local must match; `rustup update stable` if behind).
- **Zero new dependencies** — `Cargo.lock` / `Cargo.toml` / `NOTICE` unchanged (D-015 stays open). No gpui / gpui-component bump or fork (pinned 0.2.2 / 0.5.1).
- **`focus_stop` ships in release**; `.a11y` / `.a11y_label` are identity no-ops in release. The `use crate::a11y::{FocusStopExt as _, A11yExt as _, AccessRole};` imports are **UNCONDITIONAL** (used in both cfgs — do NOT `#[cfg]`-gate them; Slice-5 lesson).
- **Every `focus_stop` element MUST carry a matching `.a11y(id, …)` twin with the SAME `&'static str` id** — a `focus_stop` with no `.a11y` twin makes `focused_label()` return `None` (a bug the T0 gate surfaces).
- Only `*_for_test` accessors are `#[cfg(feature = "a11y-capture")]`; the production focus wiring is unconditional.
- `cargo fmt --all` before EVERY commit (plan code blocks are NOT pre-formatted — the CI `fmt --check` gate is unforgiving; Slice-5 T0 failed it). DCO: `git commit -s`.
- **Implementers run ONLY the focused test** `cargo test -p dat0-app --test ai_nav` synchronously; the **controller** runs the `cargo test --workspace` + clippy gate (anti-loop lesson).
- WATCH the post-merge main run — the push-to-main-only macOS grid-scroll bench can redden main silently.

---

## Reference: exact anchors on `main` (`02f7063`)

Line numbers drift — locate by the quoted anchor code, not the number.

- `crates/dat0-app/src/a11y/mod.rs:41` — `FocusStopExt::focus_stop(self, id: &'static str, fh: &FocusHandle, tab_index: isize, on_activate: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static) -> Self`.
- `crates/dat0-app/src/ai/panel.rs:94` — `pub fn render_ai_panel(panel: &AiPanel, cx: &mut Context<WorkspaceShell>) -> gpui::AnyElement`. Button helper `action_button` at `:240`. `AiPanel` struct (`pub`, all-`pub` fields) at `:51`.
- `crates/dat0-app/src/empty_state.rs:90` — `pub struct HeroHandles { pub map: HashMap<&'static str, FocusHandle> }`; `.get(id) -> &FocusHandle` at `:94`.
- `crates/dat0-app/src/window.rs`: `hero_focus: HashMap<&'static str, FocusHandle>` (`:2052`); `hero_focus_handle(&mut self, id: &'static str, cx: &mut App) -> FocusHandle` (`:5948`); `ai_panel_visible: bool` (`:2198`); `ai_panel: AiPanel` (`:2211`); catalog handle hoist `let catalog_fh = self.hero_focus_handle("catalog-tree", cx);` right before the root `div()` (`:6565`); AI-dock render `.children(self.ai_panel_visible.then(|| div().w_64().border_r_1().child(crate::ai::panel::render_ai_panel(&self.ai_panel, cx))))` (`:6686`); `handle_ai_panel_event` (`:5214`); `sql_console: Option<Entity<SqlConsole>>` (`:2090`); `toggle_sql_console(&mut self, window, cx)` (`:2796`); `spawn_ai_explain` early-returns on no-provider/no-key/empty-model (`:5530`).
- `crates/dat0-app/src/view/sql_console.rs`: `pub struct SqlConsole` (`:123`, `ai_ready: pub(crate)` at `:177`); `pub enum SqlConsoleEvent` (`#[derive(Debug, Clone)]`, `pub`) with `OpenNl2SqlPrompt`/`Explain` (`:199`); `SqlConsole::new(persisted, active, snapshot, window, cx)` (`:240`, `Self { … ai_ready: false }` at `:284`); `nl2sql-chip` in its `if enabled { chip.on_click(…) }` branch (`:1004`); `sql-explain` in its `if enabled { btn.on_click(…) }` branch (`:1025`).
- `crates/dat0-app/tests/catalog_nav.rs` — the mount-scaffolding template to COPY verbatim (`set_config_dir`, `build_empty_session`, `open_shell_window`, `init_components`, `focus_shell_neutrally`, `tab_to_*` bounded loop).
- `crates/dat0-app/tests/support/mod.rs` — `A11ySnapshot::{capture, focused_label() -> Option<&str>, has_label(&str) -> bool}`; free fns `press_tab(cx)`, `press_shift_tab(cx)`.
- `crates/dat0-app/tests/motherduck_window.rs` — Slice-5 precedent for reaching the console: shim invoked via `vcx.update(|window, app| shell.update(app, |ws, cx| ws.foo(.., window, cx)))`.

---

## Task 0: T0 spike — HARD GATE (minimal wiring proves all 4 probes)

Writes the production wiring for the AI dock (via the single `action_button`
change → all 8 ids) + the console `nl2sql-chip`, plus the three shims, plus ONE
gate test proving the four risks. **If any probe fails → STOP and re-scope**
(probes 3/4 fail → drop the console half; ship AI dock alone in later tasks).

**Files:**
- Modify: `crates/dat0-app/src/ai/panel.rs` (`render_ai_panel` signature + `action_button`)
- Modify: `crates/dat0-app/src/window.rs` (thread `HeroHandles` at the AI-dock render; add `seed_ai_panel_for_test`, `ai_panel_enabled_for_test`, `open_console_ready_for_test` shims)
- Modify: `crates/dat0-app/src/view/sql_console.rs` (two `FocusHandle` fields; wire `nl2sql-chip`)
- Create: `crates/dat0-app/tests/ai_nav.rs`

**Interfaces:**
- Consumes: `FocusStopExt::focus_stop`, `A11yExt::a11y`, `AccessRole::Button`, `HeroHandles`, `hero_focus_handle`, `handle_ai_panel_event`, `toggle_sql_console`.
- Produces (later tasks rely on these exact names):
  - `WorkspaceShell::seed_ai_panel_for_test(&mut self, panel: crate::ai::panel::AiPanel)` — sets `ai_panel` + `ai_panel_visible = true`, NO hydrate.
  - `WorkspaceShell::ai_panel_enabled_for_test(&self) -> bool`.
  - `WorkspaceShell::open_console_ready_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Entity<crate::view::sql_console::SqlConsole>` — toggles the console visible + sets `ai_ready = true` + returns the entity.
  - `render_ai_panel(panel, handles: &crate::empty_state::HeroHandles, cx)` — new signature.
  - AI-dock button ids (paint order): `"ai-toggle-enabled"`, `"ai-provider-cycle"`, `"ai-key-set"`, `"ai-key-forget"` (conditional), `"ai-model-set"`, `"ai-toggle-advanced"`, `"ai-toggle-sample-rows"`, `"ai-test-connection"`.
  - `SqlConsole` fields `nl2sql_focus: FocusHandle`, `explain_focus: FocusHandle`.

---

- [ ] **Step 1: Rewire `action_button` in `ai/panel.rs`**

At the top of `ai/panel.rs`, add the unconditional a11y imports beside the existing `use`s:

```rust
use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
```

Replace `action_button` (currently `:240`) with a version that takes a `&FocusHandle`, chains `focus_stop` + `.a11y`, and builds an Enter/Space handler mirroring the click handler:

```rust
/// A clickable, keyboard-operable panel button that dispatches `ev` to the shell
/// handler. `focus_stop` makes it a real Tab stop with Enter/Space activation +
/// focus ring (ships in release); the `.a11y` twin (same `id`) is the oracle's
/// label source and a release no-op. The Enter/Space handler calls the SAME
/// `handle_ai_panel_event` the `on_click` does, so keyboard and mouse cannot drift.
fn action_button(
    id: &'static str,
    label: impl Into<SharedString>,
    ev: AiPanelEvent,
    fh: &gpui::FocusHandle,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::Stateful<gpui::Div> {
    let label: SharedString = label.into();
    let ev_key = ev.clone();
    let click = cx.listener(move |ws, _ev, window, cx| {
        ws.handle_ai_panel_event(ev.clone(), window, cx);
    });
    let key = cx.listener(move |ws, _ev: &gpui::KeyDownEvent, window, cx| {
        ws.handle_ai_panel_event(ev_key.clone(), window, cx);
    });
    div()
        .id(id)
        .px_2()
        .py_1()
        .border_1()
        .cursor_pointer()
        .focus_stop(id, fh, 0, key)
        .a11y(id, AccessRole::Button, label.to_string())
        .child(label)
        .on_click(click)
}
```

- [ ] **Step 2: Thread `HeroHandles` through `render_ai_panel`**

Change the `render_ai_panel` signature (`:94`) to accept the handles, and pass
`handles.get(id)` at every `action_button` call. New signature line:

```rust
pub fn render_ai_panel(
    panel: &AiPanel,
    handles: &crate::empty_state::HeroHandles,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
```

Update each `action_button(...)` call to insert `handles.get("<id>")` as the 4th
arg (before `cx`). The eight call sites and their ids:

```rust
let enabled_row = action_button("ai-toggle-enabled", enabled_label, AiPanelEvent::ToggleEnabled, handles.get("ai-toggle-enabled"), cx);
let provider_row = action_button("ai-provider-cycle", provider_label(panel.provider), AiPanelEvent::SelectProvider(next_provider), handles.get("ai-provider-cycle"), cx);
// key_row: the "Set key…" button
.child(action_button("ai-key-set", dat0_i18n::t("ai.key.set_button"), AiPanelEvent::SetKey(String::new()), handles.get("ai-key-set"), cx));
// key_row (conditional forget):
key_row = key_row.child(action_button("ai-key-forget", dat0_i18n::t("ai.key.forget"), AiPanelEvent::ForgetKey, handles.get("ai-key-forget"), cx));
// model_row:
.child(action_button("ai-model-set", dat0_i18n::t("ai.model.set_button"), AiPanelEvent::SetModel(String::new()), handles.get("ai-model-set"), cx));
let advanced_row = action_button("ai-toggle-advanced", advanced_label, AiPanelEvent::ToggleAdvancedOverride, handles.get("ai-toggle-advanced"), cx);
let sample_row = action_button("ai-toggle-sample-rows", sample_label, AiPanelEvent::ToggleIncludeSampleRows, handles.get("ai-toggle-sample-rows"), cx);
let test_button = action_button("ai-test-connection", dat0_i18n::t("ai.test"), AiPanelEvent::TestConnection, handles.get("ai-test-connection"), cx);
```

(Leave the non-interactive `div().child(...)` labels — title, key-state,
model-display, test-result — untouched.)

- [ ] **Step 3: Thread the AI handles at the render call site (`window.rs`)**

Just before the root `div()` (right after the `let catalog_fh = …` hoist at
`:6565`), build the AI `HeroHandles`:

```rust
// AI-config-nav slice: stable per-button focus handles for the AI dock, minted
// on the persistent shell (hero_focus map) and threaded into `render_ai_panel`.
// Hoisted here — `hero_focus_handle` needs `&mut self`, unavailable inside the
// `.children(..)` render closures. Registering all 8 ids unconditionally is
// fine (`HeroHandles::get` is only invoked by whichever buttons actually render;
// `ai-key-forget` is only looked up when `key_set`).
let ai_handles = {
    let ids: [&'static str; 8] = [
        "ai-toggle-enabled",
        "ai-provider-cycle",
        "ai-key-set",
        "ai-key-forget",
        "ai-model-set",
        "ai-toggle-advanced",
        "ai-toggle-sample-rows",
        "ai-test-connection",
    ];
    let mut map = std::collections::HashMap::new();
    for id in ids {
        map.insert(id, self.hero_focus_handle(id, cx));
    }
    crate::empty_state::HeroHandles { map }
};
```

Update the AI-dock render (`:6690`) to pass `&ai_handles`:

```rust
.child(crate::ai::panel::render_ai_panel(&self.ai_panel, &ai_handles, cx))
```

- [ ] **Step 4: Add the AI shims on `WorkspaceShell` (`window.rs`)**

Add near the other `#[cfg(feature = "a11y-capture")]` shims (place BEFORE any
`#[cfg(test)] mod tests` in the file — `items-after-test-module` clippy lint):

```rust
/// Seed the AI dock draft state directly (bypassing `hydrate_ai_panel`, which
/// probes the OS keychain + settings.toml — the hermeticity trap) and open the
/// dock. Test-only.
#[cfg(feature = "a11y-capture")]
pub fn seed_ai_panel_for_test(&mut self, panel: crate::ai::panel::AiPanel) {
    self.ai_panel = panel;
    self.ai_panel_visible = true;
}

/// Read the AI dock's draft `enabled` flag (proves Enter-operability flipped it).
#[cfg(feature = "a11y-capture")]
pub fn ai_panel_enabled_for_test(&self) -> bool {
    self.ai_panel.enabled
}

/// Toggle the SQL console visible, mark AI ready (so the NL→SQL chip + Explain
/// button render their interactive `if enabled` branch), and return the console
/// entity so a test can subscribe to its `SqlConsoleEvent`s. Test-only.
#[cfg(feature = "a11y-capture")]
pub fn open_console_ready_for_test(
    &mut self,
    window: &mut Window,
    cx: &mut gpui::Context<Self>,
) -> gpui::Entity<crate::view::sql_console::SqlConsole> {
    if !self.sql_console_visible {
        self.toggle_sql_console(window, cx);
    }
    let console = self.sql_console.clone().expect("console built by toggle");
    console.update(cx, |c, _cx| c.ai_ready = true);
    console
}
```

- [ ] **Step 5: Add the two console focus handles (`sql_console.rs`)**

Add fields to `struct SqlConsole` (after `ai_ready: bool,` at `:177`):

```rust
    /// Stable focus handle for the NL→SQL chip (AI-config-nav slice). Minted once
    /// here so the chip is a stable Tab stop across re-renders.
    pub(crate) nl2sql_focus: gpui::FocusHandle,
    /// Stable focus handle for the Explain button (AI-config-nav slice).
    pub(crate) explain_focus: gpui::FocusHandle,
```

Initialise them in the `Self { … }` block of `new` (after `ai_ready: false,` at
`:300`):

```rust
            nl2sql_focus: cx.focus_handle(),
            explain_focus: cx.focus_handle(),
```

- [ ] **Step 6: Wire the `nl2sql-chip` focus_stop (`sql_console.rs`)**

Add the unconditional a11y imports near the top of `sql_console.rs`:

```rust
use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
```

In the `nl2sql-chip` block (`:1004`), the `if enabled { chip.on_click(…) }` arm
currently returns `chip.on_click(cx.listener(|_console, _ev, _window, cx| cx.emit(SqlConsoleEvent::OpenNl2SqlPrompt)))`. Replace that arm's body so the chip
also gets `focus_stop` + `.a11y`, with an Enter/Space handler that emits the SAME
event (`cx.listener` re-enters the `SqlConsole` context, so `cx.emit` is legal):

```rust
if enabled {
    let key = cx.listener(|_console, _ev: &gpui::KeyDownEvent, _window, cx| {
        cx.emit(SqlConsoleEvent::OpenNl2SqlPrompt);
    });
    chip.focus_stop("nl2sql-chip", &self.nl2sql_focus, 0, key)
        .a11y("nl2sql-chip", AccessRole::Button, dat0_i18n::t("sql.nl2sql.chip"))
        .on_click(cx.listener(|_console, _ev, _window, cx| {
            cx.emit(SqlConsoleEvent::OpenNl2SqlPrompt);
        }))
} else {
    chip
}
```

(Leave `sql-explain` for Task 2 — the T0 gate proves the console mechanism on
the chip alone.)

- [ ] **Step 7: Write the T0 gate test**

Create `crates/dat0-app/tests/ai_nav.rs`. Copy the scaffolding block
(`set_config_dir`, `build_empty_session`, `open_shell_window`, `init_components`,
`focus_shell_neutrally`) **verbatim** from `tests/catalog_nav.rs:23-116`
(adjust the module doc-comment). Then add the imports and the gate test:

```rust
use std::cell::RefCell;
use std::rc::Rc;

use dat0_app::ai::panel::AiPanel;
use dat0_app::ai::Provider;
use dat0_app::view::sql_console::{SqlConsole, SqlConsoleEvent};

/// Tab from the neutral shell focus until `want` is the focused stop, or panic
/// after 20 hops (catalog_nav.rs `tab_to_catalog` idiom).
fn tab_until(cx: &mut VisualTestContext, want: &str) {
    for _ in 0..20 {
        press_tab(cx);
        if A11ySnapshot::capture(cx).focused_label() == Some(want) {
            return;
        }
    }
    panic!("`{want}` was never the focused Tab stop within 20 hops");
}

/// T0 HARD GATE — four probes in one windowed test. Any failure → STOP/re-scope.
#[gpui::test]
#[serial]
fn t0_ai_nav_gate(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    // Seed the AI dock open with a known draft (bypasses keychain/settings).
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_ai_panel_for_test(AiPanel {
                provider: Some(Provider::Anthropic),
                key_set: false,
                model: String::new(),
                enabled: false,
                advanced_override: false,
                include_sample_rows: false,
                test_result: None,
            });
        });
    });
    vcx.run_until_parked();

    // Probe 1: Tab reaches ai-toggle-enabled; the oracle names it by its label.
    let enabled_label = dat0_i18n::t("ai.enabled.off");
    focus_shell_neutrally(vcx);
    tab_until(vcx, &enabled_label);

    // Probe 2: Enter flips the draft `enabled` flag (operability). The handler
    // also best-effort writes settings.toml → sandboxed by set_config_dir above.
    assert!(
        !shell.update(vcx, |ws, _cx| ws.ai_panel_enabled_for_test()),
        "enabled starts false"
    );
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        shell.update(vcx, |ws, _cx| ws.ai_panel_enabled_for_test()),
        "Enter on ai-toggle-enabled must flip the draft enabled flag (probe 2)"
    );

    // Probe 3: the console chip is Tab-reachable across the shell→console-view
    // boundary. Open the console ready + capture the entity.
    let console = vcx.update(|window, app| {
        shell.update(app, |ws, cx| ws.open_console_ready_for_test(window, cx))
    });
    vcx.run_until_parked();
    let chip_label = dat0_i18n::t("sql.nl2sql.chip");
    assert!(
        A11ySnapshot::capture(vcx).has_label(&chip_label),
        "nl2sql-chip a11y twin must render when ai_ready"
    );
    tab_until(vcx, &chip_label);

    // Probe 4: Enter on the focused chip emits OpenNl2SqlPrompt (observed via a
    // subscription BEFORE the shell's own downstream handler runs).
    let events: Rc<RefCell<Vec<SqlConsoleEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let ev2 = events.clone();
    let _sub = vcx.cx.update(|app| {
        app.subscribe(&console, move |_c, ev: &SqlConsoleEvent, _app| {
            ev2.borrow_mut().push(ev.clone());
        })
    });
    vcx.run_until_parked(); // subscription activation is deferred — flush it first
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        events
            .borrow()
            .iter()
            .any(|e| matches!(e, SqlConsoleEvent::OpenNl2SqlPrompt)),
        "Enter on nl2sql-chip must emit OpenNl2SqlPrompt (probe 4)"
    );

    drop(state);
}
```

- [ ] **Step 8: Run the T0 gate**

Run: `cargo test -p dat0-app --test ai_nav t0_ai_nav_gate`
Expected: PASS (all four probes).

**STOP-clauses (if it fails):**
- Probe 1/2 fail (AI dock) → the whole slice is blocked; investigate `focus_stop`
  on the dock (is the dock painted? is the `.a11y` twin present with the same id?).
- Probe 3/4 fail (console cross-boundary) → **drop the console half**: remove the
  chip wiring (Step 6) and Probes 3/4, ship the AI dock alone (Tasks 1 + 3 only),
  and defer `nl2sql-chip`/`sql-explain` to the SQL-Console slice. Record the drop
  in the design doc.
- Focus-entry unknown: if `focus_shell_neutrally` + Tab does not reach the chip
  (the console editor `Input` may self-focus, changing the entry point), try
  Tab-ing directly after `open_console_ready_for_test` without the neutral
  re-focus, or focus the console editor first. Resolve empirically here.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/ai/panel.rs crates/dat0-app/src/window.rs \
        crates/dat0-app/src/view/sql_console.rs crates/dat0-app/tests/ai_nav.rs
git commit -s -m "feat(a11y): AI-nav T0 gate — dock+console focus_stop wiring + 4-probe spike"
```

---

## Task 1: AI-dock reachability suite

Adds the breadth tests over the AI dock (production wiring already landed in
Task 0). No production change.

**Files:**
- Modify: `crates/dat0-app/tests/ai_nav.rs` (add tests)

**Interfaces:**
- Consumes: `seed_ai_panel_for_test`, `A11ySnapshot`, `press_tab`, the 8 button ids.

- [ ] **Step 1: Write the full-cycle reachability test**

Append to `tests/ai_nav.rs`. Helper to seed a dock in a given `key_set` state:

```rust
fn seed_ai_dock(shell: &Entity<WorkspaceShell>, vcx: &mut VisualTestContext, key_set: bool) {
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.seed_ai_panel_for_test(AiPanel {
                provider: Some(Provider::Anthropic),
                key_set,
                model: String::new(),
                enabled: false,
                advanced_override: false,
                include_sample_rows: false,
                test_result: None,
            });
        });
    });
    vcx.run_until_parked();
}

/// Collect the labels Tab visits, in order, up to `n` hops (stops early on repeat).
fn tab_labels(vcx: &mut VisualTestContext, n: usize) -> Vec<String> {
    let mut seen = Vec::new();
    for _ in 0..n {
        press_tab(vcx);
        if let Some(l) = A11ySnapshot::capture(vcx).focused_label() {
            let l = l.to_string();
            if seen.last() != Some(&l) {
                seen.push(l);
            }
        }
    }
    seen
}

#[gpui::test]
#[serial]
fn ai_dock_seven_buttons_reachable_in_paint_order(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_ai_dock(&shell, vcx, false);
    focus_shell_neutrally(vcx);

    let want = [
        dat0_i18n::t("ai.enabled.off"),
        provider_label_text(Some(Provider::Anthropic)),
        dat0_i18n::t("ai.key.set_button"),
        dat0_i18n::t("ai.model.set_button"),
        dat0_i18n::t("ai.advanced.off"),
        dat0_i18n::t("ai.sample_rows.off"),
        dat0_i18n::t("ai.test"),
    ];
    let seen = tab_labels(vcx, 40);
    for label in want {
        assert!(
            seen.contains(&label),
            "Tab never reached AI button {label:?}; visited {seen:?}"
        );
    }
    // Order: each expected label appears, and in the paint sequence above.
    let idxs: Vec<usize> = want
        .iter()
        .map(|l| seen.iter().position(|s| s == l).expect("present"))
        .collect();
    assert!(
        idxs.windows(2).all(|w| w[0] < w[1]),
        "AI buttons must be Tab-visited in paint order; got {seen:?}"
    );
    drop(state);
}
```

Add a small local helper mirroring `panel::provider_label` (it is private) so the
test can name the provider button label:

```rust
fn provider_label_text(p: Option<Provider>) -> String {
    match p {
        Some(p) => format!(
            "{}: {}",
            dat0_i18n::t("ai.provider"),
            dat0_i18n::t(&format!("ai.provider.{}", p.id()))
        ),
        None => dat0_i18n::t("ai.provider.unset"),
    }
}
```

(`Provider::id` is `pub`. If the exact provider label format drifts, read
`ai/panel.rs::provider_label` and match it verbatim.)

- [ ] **Step 2: Write the conditional-forget + panel-closed tests**

```rust
#[gpui::test]
#[serial]
fn ai_key_forget_is_reachable_when_key_set(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    seed_ai_dock(&shell, vcx, true); // key_set → ai-key-forget renders
    focus_shell_neutrally(vcx);

    let forget = dat0_i18n::t("ai.key.forget");
    assert!(
        A11ySnapshot::capture(vcx).has_label(&forget),
        "ai-key-forget must render when key_set"
    );
    let seen = tab_labels(vcx, 40);
    assert!(
        seen.contains(&forget),
        "ai-key-forget must be Tab-reachable when key_set; visited {seen:?}"
    );
    drop(state);
}

#[gpui::test]
#[serial]
fn ai_dock_not_a_tab_stop_when_closed(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    // Do NOT open the dock (ai_panel_visible stays false).
    focus_shell_neutrally(vcx);
    let enabled_label = dat0_i18n::t("ai.enabled.off");
    let seen = tab_labels(vcx, 40);
    assert!(
        !seen.contains(&enabled_label),
        "no AI button may be a Tab stop while the dock is closed; visited {seen:?}"
    );
    drop(state);
}
```

- [ ] **Step 3: Run the AI-dock suite**

Run: `cargo test -p dat0-app --test ai_nav`
Expected: PASS (T0 gate + the 3 new tests).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/ai_nav.rs
git commit -s -m "test(a11y): AI-dock reachability suite — 7-button order, forget, closed-negative"
```

---

## Task 2: SQL-Console `sql-explain` wiring + console suite

Wires the second console trigger and adds the console reachability + emit tests.
(Skip entirely if Task 0's STOP-clause dropped the console half.)

**Files:**
- Modify: `crates/dat0-app/src/view/sql_console.rs` (`sql-explain` branch)
- Modify: `crates/dat0-app/tests/ai_nav.rs` (add tests)

**Interfaces:**
- Consumes: `open_console_ready_for_test`, `SqlConsole::explain_focus`, `SqlConsoleEvent::Explain`.

- [ ] **Step 1: Wire the `sql-explain` focus_stop**

In the `sql-explain` block (`:1025`), replace the `if enabled { btn.on_click(…) }`
arm's body (mirrors Step 6 of Task 0, emitting `Explain`):

```rust
if enabled {
    let key = cx.listener(|_console, _ev: &gpui::KeyDownEvent, _window, cx| {
        cx.emit(SqlConsoleEvent::Explain);
    });
    btn.focus_stop("sql-explain", &self.explain_focus, 0, key)
        .a11y("sql-explain", AccessRole::Button, dat0_i18n::t("sql.explain.button"))
        .on_click(cx.listener(|_console, _ev, _window, cx| {
            cx.emit(SqlConsoleEvent::Explain);
        }))
} else {
    btn
}
```

- [ ] **Step 2: Write the console reachability + emit tests**

Append to `tests/ai_nav.rs`. Helper to open the console ready and capture the
entity + a subscription:

```rust
/// Open the console ready and subscribe to its events. Returns (console, log).
fn open_console_with_log(
    shell: &Entity<WorkspaceShell>,
    vcx: &mut VisualTestContext,
) -> (Entity<SqlConsole>, Rc<RefCell<Vec<SqlConsoleEvent>>>) {
    let console = vcx.update(|window, app| {
        shell.update(app, |ws, cx| ws.open_console_ready_for_test(window, cx))
    });
    vcx.run_until_parked();
    let log: Rc<RefCell<Vec<SqlConsoleEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let log2 = log.clone();
    // NOTE: the returned Subscription is intentionally leaked for the test's life
    // via mem::forget so it keeps firing (the test process is short-lived).
    let sub = vcx.cx.update(|app| {
        app.subscribe(&console, move |_c, ev: &SqlConsoleEvent, _app| {
            log2.borrow_mut().push(ev.clone());
        })
    });
    std::mem::forget(sub);
    vcx.run_until_parked(); // flush the deferred subscription activation
    (console, log)
}

#[gpui::test]
#[serial]
fn console_ai_triggers_reachable(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (_console, _log) = open_console_with_log(&shell, vcx);

    let chip = dat0_i18n::t("sql.nl2sql.chip");
    let explain = dat0_i18n::t("sql.explain.button");
    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label(&chip), "chip twin renders when ai_ready");
    assert!(snap.has_label(&explain), "explain twin renders when ai_ready");

    let seen = tab_labels(vcx, 40);
    assert!(seen.contains(&chip), "nl2sql-chip Tab-reachable; visited {seen:?}");
    assert!(seen.contains(&explain), "sql-explain Tab-reachable; visited {seen:?}");
    drop(state);
}

#[gpui::test]
#[serial]
fn enter_on_explain_emits_explain(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    let (_console, log) = open_console_with_log(&shell, vcx);

    let explain = dat0_i18n::t("sql.explain.button");
    tab_until(vcx, &explain);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(
        log.borrow().iter().any(|e| matches!(e, SqlConsoleEvent::Explain)),
        "Enter on sql-explain must emit Explain; got {:?}",
        log.borrow()
    );
    drop(state);
}
```

- [ ] **Step 3: Run the console suite**

Run: `cargo test -p dat0-app --test ai_nav`
Expected: PASS (all AI-nav tests). If `enter_on_explain_emits_explain` hangs or
panics, the shell's downstream `spawn_ai_explain` is NOT early-returning as
expected — seed the console test's shell AI provider to `None` (default) so the
provider guard returns first, and re-verify the subscription still records the
emit (the emit fires before the downstream runs).

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/view/sql_console.rs crates/dat0-app/tests/ai_nav.rs
git commit -s -m "feat(a11y): SQL-Console sql-explain focus_stop + console nav/emit tests"
```

---

## Task 3: Controller gate + final review

**Files:** none (verification only).

- [ ] **Step 1: Full workspace test run**

Run: `cargo test --workspace --no-fail-fast`
Expected: PASS. Watch for `a11y_spike` frame-count assertions — adding `.a11y`
nodes to the AI dock/console shifts OTHER binaries' captured node counts ONLY if
those binaries render the AI dock/console (they do not by default — the dock is
closed, `ai_ready` is false → chip/explain stay in their non-interactive branch
with no `.a11y`). If a count assertion trips, reconcile it (Slice-6 lesson: only
`--workspace` catches this, not the focused test).

- [ ] **Step 2: Clippy + fmt gate (pinned 1.97.0)**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Run: `cargo fmt --all -- --check`
Expected: both clean. If clippy is behind CI, `rustup update stable` first
(CI is pinned 1.97.0).

- [ ] **Step 3: Release feature-off build (prove no `a11y-capture` leak)**

Run: `cargo build -p dat0-app --release`
Expected: PASS. Confirms the production `focus_stop` wiring compiles with the
test feature OFF and no `#[cfg(feature = "a11y-capture")]` shim is referenced
from non-test code.

- [ ] **Step 4: Dependency-drift check**

Run: `git diff --stat main -- Cargo.lock NOTICE crates/dat0-app/Cargo.toml`
Expected: EMPTY (zero new deps; D-015 stays open).

- [ ] **Step 5: Final whole-branch review + PR**

Dispatch a fresh-context reviewer (opus) over the whole branch diff. Then open the
PR, poll `gh pr checks` (NOT `gh run watch`) until green on both platforms, squash-
merge, and WATCH the post-merge main run (macOS grid-scroll bench is push-to-main-
only → can redden main silently; confirm the Reclaim/Bench/Upload steps all report
`conclusion=success`). Record the owed human focus-ring glance (AI dock + 2 console
chips, both themes, WCAG ≥3:1).

---

## Self-review (plan vs. spec)

- **Spec coverage:** AI-dock 8-button wiring → Task 0 Steps 1-4 + Task 1 tests.
  Conditional `ai-key-forget` → Task 1 Step 2. SQL `nl2sql-chip` → Task 0 Step 6;
  `sql-explain` → Task 2 Step 1. Reachability tests → Tasks 0/1/2. Operability:
  Enter-flip (toggle) → Task 0 Probe 2; chip/explain emit-via-subscription →
  Task 0 Probe 4 + Task 2 Step 2. Seed-bypass-hydrate + config_dir sandbox →
  `seed_ai_panel_for_test` + `set_config_dir` scaffolding. Safety (no-Enter on
  side-effecting buttons) → only `ai-toggle-enabled` + the two safe emits are
  Enter-tested. T0 hard gate w/ STOP-clauses → Task 0 Step 8. Zero-deps / CI /
  owed glance → Task 3. All spec sections covered.
- **Placeholders:** none — every step has literal code/commands.
- **Type consistency:** `seed_ai_panel_for_test(AiPanel)`, `ai_panel_enabled_for_test() -> bool`,
  `open_console_ready_for_test(window, cx) -> Entity<SqlConsole>`,
  `render_ai_panel(panel, &HeroHandles, cx)`, `SqlConsoleEvent::{OpenNl2SqlPrompt, Explain}`,
  fields `nl2sql_focus`/`explain_focus` — used identically across tasks.
