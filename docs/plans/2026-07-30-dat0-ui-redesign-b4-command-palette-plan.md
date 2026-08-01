# B4 Command Palette — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the P3b command-palette stub with a real modal entity — fuzzy-ranked over the `ActionRegistry`, keyboard-driven, mounted through B1/B2's ModalHost — where every listed command actually runs.

**Architecture:** A pure model (`src/command_palette.rs`: ranking + visibility classification, no gpui) plus an entity view (`src/view/command_palette.rs`: `InputState` + virtualised results listbox). `WorkspaceShell` mounts it as its sixth modal — one line in `mounted_modals()` buys the scrim, the Tab trap, the single-modal assert and focus restore. A shell-side router gives the 7 Window-blocked descriptors the `&mut Window` the registry's `Fn(&mut App)` closures cannot have.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component pinned rev `0f0ab35`, `accesskit`/`kittest` behind `--features a11y-capture`.

**Design doc:** `docs/plans/2026-07-30-dat0-ui-redesign-b4-command-palette-design.md` (committed `66ab3ab`). Read it before Task 0.

## Global Constraints

- **Branch:** `feat/ui-redesign-b4-command-palette`, off main `b80cdb1`. One commit per task, `git commit -s` (DCO).
- **Never bump the gpui-component pinned rev** (`0f0ab35`). Every API claim in this plan was verified against that checkout.
- **Drive keyboard behaviour with `simulate_keystrokes`, NEVER `dispatch_action`.** The latter bypasses the keymap, so a green test can hide a dead production key path.
- **With nothing focused, Tab is completely inert** (dispatch path = window root alone). Every Tab-driven test must click into the shell first.
- `focus_stop(id: &'static str, fh: &FocusHandle, tab_index: isize, ring: Hsla, on_activate)` — 5 args since A6a. `a11y::FOCUS_RING` no longer exists; pass `cx.theme().d0().focus_ring`. `tab_index` is global, so pass `0` everywhere.
- `a11y()` and `a11y_label()` BOTH push a new capture node — never add one to a site that already has one. `a11y()` requires a `&'static str` id; dynamic ids can only use `a11y_label`.
- **No colour literals.** `tests/style_lint.rs`'s ratchet is `ALLOW = &[("window.rs", 1)]` and must not grow. The scanner matches banned colour-constructor names **in prose**, so doc comments must not spell them with call parens.
- **`tests/a11y_spike.rs` asserts an exact captured-node count of 8.** The palette is modal and must paint nothing on the empty hero, so this number must not move. If it does, the palette is rendering when it shouldn't.
- **i18n:** `dat0_i18n::t` over the flat `crates/dat0-i18n/src/strings/en.json`. **JSON silently overwrites duplicate keys** — grep before adding. No `palette.` key exists today (verified).
- **Local gate** (run by the controller, not per-task): `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test -p dat0-app` across `{}`, `--features a11y-capture`, `--features a11y-capture,gallery`. `cargo test --workspace` and `cargo bench` are unrunnable on this machine (macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift) — that is pre-existing and reproduces on `main`. `cargo build -p dat0-app --bin dat0` DOES work.
- **Per-task verification scope is four commands**, listed in each task. The full sweep is the controller's job — do not background a workspace-wide `cargo test` from inside a task.
- `grid/mod.rs` is not touched by any task. If a task wants to, stop and report.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `src/command_palette.rs` (rewrite in place) | Pure model: `filter()` (unchanged), `rank()`, `visible_items()`, `HIDDEN`, `WINDOW_ROUTED`, `open()` | T1, T3 |
| `src/view/command_palette.rs` (new) | `CommandPalette` entity + `CommandPaletteEvent` + `impl ModalContent` + render | T2 |
| `src/view/mod.rs` | `pub mod command_palette;` | T2 |
| `crates/dat0-i18n/src/strings/en.json` | 10 `palette.*` keys | T2 |
| `src/window.rs` | palette slot + drain + mount line + `run_palette_action` + event routing + `register_command_palette_keys` call | T3, T4 |
| `src/actions/{builtin,view_actions}.rs` | `keybinding:` populated on 6 descriptors | T5 |
| `tests/command_palette_nav.rs` (new binary → 112) | Integration suite | T2, T3, T4 |
| `tests/command_palette.rs` (existing) | Untouched — proves `filter()` kept its signature | — |

---

## Task 0: T0 hard gate — prove the two unproven assumptions ✅ DONE

> **Outcome (2026-07-30): all gates pass, but the design changed.** Full results in design §9.
> Headline: a bubble handler fires where the design said it could not, because `Input::render`
> registers the up/down handlers only `.when(is_multi_line())`. Chasing that turned up the real
> defect — `capture_action(MoveDown)` is **dead when focus is on the results list**, since the
> "Input" context is absent and no `MoveDown` is produced. B4 therefore ships dat0-owned
> `PaletteUp`/`PaletteDown` under key context `"CommandPalette"`, measured working from both stops
> (G2c). `uniform_list` rows DO reach the capture tree, but only the visible window — assertions
> must target rendered rows. The probe below is kept for the record; it has been run and deleted.

**Files:**
- Create (throwaway, deleted in step 6): `crates/dat0-app/tests/palette_t0_probe.rs`
- Modify (only if a gate fails): `docs/plans/2026-07-30-dat0-ui-redesign-b4-command-palette-design.md`

**Interfaces:**
- Consumes: nothing.
- Produces: a go/no-go on `uniform_list` (G1) and `capture_action` (G2). Every later task assumes both passed; the STOP clauses say exactly what changes if they didn't.

This task writes NO production code. Its output is knowledge plus, if a gate fails, an amended design doc.

- [x] **Step 1: Write the probe**

```rust
//! THROWAWAY T0 probe for slice B4 — deleted in this same task. Proves two
//! assumptions the design rests on. Not a test of dat0 behaviour.
use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Context, Entity, FocusHandle, ParentElement as _, Render, Styled as _, TestAppContext, Window, div, px, uniform_list};
use gpui_component::Root;
use gpui_component::input::{Input, InputState, MoveDown};

use dat0_app::a11y::{A11yExt as _, AccessRole};

struct Probe {
    input: Entity<InputState>,
    /// Bumped by the capture-phase handler. If MoveDown reaches InputState
    /// first (or is consumed before us), this stays 0.
    hits: Rc<RefCell<usize>>,
}

impl Render for Probe {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let hits = self.hits.clone();
        div()
            .key_context("CommandPalette")
            // G2: intercept BEFORE the Input's own bubble-phase handler.
            .capture_action(move |_: &MoveDown, _window, app| {
                *hits.borrow_mut() += 1;
                app.stop_propagation();
            })
            .child(Input::new(&self.input))
            .child(
                // G1: does a virtualised row reach the a11y capture tree?
                div().h(px(120.)).child(
                    uniform_list("probe-list", 30, |range, _window, _app| {
                        range
                            .map(|i| {
                                div()
                                    .a11y_label(AccessRole::Label, format!("probe row {i}"))
                                    .child(format!("probe row {i}"))
                            })
                            .collect::<Vec<_>>()
                    })
                    .h(px(120.)),
                ),
            )
    }
}

#[gpui::test]
fn t0_gates(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let hits = Rc::new(RefCell::new(0usize));
    let hits2 = hits.clone();
    let slot: Rc<RefCell<Option<Entity<Probe>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let probe = cx.new(|cx| Probe {
            input: cx.new(|cx| InputState::new(window, cx)),
            hits: hits2,
        });
        *slot2.borrow_mut() = Some(probe.clone());
        Root::new(probe, window, cx)
    });
    let probe = slot.borrow().clone().expect("probe captured");

    // Focus the text field, exactly as the palette does on open.
    let fh: FocusHandle = probe.read_with(vcx, |p, cx| p.input.read(cx).focus_handle(cx));
    vcx.update(|window, _cx| window.focus(&fh));
    vcx.run_until_parked();

    // G2 — a REAL keystroke, never dispatch_action.
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(*hits.borrow(), 1, "G2: capture_action did not see MoveDown");

    // G1 — force a frame and read the capture tree.
    // (Use whatever snapshot helper `tests/support/mod.rs` exposes; the point
    // is only whether "probe row 0" is present.)
    println!("G1: inspect the captured tree for 'probe row 0'");
}
```

