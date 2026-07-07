# Recents-list keyboard-nav — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Home-hero recents list keyboard-navigable (one tab stop, ↑/↓ move an active-index, Enter/Space opens the active row, ring on the active row) — a real release a11y fix — and automate its behavioral UAT via the Slice-6 focus-oracle harness.

**Architecture:** Listbox model reusing the *grid's* selection shape, not the *hero's* per-element shape. The recents list container becomes a single `focus_stop`; a `usize` active-index lives on the persistent `WorkspaceShell` (the transient `EmptyState` is rebuilt each frame); `focus_stop` supplies Enter/Space→activate + tab-stop + ring, and a **chained** second `on_key_down` adds ↑/↓. A pure `active_recent(entries, i)` seam makes selection unit-testable while the heavy file-open stays out of the automated path.

**Tech Stack:** Rust, gpui 0.2.2 + gpui-component, the `a11y-capture`-gated AccessKit test harness (`A11ySnapshot` / `focused_label` / `press_tab`), `#[gpui::test]` windowed tests, `serial_test`, `tempfile`.

## Global Constraints

- **Design doc:** `docs/plans/2026-07-06-dat0-uat-recents-nav-design.md` (authoritative; this plan implements it).
- **Branch:** `uat-recents-nav` off `main` (`d950b49`); design already committed (`b0b4ed9`).
- **Zero new dependencies.** `Cargo.lock` / `Cargo.toml` / `NOTICE.md` unchanged. D-015 stays open (no OS AccessKit adapter — the harness is test-only).
- **Production code ships unconditionally** (genuine a11y fix, like Slice 6) — `focus_stop`, the arrow `on_key_down`, the `recents_active` field, and the active-row ring are NOT feature-gated. ONLY the `record_focus_id` side-map call (inside `focus_stop`) and the `recents_active_for_test` accessor are `#[cfg(feature = "a11y-capture")]`.
- **a11y twin rule (Slice 6):** every `focus_stop` element carries a `.a11y(id, …)` twin with the **same `&'static str` id**. The oracle returns the twin's **label text**, not the id.
- **Single-source-of-truth (Slice 6):** a row's `on_click` and the Enter handler open via the same `open_recent_entry` path.
- **Anti-loop exec rule (Slice 1/4 lesson):** implementers run ONLY the fast focused test synchronously (`cargo test -p dat0-app --features a11y-capture --test recents_nav`); the **controller** runs the `cargo test --workspace --no-fail-fast` + `clippy --workspace --all-targets -D warnings` + `cargo fmt --all -- --check` + release-feature-off build gate. Never background `cargo test --workspace` inside an implementer turn.
- **Formatting:** run `cargo fmt --all` before every commit (plan code is not pre-formatted — Slice-5 lesson: the first commit failed the CI fmt gate).
- **Post-merge watch:** after squash-merge, WATCH the main run — the macOS grid-scroll bench is push-to-main-only (`ci.yml:210-226`) and can redden main silently.

## File map

