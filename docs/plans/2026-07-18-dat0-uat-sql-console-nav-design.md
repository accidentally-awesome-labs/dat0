# SQL Console keyboard-navigation — Design

**Date:** 2026-07-18
**Branch:** `uat-sql-console-nav` (off `main` `980c9a9`)
**Slice:** deferred kbd-nav carve-out #5 (after recents / catalog / AI-config / cell-editor).

## Goal

Make the SQL Console's **primary toolbar and tab strip keyboard-reachable and operable**, closing the documented gap in `docs/a11y.md` §3 ("Run/Cancel/tabs/save/history are code-verified via the View menu only, not Tab-reachable in-console"). Ships **real production a11y** (the `focus_stop` wiring is not feature-gated), plus behavioral coverage via the windowed AccessKit harness.

Out of scope (deliberately deferred): the code-editor `Input` and results `Table` (third-party gpui-component widgets with their own focus/keyboard model, already UAT-pending), and the transient overlay controls (error-strip dismiss, history-overlay rows + close, NL-preview stop/insert/discard, explain stop/close).

## Background (as-built)

- `SqlConsole` (`src/view/sql_console.rs`) is its own `Render` + `EventEmitter<SqlConsoleEvent>` entity, stored on `WorkspaceShell` as `sql_console: Option<Entity<SqlConsole>>` and toggled by `toggle_sql_console` (`window.rs`). It mints its own `FocusHandle`s as struct fields (`nl2sql_focus`, `explain_focus`, wired by the AI-config slice).
- Every toolbar control is a static-`&'static str`-id `div().id(...).on_click(...)` that already emits a `SqlConsoleEvent` — **except** the tab strip (`("sql-tab-label", i)` / `("sql-tab-close", i)`) and history rows (`("hist-row", i)`), which use dynamic tuple ids and mutate `self.active` / call `close_tab` inline.
- The two AI chips (`nl2sql-chip`, `sql-explain`) are already `focus_stop`-wired (this is the precedent to clone): `focus_stop(id, &fh, 0, on_activate).a11y(id, Button, label)` + `on_click`, both routing the same `cx.emit(SqlConsoleEvent::…)`.
- The reusable a11y kit (`src/a11y/mod.rs`): `FocusStopExt::focus_stop(id: &'static str, fh: &FocusHandle, tab_index: isize, on_activate: Fn(&KeyDownEvent, &mut Window, &mut App))`, `A11yExt::a11y(id, AccessRole, label)`, and the capture-only `focused_label(window)` oracle. `focus_stop` ships in release (real a11y); `a11y`/`focused_label` are capture-gated.
- The dynamic-list precedent (recents-nav, catalog-tree): ONE container `focus_stop` + an index field (`self.active` already exists) + a chained 2nd `.on_key_down` for arrow nav (gpui pushes key_down listeners → coexists with focus_stop's Enter/Space) + per-item ring on `i == active`. This is the model for the tab strip.

## Architecture

### 1. Toolbar buttons (fixed set)

Wire 7 fixed controls, each an exact clone of the shipped chip triad — `focus_stop(id, &fh, 0, on_activate).a11y(id, AccessRole::Button, label)` chained onto the existing `div().id(id)...on_click(...)`, where `on_activate` emits the **same** `SqlConsoleEvent` the `on_click` already emits (Enter/Space ≡ mouse click):

| id | control | event | notes |
|---|---|---|---|
| `sql-run` | Run / Cancel primary | `Run { target: MainGrid }` or `Cancel` | label + event flip on `self.running` |
| `sql-run-pane` | run-in-pane caret ▾ | `Run { target: Pane }` | idle-only → NOT a tab stop while running (like the disabled chip) |
| `sql-tab-add` | new-tab + | (inline `new_tab`) | `on_activate` replicates the inline mutation (gets `&mut Window`) |
| `sql-history` | history 🕘 | `ShowHistory` | |
| `sql-save` | save-query 💾 | `SaveQuery` | |
| `sql-saved` | saved-picker 📑 | `ShowSaved` | |
| `sql-save-as-table` | save-as-Table ⤓ | `SaveAsTable` | |

**Handle storage:** add `toolbar_focus: HashMap<&'static str, FocusHandle>` to `SqlConsole` with a get-or-insert helper `toolbar_fh(&mut self, id, cx) -> FocusHandle` (mirrors `WorkspaceShell::hero_focus_handle`; scales cleaner than 7 named fields). The 2 existing AI-chip handles stay as their named fields, untouched (no `ai_nav` retest risk; the mild inconsistency is acceptable — the chips predate the map).

`tab_index = 0` on every control → Tab cycles them in paint order, crossing the shell→console-entity boundary exactly as the two AI chips already do (proven by `ai_nav`'s Probe 3). No panel-level "enter the console" stop is added (the console panel wrapper has no focus handle today; each control is independently reachable, matching the chips).

### 2. Tab strip (tablist pattern)

ONE container `focus_stop("sql-tabstrip", &self.tabstrip_focus, 0, on_activate)` on the tab-strip row `div`, plus a chained 2nd `.on_key_down` handling ←/→/Delete/Backspace (coexists with focus_stop's Enter/Space listener — proven in recents-nav). Add one `tabstrip_focus: FocusHandle` field on `SqlConsole`.

Interaction (auto-activate model; `self.active` is the single source of truth — no separate "focused tab" index):
- **← / →** — move `self.active` by ∓1, clamped to `0..tabs.len()`, and emit `Persist` (identical to a tab click). Auto-activates: the tab under the cursor becomes the live tab immediately.
- **Delete / Backspace** — close the active tab via the existing `close_tab(self.active, cx)`; it clamps `active`. **No-op when only one tab remains** (mirrors the existing "✕ shown only when `tabs.len() > 1`" guard).
- **Enter / Space** (focus_stop's `on_activate`) — no-op (the active tab is already live).

**Focus-safety property:** because the strip is ONE container stop keyed off `self.active` (not per-tab handles), closing a tab cannot orphan focus — the `tabstrip_focus` handle stays valid and focused across the close. This sidesteps the self-removing-control hand-off problem (`ai_nav::forget_key_hands_focus_to_set_key`) by construction.

The per-tab ✕ mouse buttons and per-tab click labels are unchanged (still mouse-operable via their dynamic ids).

### 3. Focus ring

`focus_stop` already paints a focus ring (`border_2 + FOCUS_RING`) on the focused control. The tab strip's ring lands on the container row. **New owed human glance:** WCAG ≥3:1 ring contrast on the ~8 new controls (7 buttons + tab strip) in both themes — joins the standing ring-glance backlog; no automated contrast assertion beyond the existing a11y contrast gate.

## Data flow

Keystroke → focused control's `focus_stop` on_key_down (Enter/Space) **or** the tab strip's chained on_key_down (←/→/Delete) → `cx.emit(SqlConsoleEvent::…)` (buttons) or inline `self.active` mutation / `close_tab` (tab strip) → `WorkspaceShell::on_sql_console_event` (existing consumer) → the same downstream action a mouse click already triggers. No new event variants; no change to the consumer.

## Testing

New windowed integration binary `tests/sql_console_nav.rs` (feature `a11y-capture`, `#[serial]`), using the `AsyncHarness` tokio-reactor precedent from `ai_nav.rs` (required because `toggle_sql_console` → `refresh_completion_snapshot` `tokio::spawn`s). Helpers reused from `tests/support/mod.rs` (`A11ySnapshot::capture`, `focused_label`, `has_label`, `press_tab`) and `WorkspaceShell::open_console_ready_for_test`.

### T0 hard gate (load-bearing — 4 probes, STOP-clauses)

1. **Toolbar reach:** with the console open, Tab from the shell reaches `sql-run`; `focused_label()` == the Run label. Proves the per-button `focus_stop` + `.a11y` twin work across the shell→console boundary.
2. **Toolbar operate:** Enter on `sql-run` emits `SqlConsoleEvent::Run` (observed via `App::subscribe`, park-before-Enter ordering per `ai_nav` Probe 4).
3. **Tab-strip reach + switch:** seed 2 tabs; the tab strip is Tab-reachable; `→` advances `self.active` (0→1) and emits `Persist`.
4. **Tab-strip close:** `Delete` on the focused tab strip closes the active tab (tab count 2→1, `active` clamped).

STOP-clauses (report + halt if a probe's mechanism fails): P1 boundary-reach (mirror `ai_nav`'s console-open focus path); P2 emit ordering; P3 chained on_key_down not firing (verify the 2nd `.on_key_down` coexists, per recents-nav R1); P4 `close_tab` guard. If a probe fails structurally, do not build the suite on an unproven drive.

### Behavioral suite

- **Toolbar breadth:** each of the 7 buttons is Tab-reachable in paint order (`focused_label` matches each label as Tab advances).
- **Run/Cancel:** Enter on `sql-run` while idle emits `Run`; while `running`, the same control's label is Cancel and Enter emits `Cancel`.
- **Tab strip:** `→`/`←` switch `active` (+`Persist`) and clamp at the ends; `Delete` closes the active tab (clamped); `Delete` with a single tab is a no-op.
- **Closed-console negative:** with the console not visible, none of the new ids are tab stops (mirrors `ai_nav`'s closed-dock negative).

`#[cfg(feature = "a11y-capture")]` accessors on `SqlConsole` for read-back if not already public: `active_tab_for_test() -> usize`, `tab_count_for_test() -> usize`. Console-internal mutators (`new_tab`, `close_tab`, `set_running`) are already `pub`/`pub(crate)` — usable directly from the test.

**Canary:** `a11y_spike` expected ZERO frame-count drift — its scene keeps the console closed, so the new (unconditional) `.a11y` nodes in the console render never paint there (same result the AI-config slice observed). The controller `cargo test --workspace --no-fail-fast` gate is the backstop for any cross-binary drift.

## Constraints

- **Ships real production a11y** — `focus_stop`/`a11y` wiring is unconditional (not feature-gated), like Slice-6 / recents / catalog / AI-config. Only the `_for_test` read accessors are `#[cfg(feature = "a11y-capture")]`.
- **ZERO new dependencies** (`Cargo.toml`/`Cargo.lock`/`NOTICE` unchanged). D-015 stays open.
- Toolchain pinned 1.97.0; `cargo fmt --all` before every commit; DCO `-s` + `Co-Authored-By: Claude Opus 4.8 (1M context)` trailer.
- Test-only symbols before any `#[cfg(test)] mod tests`; `a11y` imports unconditional (used in both cfgs).

## Non-goals

- The editor `Input` and results `Table` keyboard models (defer; confirm their own tab-stop behavior in a future UAT).
- Transient overlay controls (error dismiss, history rows/close, NL-preview + explain stop buttons) — a possible carve-out #6.
- Any new `SqlConsoleEvent` variant or change to `on_sql_console_event`.
- Reordering tabs, or a roving "focused-but-not-active" tab index.

## Owed human glance

WCAG ≥3:1 focus-ring contrast on the 7 new toolbar buttons + the tab-strip container ring, both light and dark themes. Joins the standing ring-glance backlog (About/updates/Charts/Settings/Slice-6/recents/catalog/AI-dock).