- [x] **Step 2: Run G2**

Run: `cargo test -p dat0-app --features a11y-capture --test palette_t0_probe -- --nocapture`
Expected: PASS on the `hits == 1` assertion.

**STOP if it fails:** capture-phase interception does not work as read. Amend the design doc §4.1 with the measured behaviour, and switch the arrow keys to `ctrl-n`/`ctrl-p` bound under a `"CommandPalette"` key context (those are unbound upstream, so no depth contest). Report before continuing.

- [x] **Step 3: Run G1**

Extend the probe's final assertion to query the real snapshot the way `tests/a11y_content.rs` does (copy its `reset → refresh → run_until_parked → take_tree_update` helper verbatim), then assert `probe row 0` is present and — the interesting half — check whether a row well past the fold (say `probe row 25`) is *absent*.

Run: `cargo test -p dat0-app --features a11y-capture --test palette_t0_probe -- --nocapture`
Expected: `probe row 0` present.

**STOP if row 0 is absent:** virtualised children never reach the collector. Amend design §6 and replace `uniform_list` with a plain `div` list capped at the top 10 ranked matches (arrows clamp within the visible 10, no scroll handle, no `scroll_to_item`). Everything else in the design is unaffected. Report before continuing.

**Record either way:** whether off-screen rows are captured. If they are not, every later assertion must target a row the list actually renders — that constraint belongs in the design doc as an as-built note.

- [x] **Step 4: Prove ⌘⇧P is reachable from a test binary (G3)**

Add to the probe:

```rust
#[gpui::test]
fn t0_g3_palette_chord_is_bound_in_tests(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    // This is the whole point: production binds the chord in `run_app`, which
    // tests never call. Until T3 adds `register_command_palette_keys`, the
    // action must NOT resolve — proving the test harness really is missing it.
    let bound = cx.update(|cx| cx.is_action_available(&dat0_app::menu_macos::OpenCommandPalette));
    assert!(!bound, "expected the chord to be UNBOUND before T3 wires it");
}
```

Run: `cargo test -p dat0-app --test palette_t0_probe -- --nocapture`
Expected: PASS (the action is unavailable). This is the red half of T3's test — it documents the gap T3 must close.

- [x] **Step 5: Prove a probe descriptor round-trips (G4)**

```rust
#[test]
fn t0_g4_probe_descriptor_dispatch_is_observable() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use dat0_app::actions::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry};

    static FIRED: AtomicBool = AtomicBool::new(false);
    let reg = ActionRegistry::new();
    reg.register(ActionDescriptor {
        id: ActionId::from("probe.fire"),
        title: "Probe Fire".into(),
        group: ActionGroup::Navigation,
        keybinding: None,
        dispatch: Arc::new(|_app| FIRED.store(true, Ordering::SeqCst)),
    })
    .expect("register");
    // A hand-built registry needs no OnceCell install — this is why the palette
    // entity takes an `ActionRegistry` by value rather than reading the global.
    assert_eq!(reg.count(), 1);
}
```

Run: `cargo test -p dat0-app --test palette_t0_probe -- --nocapture`
Expected: PASS.

- [x] **Step 6: Delete the probe and commit the findings**

```bash
rm crates/dat0-app/tests/palette_t0_probe.rs
touch crates/dat0-app/src/lib.rs   # A6 lesson: a reverted/removed file can
                                   # leave cargo running a STALE binary
cargo test -p dat0-app --features a11y-capture 2>&1 | tail -5
```

If any gate deviated, append an `## 9. As-built T0 findings` section to the design doc recording what was measured (not what was expected) and commit that. If all four passed clean, commit a one-line note in the same section saying so — B3's `ba228f3` is the precedent, and a T0 that leaves no trace is a T0 nobody can audit later.

```bash
git add docs/plans/2026-07-30-dat0-ui-redesign-b4-command-palette-design.md
git commit -s -m "docs(theme): B4 T0 gate findings (UI redesign)"
```

---

## Task 1: The pure model — ranking and visibility

**Files:**
- Modify: `crates/dat0-app/src/command_palette.rs` (rewrite; keep `filter` + its private `subsequence_match` and their `mod tests` verbatim)
- Test: same file's `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `crate::actions::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry}`.
- Produces:
  - `pub const HIDDEN: &[&str; 6]`
  - `pub const WINDOW_ROUTED: &[&str; 7]`
  - `pub const PALETTE_CONTEXT: &str = "CommandPalette"`
  - `gpui::actions!(dat0_palette, [PaletteUp, PaletteDown]);`
  - `pub fn register_command_palette_keys(cx: &mut App)` — binds ⌘⇧P/⌃⇧P + `up`/`down`
  - `pub fn visible_items(reg: &ActionRegistry, query: &str) -> Vec<ActionDescriptor>`
  - `fn rank(title: &str, query: &str) -> Option<u8>` (private; exercised through `visible_items` and directly from the in-file tests)
  - `pub fn filter(...)` — **unchanged signature and body**

The actions and the registration land HERE, not in T3, because T2's tests press a real `down` key and a binding that does not exist yet makes that a no-op. T1's `register_command_palette_keys` wires ⌘⇧P to the existing `open()` — still the stub logger — and T3 rewrites only `open`'s body.

- [ ] **Step 1: Write the failing tests**

Append to `src/command_palette.rs`'s existing `mod tests`:

```rust
    use crate::actions::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry};
    use std::sync::Arc;

    fn reg_with(titles: &[(&str, &str)]) -> ActionRegistry {
        let reg = ActionRegistry::new();
        for (id, title) in titles {
            reg.register(ActionDescriptor {
                id: ActionId::from(*id),
                title: (*title).to_string(),
                group: ActionGroup::Navigation,
                keybinding: None,
                dispatch: Arc::new(|_| {}),
            })
            .expect("unique id");
        }
        reg
    }

    #[test]
    fn rank_prefers_prefix_then_word_boundary_then_subsequence() {
        assert_eq!(rank("Cancel Import", "can"), Some(3));
        assert_eq!(rank("Toggle SQL Console", "con"), Some(2), "word-boundary");
        assert_eq!(rank("Toggle SQL Console", "tsc"), Some(1), "subsequence");
        assert_eq!(rank("New Window", "xyz"), None);
    }

    #[test]
    fn visible_items_orders_by_score_then_title() {
        let reg = reg_with(&[
            ("a.one", "Toggle SQL Console"),
            ("a.two", "Cancel Import"),
            ("a.three", "Console Colors"),
        ]);
        let titles: Vec<String> = visible_items(&reg, "con")
            .into_iter()
            .map(|d| d.title)
            .collect();
        assert_eq!(
            titles,
            vec![
                "Console Colors".to_string(),   // 3: prefix
                "Toggle SQL Console".to_string(), // 2: word boundary
                "Cancel Import".to_string(),    // 1: subsequence (c-o-n)
            ]
        );
    }

    #[test]
    fn empty_query_lists_everything_visible_alphabetically() {
        let reg = reg_with(&[("a.z", "Zebra"), ("a.a", "Apple")]);
        let titles: Vec<String> = visible_items(&reg, "")
            .into_iter()
            .map(|d| d.title)
            .collect();
        assert_eq!(titles, vec!["Apple".to_string(), "Zebra".to_string()]);
    }

    #[test]
    fn hidden_ids_never_surface() {
        let reg = reg_with(&[("theme.toggle", "Toggle Theme"), ("window.new", "New Window")]);
        let ids: Vec<String> = visible_items(&reg, "")
            .into_iter()
            .map(|d| d.id.as_str().to_string())
            .collect();
        assert_eq!(ids, vec!["window.new".to_string()]);
    }

    #[test]
    fn hidden_and_window_routed_are_disjoint() {
        for id in HIDDEN {
            assert!(
                !WINDOW_ROUTED.contains(id),
                "{id} is both hidden and routed — one of the two lists is wrong"
            );
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dat0-app --lib command_palette`
Expected: FAIL — `cannot find function 'rank'`, `cannot find value 'HIDDEN'`.

- [ ] **Step 3: Write the implementation**

Replace the module doc and add below `subsequence_match` (leave `filter` and `subsequence_match` exactly as they are):