- **Modify** `crates/dat0-app/src/a11y/mod.rs:29` — make `FOCUS_RING` `pub` so `empty_state.rs` can paint the active-row ring in the same hue.
- **Modify** `crates/dat0-app/src/empty_state.rs` — add `recents_active: usize` to `EmptyState` (+ `new`); rewrite `recents_column`; add the pure `active_recent` fn + its unit tests.
- **Modify** `crates/dat0-app/src/window.rs` — add `recents_active: usize` field (~2051) + ctor init (~2256); register the `"recents-list"` handle (~6103); thread `self.recents_active` into `EmptyState::new` (~6118); add the `recents_active_for_test` accessor (near `grid_active_cell_for_test`).
- **Create** `crates/dat0-app/tests/recents_nav.rs` — the windowed harness + assertions (mount helpers copied per-binary from `tests/keyboard_nav.rs`, per this crate's per-binary-copy precedent).

---

### Task 0: T0 spike — full production nav + the load-bearing assertions (HARD GATE)

Proves the two risks the whole slice rests on, with the real production code in place: **R4** Tab reaches the list (oracle names it), **R1** a chained `on_key_down` receives ↑/↓ under `TestPlatform` and moves the active-index, **R2** a seeded `recents.json` renders the rows. **If the down-arrow assertion is RED (arrows don't route to the chained handler), STOP** and switch to a single unified `on_key_down` handling ↑/↓/Enter/Space (dropping `focus_stop`'s activate arm, keeping its tab-stop + ring) — do not proceed to Task 1 on a red gate.

**Files:**
- Modify: `crates/dat0-app/src/a11y/mod.rs:29`
- Modify: `crates/dat0-app/src/empty_state.rs`
- Modify: `crates/dat0-app/src/window.rs`
- Test: `crates/dat0-app/tests/recents_nav.rs`

**Interfaces:**
- Consumes: `FocusStopExt::focus_stop(id, &FocusHandle, tab_index, on_activate)` and `A11yExt::a11y(id, role, label)` (`a11y/mod.rs`); `HeroHandles::get(id)` (`empty_state.rs`); `WorkspaceShell::hero_focus_handle(id, cx)` + `open_recent_entry(entry, cx)` (`window.rs`); `Recents::with_path(path).push(RecentEntry)` + `RecentEntry::{Workspace,Package}{path}` (`recents/mod.rs`); `A11ySnapshot::{capture, focused_label}` + `press_tab` (`tests/support/mod.rs`).
- Produces: `WorkspaceShell.recents_active: usize` (+ `recents_active_for_test()`); `empty_state::active_recent(&[RecentEntry], usize) -> Option<RecentEntry>`; the `"recents-list"` focus stop.

- [ ] **Step 1: Make `FOCUS_RING` public**

In `crates/dat0-app/src/a11y/mod.rs:29`:

```rust
/// Focus-ring hue — matches the grid active-cell ring (`grid/mod.rs:566`).
/// `pub` so the recents list can paint its active-row ring in the same hue.
pub const FOCUS_RING: u32 = 0x3b82f6;
```

- [ ] **Step 2: Add the `recents_active` field to `WorkspaceShell`**

In `crates/dat0-app/src/window.rs`, immediately after the `hero_focus` field (~2051):

```rust
    /// Active-row index for keyboard nav of the Home-hero recents list. Held on
    /// the persistent shell because the transient `EmptyState` is rebuilt every
    /// frame; clamped to the recents length at render. Slice: recents-nav.
    recents_active: usize,
```

And in the constructor, next to `hero_focus: std::collections::HashMap::new(),` (~2256):

```rust
            recents_active: 0,
```

- [ ] **Step 3: Register the `"recents-list"` focus handle + thread the index into `EmptyState`**

In `crates/dat0-app/src/window.rs`, change the `hero_ids` array (~6103) from 4 to 5 entries:

```rust
                let hero_ids: [&'static str; 5] = [
                    "hero-take-tour",
                    "hero-open-demo",
                    "hero-open-file-samples",
                    "hero-open-file-recents",
                    "recents-list",
                ];
```

And change the `EmptyState::new(...)` call (~6118) to pass the index:

```rust
                EmptyState::new(recents_empty, first_run_done, self.recents_active).render(&hero, cx)
```

- [ ] **Step 4: Add the `recents_active` parameter to `EmptyState`**

In `crates/dat0-app/src/empty_state.rs`, extend the struct and `new` (~103–115):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyState {
    pub recents_empty: bool,
    pub first_run_done: bool,
    /// Active-row index for the recents list (from `WorkspaceShell.recents_active`).
    /// Drives the active-row ring; the arrow handler mutates the shell field.
    pub recents_active: usize,
}

impl EmptyState {
    pub fn new(recents_empty: bool, first_run_done: bool, recents_active: usize) -> Self {
        Self {
            recents_empty,
            first_run_done,
            recents_active,
        }
    }
```

- [ ] **Step 5: Add the pure `active_recent` selection seam**

In `crates/dat0-app/src/empty_state.rs`, add a free function above the `#[cfg(test)] mod tests` block (items-after-test-module lint — must precede it):

```rust
/// The recent entry the active-index currently selects, or `None` if the list
/// is empty or the index is out of range. Pure — the unit-testable core of the
/// recents-list keyboard activation (mirrors Slice-4's `resolve_relaunch_action`
/// pure seam). The heavy file-open (`WorkspaceShell::open_recent_entry`) is NOT
/// exercised here.
fn active_recent(
    entries: &[crate::recents::RecentEntry],
    active: usize,
) -> Option<crate::recents::RecentEntry> {
    entries.get(active).cloned()
}
```

- [ ] **Step 6: Rewrite `recents_column`**

Replace the entire body of `fn recents_column` in `crates/dat0-app/src/empty_state.rs` with:

```rust
    fn recents_column(
        &self,
        hero: &HeroHandles,
        cx: &mut gpui::Context<crate::window::WorkspaceShell>,
    ) -> gpui::AnyElement {
        let recent_entries: Vec<crate::recents::RecentEntry> =
            if let Ok(cfg) = crate::platform::config_dir() {
                crate::recents::Recents::with_path(cfg.join("recents.json"))
                    .list()
                    .to_vec()
            } else {
                vec![]
            };
        let len = recent_entries.len();
        // Clamp the persisted index to the current list (a recent may have been
        // removed since the last nav) so the active-row ring never points off
        // the end. `recents_column` only renders when the list is non-empty
        // (`recents_empty == false`), so `len >= 1` here; the guard is defensive.
        let active = self.recents_active.min(len.saturating_sub(1));

        // Enter/Space activate: open whichever row the active-index selects, via
        // the SAME `open_recent_entry` path a row's `on_click` uses (mouse and
        // keyboard cannot drift — Slice-6 rule). `focus_stop` wires this to
        // Enter/Space internally.
        let entries_for_enter = recent_entries.clone();
        let activate = cx.listener(move |this, _ev: &gpui::KeyDownEvent, _window, cx| {
            if let Some(entry) = active_recent(&entries_for_enter, this.recents_active) {
                this.open_recent_entry(entry, cx);
            }
        });
        // ↑/↓ move the active-index. This is a SECOND `on_key_down` chained after
        // `focus_stop` (gpui pushes key-down listeners, so both fire); `len` is
        // captured for the down-clamp.
        let arrows = cx.listener(move |this, ev: &gpui::KeyDownEvent, _window, cx| {
            match ev.keystroke.key.as_str() {
                "down" => {
                    this.recents_active = (this.recents_active + 1).min(len.saturating_sub(1))
                }
                "up" => this.recents_active = this.recents_active.saturating_sub(1),
                _ => return,
            }
            cx.notify();
        });

        // The list is ONE tab stop (`focus_stop` on this container); arrows move
        // within it. The `.a11y` twin carries the SAME "recents-list" id so the
        // focus oracle can name the focused list by its label text.
        let mut list = div()
            .flex()
            .flex_col()
            .focus_stop("recents-list", hero.get("recents-list"), 0, activate)
            .on_key_down(arrows)
            .a11y(
                "recents-list",
                AccessRole::Button,
                dat0_i18n::t("hero.recent_label"),
            )
            .child(div().child(dat0_i18n::t("hero.recent_label")));

        for (i, entry) in recent_entries.into_iter().enumerate() {
            let label = entry.path().display().to_string();
            let id = gpui::SharedString::from(format!("hero-recent-{i}"));
            let handler = cx.listener(move |this, _ev, _window, cx| {
                this.open_recent_entry(entry.clone(), cx);
            });
            let mut row = div().id(id).child(label).on_click(handler);
            if i == active {
                row = row
                    .border_2()
                    .border_color(gpui::rgb(crate::a11y::FOCUS_RING));
            }
            list = list.child(row);
        }

        // The "Open file…" button remains a SEPARATE tab stop after the list
        // (unchanged from Slice 6, moved below the list container).
        let open_handler = cx.listener(|this, _ev, _window, cx| {
            this.open_file_picker(cx);
        });
        let open_key_handler = cx.listener(|this, _ev: &gpui::KeyDownEvent, _window, cx| {
            this.open_file_picker(cx);
        });
        let open_button = div()
            .id("hero-open-file-recents")
            .focus_stop(
                "hero-open-file-recents",
                hero.get("hero-open-file-recents"),
                0,
                open_key_handler,
            )
            .a11y(
                "hero-open-file-recents",
                AccessRole::Button,
                dat0_i18n::t("hero.open_file"),
            )
            .child(dat0_i18n::t("hero.open_file"))
            .on_click(open_handler);

        div()
            .flex()
            .flex_col()
            .child(list)
            .child(open_button)
            .into_any_element()
    }
```

- [ ] **Step 7: Add the `recents_active_for_test` accessor**

In `crates/dat0-app/src/window.rs`, next to the existing `grid_active_cell_for_test` accessor:

```rust
    /// Test oracle for recents-list arrow nav (mirrors `grid_active_cell_for_test`).
    #[cfg(feature = "a11y-capture")]
    pub fn recents_active_for_test(&self) -> usize {
        self.recents_active
    }
```

- [ ] **Step 8: Write the T0 spike test**

Create `crates/dat0-app/tests/recents_nav.rs`. Copy the mount scaffolding from `tests/keyboard_nav.rs` verbatim (per-binary-copy precedent): the `mod support;` line, the imports, `set_config_dir`, `build_empty_session`, `open_shell_window`, `init_components`. Then add:

```rust
/// Seed a real `recents.json` under the (serial) test's config dir with `n`
/// Package entries. `push` inserts most-recent-first, so pushing p0 then p1
/// yields on-disk order [p1, p0]; we push in REVERSE so row 0 == "/recent/0".
fn seed_recents(cfg: &Path, n: usize) {
    let mut r = dat0_app::recents::Recents::with_path(cfg.join("recents.json"));
    for i in (0..n).rev() {
        r.push(dat0_app::recents::RecentEntry::Package {
            path: PathBuf::from(format!("/recent/{i}")),
        })
        .expect("push recent");
    }
}

/// Tab from the neutral shell focus until the recents list is the focused stop,
/// or panic after a bounded number of hops (mirrors keyboard_nav.rs:278).
fn tab_to_recents_list(cx: &mut VisualTestContext) {
    let want = dat0_i18n::t("hero.recent_label");
    for _ in 0..12 {
        press_tab(cx);
        let snap = A11ySnapshot::capture(cx);
        if snap.focused_label() == Some(want.as_str()) {
            return;
        }
    }
    panic!("recents list was never the focused Tab stop within 12 hops");
}

/// T0 HARD GATE. Seeds 2 recents, mounts the shell, Tabs to the list (R4 +
/// oracle), then presses Down and asserts the active-index moved (R1: the
/// chained `on_key_down` receives the arrow under TestPlatform). If the Down
/// assertion fails, STOP — switch to a single unified `on_key_down`.
#[gpui::test]
#[serial]
fn t0_recents_list_tab_and_arrow(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    seed_recents(cfg.path(), 2);

    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    // R2: the seeded rows render (recents_empty == false → recents_column runs).
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label(&dat0_i18n::t("hero.recent_label")),
        "seeded recents list header must render"
    );

    // R4: Tab reaches the list; the oracle names it by its label text.
    focus_shell_neutrally(vcx);
    tab_to_recents_list(vcx);

    // R1: Down moves the active-index via the chained on_key_down.
    assert_eq!(
        shell.update(cx, |ws, _cx| ws.recents_active_for_test()),
        0,
        "active-index starts at 0"
    );
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(
        shell.update(cx, |ws, _cx| ws.recents_active_for_test()),
        1,
        "Down must move the recents active-index to 1 (R1 gate)"
    );
}
```

Also copy `focus_shell_neutrally` from `tests/keyboard_nav.rs` (it depends only on `hero-take-tour`, which the fresh-config first-run enriched band still paints alongside the seeded recents).

- [ ] **Step 9: Run the spike (HARD GATE)**

Run: `cargo test -p dat0-app --features a11y-capture --test recents_nav t0_recents_list_tab_and_arrow -- --nocapture`
Expected: **PASS**. If the Down assertion FAILS → STOP; the chained `on_key_down` doesn't receive arrows under `TestPlatform`. Fall back to a single unified `on_key_down` on the container (match `up`/`down`/`enter`/`space`; keep `focus_stop` for the tab-stop + ring but pass a no-op `on_activate`), re-run, then continue.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/a11y/mod.rs crates/dat0-app/src/empty_state.rs crates/dat0-app/src/window.rs crates/dat0-app/tests/recents_nav.rs
git commit -s -m "feat(a11y): recents-list keyboard nav — T0 spike + production nav (HARD GATE)"
```

---

### Task 1: Full arrow-nav + clamp + activation coverage

Locks in the remaining behavioral assertions (up/down clamp at both ends; up returns to 0) and the pure-seam unit tests. Given Task 0 shipped the clamp already, these are verification tests plus the `active_recent` unit coverage that guards the selection logic independently of GPUI.

**Files:**
- Test: `crates/dat0-app/tests/recents_nav.rs`
- Test: `crates/dat0-app/src/empty_state.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: everything Task 0 produced (`recents_active_for_test`, `active_recent`, the `"recents-list"` stop, `seed_recents`, `tab_to_recents_list`, `focus_shell_neutrally`).

- [ ] **Step 1: Write the `active_recent` unit tests**

In `crates/dat0-app/src/empty_state.rs`, inside `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn active_recent_selects_in_range_and_none_otherwise() {
        use crate::recents::RecentEntry;
        let entries = vec![
            RecentEntry::Package {
                path: std::path::PathBuf::from("/a"),
            },
            RecentEntry::Workspace {
                path: std::path::PathBuf::from("/b"),
            },
        ];
        assert_eq!(super::active_recent(&entries, 0), Some(entries[0].clone()));
        assert_eq!(super::active_recent(&entries, 1), Some(entries[1].clone()));
        // Out of range → None (guards against a stale index outrunning the list).
        assert_eq!(super::active_recent(&entries, 2), None);
        // Empty list → None (the recents_column isn't even rendered, but the
        // pure seam must still be total).
        assert_eq!(super::active_recent(&[], 0), None);
    }
```

- [ ] **Step 2: Run the unit test to verify it passes**

Run: `cargo test -p dat0-app --lib active_recent_selects_in_range_and_none_otherwise`
Expected: PASS.

- [ ] **Step 3: Write the arrow-clamp behavioral test**

In `crates/dat0-app/tests/recents_nav.rs`:

```rust
/// Arrow nav moves the active-index and clamps at both ends. Seeds 2 recents
/// (indices 0..=1); Up at 0 stays 0, Down past the last row stays at len-1.
#[gpui::test]
#[serial]
fn recents_arrows_move_and_clamp(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());
    seed_recents(cfg.path(), 2);

    init_components(cx);
    let session = build_empty_session(state.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    focus_shell_neutrally(vcx);
    tab_to_recents_list(vcx);

    let active = |cx: &mut TestAppContext| shell.update(cx, |ws, _cx| ws.recents_active_for_test());

    // Up at the top is a no-op (saturating).
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    assert_eq!(active(cx), 0, "Up at index 0 clamps to 0");

    // Down moves to the last row, then clamps there.
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(active(cx), 1, "Down moves to index 1");
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(active(cx), 1, "Down at the last row clamps to len-1");

    // Up returns toward the top.
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    assert_eq!(active(cx), 0, "Up returns to index 0");
}
```

- [ ] **Step 4: Run the focused test binary**

Run: `cargo test -p dat0-app --features a11y-capture --test recents_nav`
Expected: both `t0_recents_list_tab_and_arrow` and `recents_arrows_move_and_clamp` PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/recents_nav.rs crates/dat0-app/src/empty_state.rs
git commit -s -m "test(a11y): recents-list arrow clamp + active_recent unit coverage"
```

---

## Controller gate (run by the controller, NOT the implementer)

After both tasks land, the controller runs the full gate (anti-loop rule — never inside an implementer turn):

```bash
cargo fmt --all -- --check
cargo clippy -p dat0-app --all-targets --features a11y-capture -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
cargo build -p dat0-app --release   # feature-off: the nav ships, the accessor/oracle don't
```

- **Watch for R3 (cross-binary node-count drift):** the new `"recents-list"` `.a11y` node renders only when recents are non-empty. `a11y_spike.rs` mounts an empty-recents hero, so it should see zero new nodes — but the `cargo test --workspace --no-fail-fast` run is the backstop that catches any exact-count assertion the focused `--test recents_nav` cannot. If a count assertion in another binary breaks, update it (Slice-6 precedent: `a11y_spike` hero count 5→7).
- **Release build:** confirm `cargo build --release` (feature-off) succeeds and the `recents_active_for_test` accessor is absent (it is `#[cfg(feature = "a11y-capture")]`).

## Self-review (completed against the design)

- **Spec coverage:** design "production change" → Task 0 Steps 1–7; "pure activation seam" → Step 5 + Task 1 Step 1; assertion (1) Tab-reaches → Task 0 Step 8 `tab_to_recents_list`; assertion (2) arrows+clamp → Task 1 Step 3; assertion (3) `active_recent` unit → Task 1 Step 1; R1/R2/R4 → Task 0 Step 9 gate; R3 → Controller gate. Ring pixels + real file-open → design cut-lines (human), no task (correct).
- **Placeholder scan:** none — every step carries full code or an exact command.
- **Type consistency:** `recents_active: usize` (field / param / accessor) consistent; `active_recent(&[RecentEntry], usize) -> Option<RecentEntry>` used identically in `recents_column`, the unit test, and the design; `EmptyState::new(bool, bool, usize)` updated at its sole call site (window.rs ~6118); `recents_active_for_test()` matches the `grid_active_cell_for_test` read idiom.

## Known visual note (for the owed human glance)

`focus_stop` paints a border on the focused **container** AND the active **row** carries its own ring → when the list is focused there are two rings (list outline + active-row highlight). This is informative (list-focused + which-row-active) but may read as busy; the human focus-ring glance should evaluate it. If busy, a follow-up can add a ringless `focus_stop` variant for containers. Joins the standing About / Charts / Settings / Slice-6 ring glances (Gap 1, pixels stay human).

## Owed after merge

- **Human glance:** recents active-row ring + the container/row double-ring (above), WCAG ≥3:1 contrast in both themes.
- **Watch post-merge main** (macOS grid-scroll bench push-to-main-only → silent-red risk).
- **Memory update:** append Slice result to `[[dat0-uat-backlog-automation]]` NEXT-UP (recents done; Catalog reuses this list-nav pattern next).