```rust
/// Registered but never shown in the palette. Each entry is dead for a reason
/// the palette cannot fix from a no-arg invocation:
///
/// - `file.open`, `theme.toggle`, `recents.show`, `sample_data.retry_taxi` —
///   the dispatch body is a `tracing` breadcrumb; there is nothing to run.
/// - `view.set_value` needs a `Scalar` and `view.delete_column` needs a
///   `col_ix`; the context menu passes them through a direct closure
///   (`edit_actions.rs:88-100`, `:116-128`). A fuzzy search box has neither.
///
/// Showing these would repeat the greyed-out-menu-item defect PRs #59/#60 fixed.
pub const HIDDEN: &[&str; 6] = &[
    "file.open",
    "theme.toggle",
    "recents.show",
    "sample_data.retry_taxi",
    "view.set_value",
    "view.delete_column",
];

/// Shown, but the registry closure is a breadcrumb: these need a `&mut Window`
/// that `DispatchFn`'s `Fn(&mut App)` cannot supply. `WorkspaceShell::
/// run_palette_action` runs them instead — the palette is a modal inside the
/// window, so it HAS the `Window` the boot-time closure lacks.
pub const WINDOW_ROUTED: &[&str; 7] = &[
    "console.toggle",
    "sql.new_tab",
    "sql.save_query",
    "sql.load_query",
    "sql.history",
    "sql.save_as_table",
    "view.save_as_table",
];

/// Match quality, higher is better. `None` = no match at all.
///
/// 3 = the title starts with the query; 2 = some word in the title starts with
/// it; 1 = the query is a subsequence, which is all [`filter`] itself requires.
/// Split out from `filter` rather than folded into it because `filter`'s
/// signature is pinned by `tests/command_palette.rs`.
fn rank(title: &str, query: &str) -> Option<u8> {
    let t = title.to_lowercase();
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(1);
    }
    if t.starts_with(&q) {
        return Some(3);
    }
    if t.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&q))
    {
        return Some(2);
    }
    subsequence_match(&t, &q).then_some(1)
}

/// Key context carried by the palette's root element. Every stop inside the
/// modal sits below it, which is what lets [`PaletteUp`]/[`PaletteDown`] match
/// from the query field AND from the results list.
pub const PALETTE_CONTEXT: &str = "CommandPalette";

gpui::actions!(dat0_palette, [PaletteUp, PaletteDown]);

/// Bind ⌘⇧P / ⌃⇧P and the palette-scoped arrows.
///
/// MUST be called by production (`run_app`) **and** by every test binary's
/// `init_components` — the harness calls only `gpui_component::init`, so a
/// prod-only binding is invisible to tests and a green suite can hide a dead
/// production key path (the carve-out #7 lesson, and the same rule
/// `overlay::register_modal_keys` carries). T0 gate G3 confirmed the chord is
/// genuinely unbound in a bare test app today.
///
/// The arrows are dat0 actions under [`PALETTE_CONTEXT`] rather than an
/// interception of upstream's `MoveUp`/`MoveDown`, because with focus on the
/// results list the "Input" key context is absent and those are never produced
/// at all (T0 gate G2c). See `view::command_palette`'s module docs.
pub fn register_command_palette_keys(cx: &mut gpui::App) {
    #[cfg(target_os = "macos")]
    let open_ks = "cmd-shift-p";
    #[cfg(not(target_os = "macos"))]
    let open_ks = "ctrl-shift-p";
    cx.bind_keys([
        gpui::KeyBinding::new(open_ks, crate::menu_macos::OpenCommandPalette, None),
        gpui::KeyBinding::new("up", PaletteUp, Some(PALETTE_CONTEXT)),
        gpui::KeyBinding::new("down", PaletteDown, Some(PALETTE_CONTEXT)),
    ]);
    cx.on_action(|_a: &crate::menu_macos::OpenCommandPalette, cx: &mut gpui::App| open(cx));
}

/// The palette's data source: everything that matches `query`, minus [`HIDDEN`],
/// in a DETERMINISTIC order. `ActionRegistry::iter` snapshots a `HashMap`, so
/// without this sort the list would reshuffle between frames.
pub fn visible_items(reg: &ActionRegistry, query: &str) -> Vec<ActionDescriptor> {
    let mut scored: Vec<(u8, ActionDescriptor)> = filter(reg, query)
        .into_iter()
        .filter(|d| !HIDDEN.contains(&d.id.as_str()))
        .filter_map(|d| rank(&d.title, query).map(|s| (s, d)))
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| sb.cmp(sa).then_with(|| a.title.cmp(&b.title)));
    scored.into_iter().map(|(_, d)| d).collect()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dat0-app --lib command_palette`
Expected: PASS, including the three pre-existing `subsequence_match_*` tests.

Then confirm the pinned signature really is untouched:
Run: `cargo test -p dat0-app --test command_palette`
Expected: PASS, file unmodified.

- [ ] **Step 5: Prove the tests are not vacuous**

Change `rank`'s word-boundary arm to `return Some(3)` and re-run: `visible_items_orders_by_score_then_title` must go RED. Revert, `touch crates/dat0-app/src/command_palette.rs`, re-run: GREEN.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/command_palette.rs
git commit -s -m "feat(theme): B4 T1 — palette ranking + visibility model"
```

---

## Task 2: The palette entity

**Files:**
- Create: `crates/dat0-app/src/view/command_palette.rs`
- Modify: `crates/dat0-app/src/view/mod.rs` (add `pub mod command_palette;` in alphabetical position)
- Modify: `crates/dat0-i18n/src/strings/en.json`
- Create: `crates/dat0-app/tests/command_palette_nav.rs`

**Interfaces:**
- Consumes: `crate::command_palette::visible_items` (T1); `crate::overlay::{ModalContent, ModalButton, modal_button}`; `crate::a11y::{A11yExt, AccessRole, FocusStopExt}`; `crate::theme::tokens::{Dat0Theme, Sp, SpStyled, TextRole, TypoStyled}`.
- Produces:
  - `pub enum CommandPaletteEvent { Run(ActionId), Cancel }`
  - `pub struct CommandPalette`
  - `pub fn CommandPalette::new(reg: ActionRegistry, window: &mut Window, cx: &mut Context<Self>) -> Self`
  - `pub fn input_focus_handle(&self, cx: &App) -> FocusHandle`
  - `impl ModalContent for CommandPalette` → `modal_focus_order` = `[input, list, close]`
  - `#[cfg(feature = "a11y-capture")] pub fn active_for_test(&self) -> usize`, `pub fn item_count_for_test(&self) -> usize`

**The entity takes the registry by value** (`ActionRegistry` is `Clone`, `Arc` inside) rather than reading `window_registry::action_registry()`. That is what lets a test hand it a two-descriptor probe registry with no `OnceCell` install, and it keeps the view free of global state.

- [ ] **Step 1: Add the i18n keys**

In `crates/dat0-i18n/src/strings/en.json`, first:

```bash
grep -n '"palette\.' crates/dat0-i18n/src/strings/en.json    # must print nothing
```

Then add (duplicate keys are silently overwritten — that is why the grep comes first):

```json
  "palette.title": "Command Palette",
  "palette.placeholder": "Type a command…",
  "palette.no_results": "No matching commands",
  "palette.group.navigation": "Navigation",
  "palette.group.theme": "Theme",
  "palette.group.file": "File",
  "palette.group.settings": "Settings",
  "palette.group.recovery": "Recovery",
  "palette.group.import": "Import",
  "palette.group.edit": "Edit",
```

- [ ] **Step 2: Write the failing test**

Create `crates/dat0-app/tests/command_palette_nav.rs`. Copy the snapshot/`init_components` scaffolding from `tests/modal_b2_nav.rs` verbatim (that is the established practice for a new nav binary), then:

```rust
/// Mount a standalone `CommandPalette` under a `Root`, the way
/// `tests/a11y_content.rs::open_sql_console_window` mounts a bare console:
/// production builds it with the same `ActionRegistry` + `&mut Window`.
fn open_palette(
    cx: &mut TestAppContext,
    reg: ActionRegistry,
) -> (Entity<CommandPalette>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<CommandPalette>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let p = cx.new(|cx| CommandPalette::new(reg, window, cx));
        *slot2.borrow_mut() = Some(p.clone());
        Root::new(p, window, cx)
    });
    let p = slot.borrow().clone().expect("palette captured");
    (p, vcx)
}

#[gpui::test]
fn arrows_move_the_active_row_and_clamp_at_both_ends(cx: &mut TestAppContext) {
    init_components(cx);
    let (palette, vcx) = open_palette(cx, probe_registry_with_three());
    vcx.run_until_parked();

    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 0);
    assert_eq!(palette.read_with(vcx, |p, _| p.item_count_for_test()), 3);

    // Up at the top is a no-op — list surfaces CLAMP, only radio groups wrap.
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 0);

    vcx.simulate_keystrokes("down down");
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 2);

    // …and clamps at the bottom too.
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 2);
}

#[gpui::test]
fn enter_emits_run_for_the_active_row(cx: &mut TestAppContext) {
    init_components(cx);
    let (palette, vcx) = open_palette(cx, probe_registry_with_three());
    let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let seen2 = seen.clone();
    let _sub = vcx.update(|_window, cx| {
        cx.subscribe(&palette, move |_p, ev: &CommandPaletteEvent, _cx| {
            if let CommandPaletteEvent::Run(id) = ev {
                seen2.borrow_mut().push(id.as_str().to_string());
            }
        })
    });
    vcx.run_until_parked();

    vcx.simulate_keystrokes("down enter");
    vcx.run_until_parked();
    assert_eq!(seen.borrow().len(), 1, "exactly one Run per Enter");
    assert_eq!(seen.borrow()[0], "probe.beta");
}

#[gpui::test]
fn typing_narrows_the_list_and_resets_the_active_row(cx: &mut TestAppContext) {
    init_components(cx);
    let (palette, vcx) = open_palette(cx, probe_registry_with_three());
    vcx.simulate_keystrokes("down down");
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 2);

    palette.update(vcx, |p, cx| p.seed_query_for_test("gam", cx));
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.item_count_for_test()), 1);
    assert_eq!(
        palette.read_with(vcx, |p, _| p.active_for_test()),
        0,
        "a stale active index would run the wrong command"
    );
}
```

`probe_registry_with_three()` builds descriptors `probe.alpha` "Alpha", `probe.beta` "Beta", `probe.gamma` "Gamma" using the T0 G4 shape.

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p dat0-app --features a11y-capture --test command_palette_nav`
Expected: FAIL — `unresolved import dat0_app::view::command_palette`.

- [ ] **Step 4: Write the entity**

Create `crates/dat0-app/src/view/command_palette.rs`:

```rust
//! Command palette modal (UI redesign B4).
//!
//! The MODEL — ranking, and which descriptors are fit to show — lives in
//! `crate::command_palette`; this file is the view. That split is what lets the
//! ranking be unit-tested with no `Window`, the same reason `overlay::next_index`
//! was extracted in B1.
//!
//! ## Why the arrows are palette-scoped ACTIONS
//!
//! `up`/`down` are bound to dat0's own `PaletteUp`/`PaletteDown` under key
//! context "CommandPalette" (`command_palette::register_command_palette_keys`),
//! and this file handles them with a plain `on_action`. Both halves were
//! measured by the T0 gate; neither is obvious:
//!
//! - **With focus in the query field**, two bindings match: upstream's
//!   `MoveDown` under context "Input" (deeper, so gpui chooses it first —
//!   `keymap.rs:165`) and ours. `MoveDown` finds NO handler, because
//!   `Input::render` registers `InputState::up`/`down` only
//!   `.when(state.mode.is_multi_line(), …)` (`input/input.rs:309-311`) and this
//!   field is single-line. Unhandled leaves `propagate_event` true, so the
//!   next-best binding — ours — wins.
//! - **With focus on the results list**, the "Input" context is not in the stack
//!   at all, so `down` produces `PaletteDown` directly.
//!
//! An earlier draft used `capture_action(MoveDown)` instead. It works from the
//! field and is DEAD on the list, where no `MoveDown` is ever produced — the
//! kind of hole every test written against the first stop would have missed.
//!
//! ⚠ The in-field half rests on that single-line registration guard. If this
//! field ever became multi-line, `MoveDown` would find a handler, consume, and
//! arrows would die in the field while still working on the list.
//! `arrows_move_the_active_row_and_clamp_at_both_ends` drives a real keystroke
//! with the field focused, so that regression fails a test.
//!
//! Enter and Escape need none of this: `enter()` emits `InputEvent::PressEnter`
//! and `escape()` propagates on a single-line field, exactly as `NamePrompt`
//! already relies on.

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, ParentElement, ScrollStrategy, SharedString,
    Styled, Subscription, UniformListScrollHandle, Window, div, uniform_list,
};
use gpui_component::ActiveTheme as _;
use gpui_component::input::{Escape, Input, InputEvent, InputState};

use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use crate::actions::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry};
use crate::command_palette::visible_items;
use crate::theme::tokens::{Dat0Theme as _, Sp, SpStyled as _, TextRole, TypoStyled as _};

/// What the palette asks the shell to do. The shell owns every dispatch.
#[derive(Debug, Clone)]
pub enum CommandPaletteEvent {
    /// Run this action, then dismiss. The shell dismisses FIRST — a routed
    /// action may open its own modal, and two mounted modals trip the
    /// single-modal `debug_assert!`.
    Run(ActionId),
    /// Dismiss without running anything.
    Cancel,
}

pub struct CommandPalette {
    /// The registry, by value. Held rather than read from
    /// `window_registry::action_registry()` so a test can hand this entity a
    /// two-descriptor probe registry with no `OnceCell` install.
    reg: ActionRegistry,
    input: Entity<InputState>,
    /// Ranked, visibility-filtered snapshot for the CURRENT query. Rebuilt on
    /// change, never per-frame: `ActionRegistry::iter` clones every descriptor
    /// (an `Arc` per dispatch closure), and a render-time rebuild would pay that
    /// on every frame.
    items: Vec<ActionDescriptor>,
    /// Keyboard-selected row. Clamped, never wrapped — list surfaces clamp,
    /// only radio groups wrap (`empty_state.rs:436-439`).
    active: usize,
    /// The results list is ONE tab stop; arrows move `active` within it.
    list_focus: FocusHandle,
    close_focus: FocusHandle,
    scroll: UniformListScrollHandle,
    _change_sub: Subscription,
    _enter_sub: Subscription,
}

impl CommandPalette {
    pub fn new(reg: ActionRegistry, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(dat0_i18n::t("palette.placeholder"))
        });
        let change_sub = cx.subscribe(&input, |this, input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let q = input.read(cx).value().to_string();
                this.items = visible_items(&this.reg, &q);
                // Reset rather than clamp: after a narrowing keystroke, row 2 of
                // the OLD list is a different command than row 2 of the new one,
                // and Enter would run it.
                this.active = 0;
                cx.notify();
            }
        });
        let enter_sub = cx.subscribe(&input, |this, _input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.run_active(cx);
            }
        });
        let items = visible_items(&reg, "");
        Self {
            reg,
            input,
            items,
            active: 0,
            list_focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            _change_sub: change_sub,
            _enter_sub: enter_sub,
        }
    }

    /// The query field's stop — the modal's FIRST stop, and the one the open
    /// path focuses so a user can type immediately.
    pub fn input_focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    fn run_active(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.items.get(self.active) {
            cx.emit(CommandPaletteEvent::Run(d.id.clone()));
        }
    }

    /// Move the selection by `delta`, clamped, and keep the active row on
    /// screen — without `scroll_to_item` the ring walks off the fold and a
    /// keyboard user loses track of it on a keyboard-first surface.
    fn move_active(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() - 1;
        self.active = (self.active as isize + delta).clamp(0, last as isize) as usize;
        self.scroll.scroll_to_item(self.active, ScrollStrategy::Top);
        cx.notify();
    }

    fn group_label(group: ActionGroup) -> SharedString {
        let key = match group {
            ActionGroup::Navigation => "palette.group.navigation",
            ActionGroup::Theme => "palette.group.theme",
            ActionGroup::File => "palette.group.file",
            ActionGroup::Settings => "palette.group.settings",
            ActionGroup::Recovery => "palette.group.recovery",
            ActionGroup::Import => "palette.group.import",
            ActionGroup::Edit => "palette.group.edit",
        };
        dat0_i18n::t(key).into()
    }
}

#[cfg(feature = "a11y-capture")]
impl CommandPalette {
    pub fn active_for_test(&self) -> usize {
        self.active
    }
    pub fn item_count_for_test(&self) -> usize {
        self.items.len()
    }
    /// Set the query without keystrokes. Goes through `set_value`, so the
    /// `Change` subscription — the code under test — still runs.
    pub fn seed_query_for_test(&self, q: &str, window: &mut Window, cx: &mut App) {
        let q = q.to_string();
        self.input.update(cx, |s, cx| s.set_value(q, window, cx));
    }
}

impl EventEmitter<CommandPaletteEvent> for CommandPalette {}

impl crate::overlay::ModalContent for CommandPalette {
    fn modal_title(&self, _cx: &App) -> SharedString {
        dat0_i18n::t("palette.title").into()
    }
    fn modal_focus_order(&self, cx: &App) -> Vec<FocusHandle> {
        vec![
            self.input_focus_handle(cx),
            self.list_focus.clone(),
            self.close_focus.clone(),
        ]
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ring = cx.theme().d0().focus_ring;
        let muted = cx.theme().muted_foreground;
        let active = self.active;
        let items = self.items.clone();
        let count = items.len();

        let entity_close = cx.entity();
        let close_btn = crate::overlay::modal_button(
            "palette-close",
            dat0_i18n::t("common.close").into(),
            &self.close_focus,
            crate::overlay::ModalButton::Ghost,
            cx,
            move |_window, app| {
                entity_close.update(app, |_t, cx| cx.emit(CommandPaletteEvent::Cancel));
            },
        );

        // Enter/Space on the LIST runs the active row — `focus_stop` supplies
        // this half of the keyboard contract for a user who Tabbed off the field.
        let activate = cx.listener(|this, _ev: &gpui::KeyDownEvent, _window, cx| {
            this.run_active(cx);
        });

        let rows = uniform_list("palette-results", count, move |range, _window, app| {
            range
                .map(|i| {
                    let d = &items[i];
                    let mut row = div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_sp(Sp::S8)
                        .px_sp(Sp::S8)
                        .py_sp(Sp::S4)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_sp(Sp::S8)
                                .child(SharedString::from(d.title.clone()))
                                .child(
                                    div()
                                        .text_role(TextRole::Caption)
                                        .text_color(muted)
                                        .child(Self::group_label(d.group)),
                                ),
                        )
                        .a11y_label(AccessRole::Button, d.title.clone());
                    if let Some(k) = &d.keybinding {
                        row = row.child(
                            div()
                                .text_role(TextRole::Caption)
                                .text_color(muted)
                                .child(SharedString::from(k.to_string())),
                        );
                    }
                    if i == active {
                        row = row.border_1().border_color(ring);
                    }
                    let _ = app;
                    row
                })
                .collect::<Vec<_>>()
        })
        .track_scroll(self.scroll.clone());

        div()
            .flex()
            .flex_col()
            .min_w(gpui::px(520.))
            // This context is what makes the palette-scoped `up`/`down`
            // bindings match — see the module docs.
            .key_context(crate::command_palette::PALETTE_CONTEXT)
            .on_action(cx.listener(|this, _: &crate::command_palette::PaletteDown, _window, cx| {
                this.move_active(1, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::command_palette::PaletteUp, _window, cx| {
                this.move_active(-1, cx);
            }))
            // Escape cancels from any stop. `overlay::register_modal_keys` binds
            // `escape` under the `Dat0Modal` context `modal_trap` installs on the
            // shell root, and a single-line Input propagates its own `escape()`
            // (`input/state.rs:1198`), so this ancestor handler catches both.
            .on_action(cx.listener(|_this, _ev: &Escape, _window, cx| {
                cx.emit(CommandPaletteEvent::Cancel);
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px_sp(Sp::S8)
                    .py_sp(Sp::S4)
                    .child(div().text_role(TextRole::Title).child(dat0_i18n::t("palette.title")))
                    .child(close_btn),
            )
            .child(div().px_sp(Sp::S8).py_sp(Sp::S4).child(Input::new(&self.input)))
            .child(
                div()
                    .h(gpui::px(320.))
                    .focus_stop("palette-results", &self.list_focus, 0, ring, activate)
                    .a11y("palette-results", AccessRole::Button, dat0_i18n::t("palette.title"))
                    .child(rows),
            )
            .when(count == 0, |d| {
                d.child(
                    div()
                        .px_sp(Sp::S8)
                        .py_sp(Sp::S4)
                        .text_color(muted)
                        .child(dat0_i18n::t("palette.no_results")),
                )
            })
    }
}
```

Add `pub mod command_palette;` to `crates/dat0-app/src/view/mod.rs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p dat0-app --features a11y-capture --test command_palette_nav`
Expected: PASS (3 tests).

Run: `cargo clippy -p dat0-app --all-targets -- -D warnings`
Expected: exit 0. (`pub const X: &'static [T]` trips `redundant_static_lifetimes`, which is warn-by-default and therefore fatal here — T1 already writes `&[&str; N]`.)

- [ ] **Step 6: Prove the arrow test is not vacuous — from BOTH stops**

Comment out the `PaletteDown` `on_action` line, `touch` the file, re-run: `arrows_move_the_active_row_and_clamp_at_both_ends` must go RED (active stays 0). Restore, `touch`, re-run: GREEN. **This is the assertion that proves the production key path is live rather than the test driving the model directly.**

Then add the second-stop test, which is the one T0 G2c showed the original design would have failed:

```rust
#[gpui::test]
fn arrows_work_with_focus_on_the_results_list_too(cx: &mut TestAppContext) {
    init_components(cx);
    let (palette, vcx) = open_palette(cx, probe_registry_with_three());
    // Move focus off the query field and onto the list container. The "Input"
    // key context leaves the stack here, so any mechanism keyed on upstream's
    // MoveDown is dead from this stop.
    let lf = palette.read_with(vcx, |p, _| p.list_focus_handle_for_test());
    vcx.update(|window, _cx| window.focus(&lf));
    vcx.run_until_parked();

    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 1);
}
```

Add `pub fn list_focus_handle_for_test(&self) -> FocusHandle` to the `a11y-capture` accessor block.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/view/command_palette.rs crates/dat0-app/src/view/mod.rs \
        crates/dat0-i18n/src/strings/en.json crates/dat0-app/tests/command_palette_nav.rs
git commit -s -m "feat(theme): B4 T2 — command palette entity"
```

---

## Task 3: Mount it — open path, modal registry, event routing

**Files:**
- Modify: `crates/dat0-app/src/command_palette.rs` (rewrite `open`)
- Modify: `crates/dat0-app/src/window.rs` (slot + drain + mount line + routing + key registration)
- Modify: `crates/dat0-app/tests/command_palette_nav.rs` (add shell-level tests + `init_components`)

**Interfaces:**
- Consumes: T2's `CommandPalette`, `CommandPaletteEvent`, `input_focus_handle`.
- Produces:
  - `pub fn crate::command_palette::open(app: &mut App)` — now really opens it
  - `pub fn crate::command_palette::register_command_palette_keys(cx: &mut App)`
  - `WorkspaceShell` fields `command_palette: Option<Entity<CommandPalette>>`, `command_palette_sub: Option<Subscription>`, `pending_palette_open: bool`
  - `#[cfg(feature = "a11y-capture")] pub fn command_palette_for_test(&self) -> Option<Entity<CommandPalette>>`

- [ ] **Step 1: Write the failing tests**

Append to `tests/command_palette_nav.rs`:

```rust
#[gpui::test]
fn cmd_shift_p_opens_the_palette_from_the_shell(cx: &mut TestAppContext) {
    init_components(cx);   // must now also call register_command_palette_keys
    let (shell, vcx) = open_shell_window(cx, test_session());
    focus_shell_neutrally(&shell, vcx);   // nothing focused ⇒ nothing dispatches

    assert_eq!(shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)), 0);
    #[cfg(target_os = "macos")]
    vcx.simulate_keystrokes("cmd-shift-p");
    #[cfg(not(target_os = "macos"))]
    vcx.simulate_keystrokes("ctrl-shift-p");
    vcx.run_until_parked();

    assert_eq!(
        shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)),
        1,
        "the palette is the mounted modal"
    );
}

#[gpui::test]
fn escape_dismisses_and_restores_focus(cx: &mut TestAppContext) {
    init_components(cx);
    let (shell, vcx) = open_shell_window(cx, test_session());
    focus_shell_neutrally(&shell, vcx);
    let before = vcx.update(|window, cx| window.focused(cx));

    open_palette_via_chord(vcx);
    vcx.run_until_parked();
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();

    assert_eq!(shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)), 0);
    assert_eq!(
        vcx.update(|window, cx| window.focused(cx)),
        before,
        "focus must return to where it was before the palette opened"
    );
}

#[gpui::test]
fn tab_cycles_the_three_stops_and_never_escapes(cx: &mut TestAppContext) {
    init_components(cx);
    let (shell, vcx) = open_shell_window(cx, test_session());
    focus_shell_neutrally(&shell, vcx);
    open_palette_via_chord(vcx);
    vcx.run_until_parked();

    // input → list → close → input. Four Tabs return to the start; a Tab that
    // escaped into the obscured shell would break this.
    let start = focused_label(vcx);
    vcx.simulate_keystrokes("tab tab tab");
    vcx.run_until_parked();
    assert_eq!(focused_label(vcx), start, "the trap must cycle, not leak");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p dat0-app --features a11y-capture --test command_palette_nav`
Expected: FAIL — the chord does nothing, `open_modal_count_for_test` stays 0.

- [ ] **Step 3: Rewrite `open`**

`register_command_palette_keys` already exists from T1. Replace only the `open` function body:

```rust
/// Ask the focused workspace to mount the palette on its next frame.
///
/// The handler stays GLOBAL rather than moving to a shell-root `.on_action`, as
/// the master plan suggested. B1 measured that with nothing focused the dispatch
/// path is the window root alone — eight Tab hops from a fresh window moved
/// focus not at all — so a shell-root handler would be silently dead on exactly
/// the window state where ⌘⇧P is most likely to be the user's first keystroke.
///
/// This path has no `Window` (and `InputState::new` needs one), so it sets a
/// flag that `WorkspaceShell::render` drains — B2's export-dialog idiom, where
/// focus set from inside `render` is proven to stick.
pub fn open(app: &mut gpui::App) {
    let Some(weak) = crate::window_registry::focused_workspace_weak() else {
        tracing::warn!("command_palette::open — no focused workspace; skipping");
        return;
    };
    let Some(shell) = weak.upgrade().and_then(|e| e.downcast::<crate::window::WorkspaceShell>().ok())
    else {
        tracing::warn!("command_palette::open — focused entity is not a WorkspaceShell");
        return;
    };
    shell.update(app, |ws, cx| ws.request_command_palette(cx));
}
```

Delete the old `open` body and its stale "T13 follow-up" doc comment. Update the module doc's first paragraph: the palette is no longer a stub.

- [ ] **Step 4: Wire the shell**

Five edits in `src/window.rs`:

1. **Fields** — next to `saved_picker` (~line 2342):

```rust
    /// The command palette modal (B4). `None` = closed.
    command_palette: Option<Entity<crate::view::command_palette::CommandPalette>>,
    /// Keeps the palette's event subscription alive; a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap).
    command_palette_sub: Option<Subscription>,
    /// Set by `command_palette::open`, which has no `Window`. Drained at the
    /// TOP of `render`, before the `pending_modal_focus` block, so the palette
    /// is mounted in time for that block to focus its first stop this frame.
    pending_palette_open: bool,
```

Initialise all three in `WorkspaceShell::new` (`None`, `None`, `false`) next to the existing `pending_modal_focus: false`.

2. **Request + routing methods**, next to `show_saved_picker`:

```rust
    /// Ask for the palette on the next frame. Windowless by construction: the
    /// only caller is the global ⌘⇧P handler.
    pub(crate) fn request_command_palette(&mut self, cx: &mut Context<Self>) {
        self.pending_palette_open = true;
        cx.notify();
    }

    /// Mount the palette. Called from `render`, which is where the `&mut Window`
    /// `InputState::new` needs comes from.
    fn mount_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::view::command_palette::{CommandPalette, CommandPaletteEvent};
        let Some(reg) = crate::window_registry::action_registry().cloned() else {
            tracing::warn!("command palette: no ActionRegistry installed; skipping");
            return;
        };
        let palette = cx.new(|cx| CommandPalette::new(reg, window, cx));
        let sub = cx.subscribe_in(
            &palette,
            window,
            |ws: &mut Self, _p, ev: &CommandPaletteEvent, window, cx| {
                ws.on_command_palette_event(ev.clone(), window, cx);
            },
        );
        self.command_palette_sub = Some(sub);
        self.command_palette = Some(palette);
        debug_assert!(
            self.open_modal_count(cx) <= 1,
            "two modals mounted at once ({}) — B1 assumes a single modal; see \
             docs/plans/2026-07-28-dat0-ui-redesign-b1-modal-host-design.md §2.7",
            self.open_modal_count(cx)
        );
    }

    /// Route a `CommandPaletteEvent`.
    ///
    /// ⚠ ORDER IS LOAD-BEARING: dismiss BEFORE running. `sql.save_query` and
    /// `sql.save_as_table` open a `NamePrompt`, and with the palette still
    /// mounted that is two modals — which the `debug_assert!` above rejects and
    /// which would leave the SECOND one untrapped in release.
    fn on_command_palette_event(
        &mut self,
        ev: crate::view::command_palette::CommandPaletteEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::command_palette::CommandPaletteEvent as E;
        let run = match ev {
            E::Run(id) => Some(id),
            E::Cancel => None,
        };
        self.command_palette = None;
        self.command_palette_sub = None;
        self.restore_modal_focus(window);
        let Some(id) = run else {
            cx.notify();
            return;
        };
        if !self.run_palette_action(&id, window, cx) {
            if let Some(reg) = crate::window_registry::action_registry() {
                if let Some(desc) = reg.get(&id) {
                    (desc.dispatch)(cx);
                }
            }
        }
        cx.notify();
    }
```

3. **Render drain** — insert at the very top of `Render::render`, *above* the `pending_modal_focus` block:

```rust
        // B4: build the palette here, where a `Window` exists. Placed BEFORE the
        // `pending_modal_focus` block on purpose — that block then finds the
        // palette in `mounted_modals()` and focuses its first stop (the query
        // field) in this same frame.
        if self.pending_palette_open {
            self.pending_palette_open = false;   // cleared unconditionally
            if self.command_palette.is_none() {
                self.mount_command_palette(window, cx);
                self.pending_modal_focus = true;
            }
        }
```

4. **Mount line** — one line in `mounted_modals`, after the saved picker:

```rust
        push_modal(&mut v, "command-palette-modal", &self.command_palette, cx);
```

5. **Replace the old registration** at `window.rs:1331-1346` — delete the whole `{ … cx.bind_keys(…); cx.on_action(…OpenCommandPalette…) }` block and its now-stale comment, and call `crate::command_palette::register_command_palette_keys(cx);` next to `crate::overlay::register_modal_keys(cx);` (~line 1794).

6. **A stub for T4's router**, so this task compiles on its own. T4 replaces the body; the signature is final:

```rust
    /// Run a `WINDOW_ROUTED` palette command with the `&mut Window` the registry
    /// closure cannot have, returning whether this id was ours. **T4 fills in the
    /// arms** — until then every id falls through to the registry path, which is
    /// exactly today's behaviour, so nothing regresses in between.
    pub(crate) fn run_palette_action(
        &mut self,
        _id: &crate::actions::ActionId,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> bool {
        false
    }
```

Add the test accessor beside `open_modal_count_for_test`:

```rust
    #[cfg(feature = "a11y-capture")]
    pub fn command_palette_for_test(
        &self,
    ) -> Option<gpui::Entity<crate::view::command_palette::CommandPalette>> {
        self.command_palette.clone()
    }
```

And in `tests/command_palette_nav.rs`, extend `init_components`:

```rust
fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(dat0_app::overlay::register_modal_keys);
    // Without this the ⌘⇧P chord is unbound in tests — T0 G3 proved it.
    cx.update(dat0_app::command_palette::register_command_palette_keys);
}
```

- [ ] **Step 5: Run to verify the tests pass**

Run: `cargo test -p dat0-app --features a11y-capture --test command_palette_nav`
Expected: PASS (6 tests).

Run: `cargo test -p dat0-app --features a11y-capture --test a11y_spike`
Expected: PASS — **the captured-node count must still be 8.** If it moved, the palette is painting on the empty hero; fix that rather than the number.

Run: `cargo test -p dat0-app --features a11y-capture --test modal_b2_nav`
Expected: PASS — the modal registry grew an entry, and this is the suite that guards it.

Run: `cargo test -p dat0-app --test menu_reachability`
Expected: PASS — `OpenCommandPalette` moved registration sites and this gate walks the real `build_menus` against `is_action_available`.

- [ ] **Step 6: Prove the focus-restore assertion is not vacuous**

Delete the `self.restore_modal_focus(window);` line, `touch` `window.rs`, re-run: `escape_dismisses_and_restores_focus` must go RED. Restore, `touch`, re-run: GREEN.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/command_palette.rs crates/dat0-app/src/window.rs \
        crates/dat0-app/tests/command_palette_nav.rs
git commit -s -m "feat(theme): B4 T3 — mount the palette as a modal + open path"
```

---

## Task 4: The router — make the 7 Window-blocked commands real

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (add `run_palette_action`)
- Modify: `crates/dat0-app/tests/command_palette_nav.rs`

**Interfaces:**
- Consumes: `crate::command_palette::WINDOW_ROUTED` (T1); the shell's existing `toggle_sql_console`, `show_saved_picker`, `open_name_prompt`, `open_name_prompt_with`, `open_save_view_as_table`, and `SqlConsole::{new_tab, show_history, active_sql_and_cursor}`.
- Produces: `pub(crate) fn run_palette_action(&mut self, id: &ActionId, window: &mut Window, cx: &mut Context<Self>) -> bool`.

- [ ] **Step 1: Write the failing tests**

```rust
#[gpui::test]
fn every_window_routed_id_is_actually_handled(cx: &mut TestAppContext) {
    init_components(cx);
    let (shell, vcx) = open_shell_window(cx, test_session());
    for id in dat0_app::command_palette::WINDOW_ROUTED {
        let handled = shell.update_in(vcx, |ws, window, cx| {
            ws.run_palette_action_for_test(&ActionId::from(*id), window, cx)
        });
        assert!(handled, "{id} is listed as window-routed but the router ignores it");
    }
}

#[gpui::test]
fn an_unrouted_id_falls_through_to_the_registry(cx: &mut TestAppContext) {
    init_components(cx);
    let (shell, vcx) = open_shell_window(cx, test_session());
    let handled = shell.update_in(vcx, |ws, window, cx| {
        ws.run_palette_action_for_test(&ActionId::from("settings.open"), window, cx)
    });
    assert!(!handled, "a live App-path action must NOT be claimed by the router");
}

#[gpui::test]
fn running_console_toggle_from_the_palette_mounts_the_console(cx: &mut TestAppContext) {
    init_components(cx);
    let (shell, vcx) = open_shell_window(cx, test_session());
    focus_shell_neutrally(&shell, vcx);
    assert!(shell.read_with(vcx, |s, _| s.sql_console_for_test().is_none()));

    shell.update_in(vcx, |ws, window, cx| {
        ws.run_palette_action_for_test(
            &ActionId::from("console.toggle"),
            window,
            cx,
        );
    });
    vcx.run_until_parked();
    assert!(
        shell.read_with(vcx, |s, _| s.sql_console_for_test().is_some()),
        "the breadcrumb dispatch would have left this None — that is the whole point of B4"
    );
}
```

If `sql_console_for_test` does not exist, add it under `#[cfg(feature = "a11y-capture")]` alongside the other test accessors, returning `Option<Entity<SqlConsole>>`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p dat0-app --features a11y-capture --test command_palette_nav`
Expected: FAIL — `no method named run_palette_action_for_test`.

- [ ] **Step 3: Implement the router**

```rust
    /// Run a `WINDOW_ROUTED` palette command with the `&mut Window` the registry
    /// closure cannot have, returning whether this id was ours.
    ///
    /// Every arm calls the SAME shell method the corresponding console event or
    /// menu item calls, so the palette is a third entry point rather than a
    /// second implementation. `false` means "not mine" and the caller falls back
    /// to `desc.dispatch(app)` — an unknown id must never be silently swallowed.
    ///
    /// The ids here are exactly `command_palette::WINDOW_ROUTED`;
    /// `every_window_routed_id_is_actually_handled` fails if the two drift.
    pub(crate) fn run_palette_action(
        &mut self,
        id: &crate::actions::ActionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::actions::builtin::ids;
        match id.as_str() {
            ids::CONSOLE_TOGGLE => self.toggle_sql_console(window, cx),
            ids::SQL_NEW_TAB => {
                if let Some(c) = self.sql_console.clone() {
                    c.update(cx, |c, cx| c.new_tab(window, cx));
                }
            }
            ids::SQL_HISTORY => {
                if let Some(c) = self.sql_console.clone() {
                    let entries = self.session.lock().query_history().to_vec();
                    c.update(cx, |c, cx| c.show_history(entries, cx));
                }
            }
            ids::SQL_SAVE_QUERY => {
                if let Some(c) = self.sql_console.clone() {
                    let sql = c.read(cx).active_sql_and_cursor(cx).0;
                    self.open_name_prompt(sql, window, cx);
                }
            }
            ids::SQL_LOAD_QUERY => self.show_saved_picker(window, cx),
            ids::SQL_SAVE_AS_TABLE => self.open_name_prompt_with(
                "Save as table…",
                "",
                NamePromptIntent::SaveConsoleAsTable,
                window,
                cx,
            ),
            ids::VIEW_SAVE_AS_TABLE => self.open_save_view_as_table(window, cx),
            _ => return false,
        }
        true
    }
```

Guards are deliberate: with no console mounted, `sql.new_tab` / `sql.history` / `sql.save_query` do nothing rather than panicking — the same shape the console-event arms already have. They still return `true`: the id *is* routed; there was simply nothing to act on.

Add the test accessor:

```rust
    #[cfg(feature = "a11y-capture")]
    pub fn run_palette_action_for_test(
        &mut self,
        id: &crate::actions::ActionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.run_palette_action(id, window, cx)
    }
```

- [ ] **Step 4: Run to verify the tests pass**

Run: `cargo test -p dat0-app --features a11y-capture --test command_palette_nav`
Expected: PASS (9 tests).

- [ ] **Step 5: Prove the drift gate bites**

Add `"sql.bogus"` to `WINDOW_ROUTED` in `src/command_palette.rs`, `touch` both files, re-run: `every_window_routed_id_is_actually_handled` must go RED. Remove it, `touch`, re-run: GREEN. Then also confirm the gate against the registry — add a test:

```rust
#[test]
fn listed_ids_are_all_really_registered() {
    let reg = ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&reg).expect("register_all");
    for id in dat0_app::command_palette::HIDDEN
        .iter()
        .chain(dat0_app::command_palette::WINDOW_ROUTED.iter())
    {
        assert!(reg.contains(id), "{id} is listed but not registered — stale id");
    }
}
```

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/command_palette_nav.rs
git commit -s -m "feat(theme): B4 T4 — route the seven Window-blocked commands"
```

---

## Task 5: Keybinding hints

**Files:**
- Modify: `crates/dat0-app/src/actions/view_actions.rs` (5 descriptors)
- Modify: `crates/dat0-app/src/actions/builtin.rs` (module doc note only)
- Test: `crates/dat0-app/tests/action_registry.rs`

**Interfaces:**
- Consumes: `gpui::Keystroke::parse`.
- Produces: `keybinding: Some(Keystroke)` on `view.undo`, `view.redo`, `view.export`, `sql.run`, `sql.cancel`, `console.toggle`. T2's row renderer already displays `d.keybinding` when present, so no view change is needed.

- [ ] **Step 1: Write the failing test**

Append to `tests/action_registry.rs`:

```rust
/// Every hint must parse, and must match the chord `window.rs` actually binds.
/// A typo would silently degrade to no hint, which looks identical to "this
/// command has no shortcut".
#[test]
fn keybinding_hints_parse_and_cover_exactly_the_bound_actions() {
    let reg = ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&reg).expect("register_all");

    let with_hints: std::collections::BTreeMap<String, String> = reg
        .iter()
        .filter_map(|d| d.keybinding.map(|k| (d.id.as_str().to_string(), k.to_string())))
        .collect();

    let expected: Vec<&str> = vec![
        "console.toggle",
        "sql.cancel",
        "sql.run",
        "view.export",
        "view.redo",
        "view.undo",
    ];
    assert_eq!(
        with_hints.keys().map(|s| s.as_str()).collect::<Vec<_>>(),
        expected,
        "window.new has NO cmd-n binding in the tree — hinting it would lie"
    );
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p dat0-app --test action_registry`
Expected: FAIL — the map is empty.

- [ ] **Step 3: Populate the six descriptors**

In `view_actions.rs`, replace `keybinding: None` on the five it owns. Each mirrors the `cfg` split at its binding site so the hint follows the platform:

```rust
        // Hint only — `window.rs:1358` is what actually binds this. B4 shows it
        // in the command palette; nothing dispatches from this field.
        keybinding: gpui::Keystroke::parse(if cfg!(target_os = "macos") {
            "cmd-z"
        } else {
            "ctrl-z"
        })
        .ok(),
```

| Descriptor | macOS | else | Bound at |
|---|---|---|---|
| `view.undo` | `cmd-z` | `ctrl-z` | `window.rs:1358` |
| `view.redo` | `cmd-shift-z` | `ctrl-shift-z` | `window.rs:1358` |
| `view.export` | `cmd-e` | `ctrl-e` | `window.rs:1392` |
| `sql.run` | `cmd-enter` | `ctrl-enter` | `window.rs:1536` |
| `sql.cancel` | `cmd-.` | `ctrl-.` | `window.rs:1536` |
| `console.toggle` | `cmd-shift-c` | `ctrl-shift-c` | `window.rs:1536` |

In `builtin.rs`, add to the module doc:

```rust
//! ⚠ `window.new` deliberately carries NO keybinding hint. The obvious guess is
//! ⌘N, but nothing in `src/` binds it (`grep '"cmd-n"'` is empty): `NewWindow`
//! has a global `on_action` handler and a File-menu item, but no `KeyBinding`,
//! so macOS derives no key equivalent and ⌘N does nothing. That is a real gap —
//! the same class as the dead menu items PRs #59/#60 fixed — and it wants its
//! own slice with a reachability assertion, not a hint that lies about it.
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p dat0-app --test action_registry`
Expected: PASS, including the pre-existing `count == 35`.

Run: `cargo test -p dat0-app --features a11y-capture --test command_palette_nav`
Expected: PASS — the hint column renders; nothing regressed.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/actions/view_actions.rs crates/dat0-app/src/actions/builtin.rs \
        crates/dat0-app/tests/action_registry.rs
git commit -s -m "feat(theme): B4 T5 — keybinding hints for the six bound actions"
```

---

## Task 6: Whole-branch gate

**Files:**
- Modify: `docs/plans/2026-07-30-dat0-ui-redesign-b4-command-palette-design.md` (as-built section)

No production code unless the gate finds something. **The cross-cutting review catches what per-task review structurally cannot** — that has now happened on B2 ("1 cells selected"), B3 (the singular-noun bug) and A3 (the two-muted-greys divergence), so budget real attention here rather than treating it as a formality.

- [ ] **Step 1: Full local gate**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app 2>&1 | tee /tmp/b4-plain.txt | tail -3
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b4-a11y.txt | tail -3
cargo test -p dat0-app --features a11y-capture,gallery 2>&1 | tee /tmp/b4-gal.txt | tail -3
grep -c "test result: ok" /tmp/b4-a11y.txt      # expect 112 (111 + the new binary)
```

Never pipe a binary count through `head` — `head` SIGPIPEs cargo mid-write and truncates the output (the A6 lesson: 51 counted instead of 109). Redirect to a file and count there.

- [ ] **Step 2: Ratchet + invariant checks**

```bash
cargo test -p dat0-app --test style_lint                 # 4/4, ALLOW still [("window.rs", 1)]
cargo test -p dat0-app --features a11y-capture --test a11y_spike   # node count still 8
cargo test -p dat0-app --test menu_reachability
cargo test -p dat0-app --test command_palette            # the pinned filter() signature
git diff main --stat -- crates/dat0-app/src/grid/        # must be EMPTY
```

- [ ] **Step 3: Build and drive the real app**

```bash
cargo build -p dat0-app --bin dat0
DAT0_CONFIG_DIR=/tmp/dat0-b4-glance ./target/debug/dat0
```

Walk it: ⌘⇧P on the empty hero → type "con" → confirm "Toggle SQL Console" ranks above subsequence matches → Enter → the console really opens. Reopen → arrow past the fold → confirm the ring stays visible. Escape → confirm focus lands where it started. Then ⌘⇧P → "save query" → Enter → confirm the NamePrompt opens with exactly one modal on screen (this is the dismiss-before-dispatch path).

- [ ] **Step 4: Read the whole diff**

```bash
git diff main -- crates/ | less
```

Look for: a deleted line nobody intended (the B2 near-miss — a heuristic deletion ran 163 lines past its target and was caught only by reading the diff); colour names spelled with call parens inside prose; an `a11y()` added to a site that already had one; any `dispatch_action` that crept into a test.

- [ ] **Step 5: Record as-built deviations**

Append `## 10. As-built` to the design doc: every place the implementation departed from §1-§8 and why. If T0 forced the cap-10 fallback or the ctrl-n/ctrl-p fallback, say so plainly here — a design doc that reads as if nothing went sideways is worse than no design doc.

- [ ] **Step 6: Commit**

```bash
git add docs/plans/2026-07-30-dat0-ui-redesign-b4-command-palette-design.md
git commit -s -m "docs(theme): B4 as-built notes (UI redesign)"
```

---

## After the branch is green

1. Push, open the PR, poll `gh pr checks` (**not** `gh run watch`).
2. Squash-merge with an explicit `--subject`/`--body-file`. **Never write the CI skip marker in any commit message, even quoted in prose** — it has been inherited through a squash body twice.
3. ⚠ **WATCH THE POST-MERGE MAIN RUN.** Verify at STEP level (a green job can mask a skipped bench) and `gh run download` the bench artifact. `grid/mod.rs` is untouched, and B3 proved `benches/grid_scroll.rs` never exercises the Table delegate anyway — so a "bench held" reading here is evidence of nothing either way. Record the number for the series regardless.
4. Check macOS `DISK[after-live-ai]` — B4 adds one test binary; B3 sat at 4.8 Gi and the #65 hotfix line is 2.9 Gi.
5. Owed human glance: all 3 themes, HC most of all — muted group tag legibility, Kbd hint contrast, active-row ring against the card, and the scroll behaviour past the fold.

---

## Self-review

**Spec coverage:** §2 module split → T1/T2 · §3 open path + dismiss-before-dispatch → T3 · §4 keyboard contract → T0 G2, T2 · §5.1 classification + gate → T1, T4 step 5 · §5.2 router → T4 · §5.3 ranking → T1 · §5.4 hints + the ⌘N finding → T5 · §6 rendering + i18n → T2 · §7 tests → T0, and one suite grown across T2/T3/T4 · §7.3 the count-of-8 check → T3 step 5 · §8 risks → T6.

**Placeholder scan:** no TBDs; every code step carries real code; the two STOP fallbacks are specified concretely rather than as "handle the failure".

**Type consistency:** `visible_items(&ActionRegistry, &str) -> Vec<ActionDescriptor>` is used identically in T1 and T2. `run_palette_action(&ActionId, &mut Window, &mut Context<Self>) -> bool` is called in T3 but its arms belong to T4 — caught in this review, and fixed by giving T3 step 4 edit 6 a stub with the FINAL signature that returns `false`. T3 therefore compiles alone and behaves exactly as today (registry path for everything); T4 replaces only the body. `CommandPaletteEvent::Run(ActionId)` carries the same type in T2 and T3. `HIDDEN`/`WINDOW_ROUTED` are `&[&str; N]` in T1 and iterated as such in T4.
