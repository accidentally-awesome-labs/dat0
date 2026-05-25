# dat0 P3b — UX polish + deferral closure — Design Spec

**Date:** 2026-05-25
**Phase:** P3b (second half of P3 Scratch mode + DataGrid; ~3 weeks)
**Entry:** P3a Scratch + DataGrid hot path merged to `main` (PR #4, merge `d2453dc`).
**Authoritative source:** `docs/specs/2026-04-26-dat0-design.md` §21.2 P3; `docs/specs/2026-05-14-dat0-p3a-scratch-datagrid-design.md` (P3a non-goals list); `docs/plans/2026-05-16-dat0-p3a-retro.md` Recommendations 1–6.
This document is the brainstorm-output spec consumed by the forthcoming P3b implementation plan.

---

## 1. Goal

Close spec §21.2 P3 by polishing UX on the stable hot path P3a delivered.
Wire the cross-thread bridge P3a deferred (PD-010). Mount the real
`gpui-component::Table` widget in place of P3a's placeholder. Refactor
`Banner` to the structured `{title, body, link, primary, secondary, kind}`
shape that recovery + future surfaces consume. Introduce a central
`ActionRegistry` and ship a command palette over it. Ship the import wizard
drawer with progress + cancel. Ship the empty-state hero with sample data.
Close deferrals D-001 (editable Profile + Theme widgets) and D-002 (theme
live-switch through running window).

**Non-goals (P3b):**
- Memory Budget Settings section → **D-014 (new)**, target P3c or P9c.
- `.sqlite` drag-drop → ATTACH + table picker — preserved as P3a Banner-rejection; ATTACH UX deferred to P5 alongside D-007.
- D-008 cancellation-token trait → stays P5; import cancel uses ad-hoc `Arc<AtomicBool>` + `engine.interrupt(handle)`.
- Non-UTF-8 encoding wizard control (D-010) → wizard does **not** expose encoding selector this phase; non-UTF-8 still rejected at sniff with Banner.
- Sparkle / AppImageUpdate bridges (D-003 / D-004) → P10.
- Linux Secret Service banner UX (D-005) → consumes new Banner shape when its phase arrives; not P3b work.
- macOS Intel CI (D-006) and self-hosted macOS runner (D-013) → TBD.
- sqlite_scanner static bundle (D-009) → TBD.
- MotherDuck ATTACH end-to-end (D-007) → P5.
- 1M-row scroll bench gated as merge requirement → stays P10 per spec line 819. P3b refreshes the baseline against the real Table widget; merge gate not introduced.

## 2. Phase decomposition (locked from 2026-05-14 brainstorm + 2026-05-25 brainstorm)

P3 was split into **P3a + P3b** during the 2026-05-14 brainstorm. P3a shipped
hot-path + perf-risk items. P3b ships UX polish + deferral closures
D-001 / D-002 + the PD-010 bridge that unblocks recovery actions and import
cancel.

This brainstorm confirmed the **single-phase, full-scope** option for P3b
(rather than splitting further or trimming). All ten carry-over surfaces ship
in one merge. Phase shape = **linear hot-path** (P3a pattern): one
sequential task chain, combined-verify default per the dev-workflow memo,
full review reserved for API-contact tasks.

## 3. Architecture

No engine changes. All new code lives in `dat0-app`. `dat0-engine` surface
stays P3a-stable.

### 3.1 `MainThreadDispatcher` (closes PD-010)

```rust
// crates/dat0-app/src/main_bridge.rs (new)
pub struct MainThreadDispatcher {
    tx: futures::channel::mpsc::Sender<Box<dyn FnOnce(&mut App) + Send>>,
}

impl MainThreadDispatcher {
    pub fn dispatch<F>(&self, f: F) -> Result<(), DispatchError>
    where F: FnOnce(&mut App) + Send + 'static;
}
```

The sender is captured **before `Application::run`**. A receiver loop is
registered inside a `cx.spawn` future at app init; each received closure
runs on the GPUI main thread via `cx.update(|cx, _| f(cx))` style invocation
(exact GPUI call site verified by T0 spike). Drop semantics: shutdown signal
on `Application::quit` drops the sender; the receiver loop exits cleanly when
the channel closes.

This is deferral D-010 option **(b)** (futures channel + posted closure).
Options (a) GCD-dispatch and (c) upstream gpui contribution are rejected:
(a) adds `block2` + `dispatch` FFI dependencies for a Cocoa-only path; (c)
is out of scope for a single-phase ship.

### 3.2 `Banner` refactor

P3a Banner is bare strings. P3b shape:

```rust
// crates/dat0-app/src/banner.rs (refactor existing)
pub struct Banner {
    pub title:     String,
    pub body:      String,
    pub link:      Option<BannerLink>,        // { label, url }
    pub primary:   Option<BannerAction>,      // { label, action_id }
    pub secondary: Option<BannerAction>,
    pub kind:      BannerKind,                // Info | Warning | Error (P1 shape)
}
```

Two-action capacity (primary + secondary) chosen over `Vec<Action>` to
constrain UX consistency. Single-optional rejected because recovery needs
both Open and Discard explicit. Future Linux Secret Service banner (D-005)
slots in via this shape: primary = "Setup guide" link, secondary = dismiss.

### 3.3 `ActionRegistry`

```rust
// crates/dat0-app/src/actions/registry.rs (new module)
pub struct ActionRegistry { inner: Arc<RwLock<HashMap<ActionId, ActionDescriptor>>> }

pub struct ActionDescriptor {
    pub id:         ActionId,
    pub title:      String,
    pub group:      ActionGroup,   // Navigation | Theme | File | Settings | Recovery
    pub keybinding: Option<Keystroke>,
    pub dispatch:   Arc<dyn Fn(&mut App) + Send + Sync>,
}
```

Central singleton, registered on app init. Features register their actions
through one path. Command palette enumerates by `iter()`; keybindings still
route through `cx.on_action` (the registry owns metadata + dispatch closure,
not the gpui-dispatch surface).

Closure thread-safety: `Send + Sync` bound enforced at compile time. Actions
whose handlers capture non-`Send` state (e.g., some `cx.open_window` paths)
route their dispatch through `MainThreadDispatcher::dispatch`, not directly.

Distributed `inventory`/`linkme` registration rejected (ordering
nondeterminism + untested with GPUI's main-thread model). JSON metadata
sidecar rejected (drift risk).

### 3.4 `Table` widget mount

`WorkspaceShell::render` (P3a placeholder div) is replaced with
`gpui_component::Table` bound to the existing `data_source: GridDataSource`
field. T0 spike verifies the widget surface at pinned commit
`0f0ab35...` and appends to `docs/internal/gpui-table-api-notes.md`.

### 3.5 Recovery panel

Banner emits "N previous sessions found" with primary action `REVIEW_RECOVERY`.
Clicking primary opens a `RecoveryPanel` listing each orphan with timestamp
+ per-row Open / Discard. Open spawns a new window via `MainThreadDispatcher`;
the orphan's `session.json` is replayed — registered file paths re-registered
into the new engine, then the tab list (table list + active tab) restored.
Scroll position and column widths are not persisted (P3a YAGNI; preserved
here). Discard deletes the orphan dir + removes its row; the banner updates
as the count drops.

Open spawns a **new** window (rejected: replace-current — confuses user if
current window has unsaved work). One-banner-per-orphan rejected (noisy on
many crashes); count + expandable list chosen.

### 3.6 Command palette

`gpui-component` fuzzy-list against `ActionRegistry::iter()`. Cmd-Shift-P
opens. Minimum 5 actions registered at ship: Open File, New Window, Toggle
Theme, Open Settings, Show Recents.

### 3.7 Import wizard drawer

Triggered **only** on ambiguous sniff. Ambiguity rule (refined by T9 against
the real `duckdb-rs` sniff API; T0 spike confirms the exact return shape):
either (a) more than one candidate delimiter scored within 5% of the top,
or (b) sniff returns a non-UTF-8 encoding marker, or (c) type-inference
flags any column with confidence below the sniff's reported threshold.
Confident sniff bypasses → tab opens directly. Drawer slides from the top
of the workspace shell; the grid below shows a live preview that re-runs
as the user toggles delimiter.

Always-show-wizard and "behind a setting" rejected: ambiguous-only matches
spec wording and optimizes the 95% path.

### 3.8 Import progress + cancel

Each import owns an `Arc<AtomicBool> cancel`. Drawer shows a progress
indicator. Cancel button → `MainThreadDispatcher::dispatch(|cx|
session.cancel_import())` → tokio task observes flag → calls
`engine.interrupt(handle)` (P2 surface) → drops handle → Banner "Import
cancelled".

D-008 trait introduction deferred (out of scope; stays P5).

### 3.9 Empty-state hero

Split layout: drop-zone column (~⅔ width, dashed border, "↓ Drop a file to
start" + "Open File…" + "Try sample data ▾") + recents column (~⅓ width).
Single layout for empty + non-empty (recents column populated when
non-empty). The recents column is the recents primary surface.

Centered hero (drop-zone only, recents swapped in template when present)
rejected: requires two render paths.

### 3.10 Sample data

```rust
// crates/dat0-app/src/sample_data.rs (new)
const IRIS_CSV: &[u8] = include_bytes!("../assets/iris.csv");       // ~5 KB
const CHINOOK_SQLITE: &[u8] = include_bytes!("../assets/chinook.sqlite"); // ~1 MB
const NYC_TAXI_URL: &str =
    "https://github.com/accidentally-awesome-labs/dat0/releases/download/sample-data-v1/nyc_taxi.parquet";
const NYC_TAXI_SHA256: &str = "<filled at release-asset upload>"; // T8 acceptance includes filling this; uploading the GitHub Release asset is a prerequisite for T8 merge.
```

Iris + chinook bundled via `include_bytes!`. NYC taxi fetched from GitHub
Release asset on first click; cached at `$STATE/samples/nyc_taxi.parquet`;
SHA256 checksum verified. Network failure → Banner with retry primary
action. Offline → row disabled with tooltip "requires network".

Bundle-all rejected (~55 MB binary growth across every install). Fetch-all
rejected (first-run feel + no offline samples at all).

`chinook.sqlite` byte slice is extracted to `$STATE/samples/chinook.sqlite`
on first click (DuckDB ATTACH needs a path, not a byte blob).

### 3.11 Settings widgets (closes D-001)

Profile section: editable `name` + `email` text inputs bound to
`SettingsStore::set("author.name", …)` / `"author.email"`.
Theme section: dropdown select bound to `SettingsStore::set("theme.id", …)`.
Memory Budget section absent (D-014 deferred — see §7).

Full `SettingsWidget` trait refactor rejected (speculative against future
sections that may not converge to one shape).

### 3.12 Theme live-switch (closes D-002)

`Theme` becomes a GPUI app-scoped global:

```rust
cx.set_global::<Theme>(Theme::load_builtin(&id));
```

Every theme-consuming view registers once at construction:

```rust
cx.observe_global::<Theme>(|view, cx| { cx.notify(); });
```

Settings dropdown change calls `cx.update_global::<Theme>(|t, _| *t = …)`;
all subscribed views re-render in the same tick. Cross-window propagation
is automatic because the global is `App`-scoped.

Per-window `Model<Theme>` rejected (cross-window drift). File-watcher
roundtrip rejected (hides bugs behind disk I/O).

## 4. Module touches

- `WorkspaceShell::render` — placeholder div → real `gpui_component::Table`.
- `Theme` consumer audit — every reader swaps to `cx.global::<Theme>()` +
  `cx.observe_global` once at construction.
- `crates/dat0-app/src/window.rs` `run_app` — capture
  `MainThreadDispatcher` sender before `Application::run`; UDS handler
  posts visual-spawn closures via the dispatcher.
- `SettingsPanel` — Profile + Theme sections gain editable widgets bound
  to `SettingsStore`.
- `crates/dat0-app/src/recovery.rs` — extends P3a `orphan_scan` with the
  `RecoveryPanel` view; banner emission becomes count-based.

Crate boundaries unchanged.

## 5. Task ordering (linear hot-path)

Each task is one PR-able commit; combined-verify default; full review
reserved for T0, T1, T3, T8, T9.

| #   | Task | Type |
|-----|------|------|
| T0  | Spike — verify `gpui-component::Table` + Drawer/Modal + `cx.set_global`/`observe_global` semantics at pinned commit `0f0ab35...`. Append to `docs/internal/gpui-table-api-notes.md` + `gpui-api-notes.md`. | full |
| T1  | `MainThreadDispatcher` + UDS handler rewire (closes PD-010). | full |
| T2  | Banner shape refactor → `{ title, body, link, primary, secondary, kind }`; migrate P3a call sites. | combined-verify |
| T3  | `ActionRegistry` + register `NewWindow` through it. | full |
| T4  | Mount real `gpui_component::Table` against `WorkspaceShell.data_source`. | combined-verify |
| T5  | Recovery panel — banner emits count, primary action opens panel with per-row Open / Discard. | combined-verify |
| T6  | Command palette (Cmd-Shift-P) + 5 actions. | combined-verify |
| T7  | Empty-state hero (split layout + sample data dropdown). | combined-verify |
| T8  | NYC taxi fetch + cache + progress indicator. | full |
| T9  | Import wizard drawer + ambiguous-sniff trigger + live preview. | full |
| T10 | Import progress + cancel wiring. | combined-verify |
| T11 | D-001 — editable Profile + Theme widgets. | combined-verify |
| T12 | D-002 — theme `cx.set_global` + `observe_global` audit. | combined-verify |
| T13 | Bench refresh against real Table + retro doc. | combined-verify |

Worktree: `.worktrees/p3b-ux-polish` on branch `p3b-ux-polish`, cut from
`main` post-P3a merge `d2453dc`. Remove worktree post-merge per project
convention.

**Pre-T0 blockers** (P3a retro Lesson 1): `xcrun metal --version` succeeds;
`cargo clippy --workspace -D warnings` clean on `main`.

## 6. Exit criteria (maps spec §21.2 P3)

| # | Criterion | Test surface |
|---|---|---|
| 1 | Drop 100 MB CSV → table appears, scrolls smoothly. | Manual UAT; `tests/file_drop_formats.rs` asserts real `Table` widget type-id (not placeholder). Closes P3a partial #1. |
| 2 | Open second window → independent scratch + engine; theme applies app-globally. | `tests/multi_window.rs` extended — assert independent Sessions; theme change in window 1 reflects in window 2 (per §3.12 global Theme — "independent theme" in spec line 952 is satisfied by consistent app-global application, not by per-window divergence). |
| 3 | Theme change reflects in grid immediately. | `tests/theme_live_switch.rs` (new) — drive dropdown change, assert `observe_global` callback fires + view re-renders. |
| 4 | Cmd-Shift-P opens palette; ≥5 actions registered. | `tests/command_palette.rs` (new) — `ActionRegistry::iter().count() >= 5`; integration test triggers Cmd-Shift-P + selects "New Window" + asserts window count incremented. |
| 5 | Import wizard prompts on ambiguous CSV. | `tests/import_wizard.rs` (new) — two-fixture (confident comma, ambiguous tab/comma); wizard opens only on the second. |
| 6 | Long import shows progress; cancel aborts cleanly. | `tests/import_cancel.rs` (new) — 100 MB fixture, set cancel flag, assert `engine.interrupt` called + no partial table + Banner emitted. |
| 7 | 1M-row scroll bench produces p99 frame-time output on macOS CI. | T13 reruns `grid_scroll` against real Table; CI artifact step unchanged (no merge gate; spec line 819 / P10 enforces). |
| 8 | Force-quit during import → next launch surfaces recovery notice; data accessible. | `tests/scratch_lifecycle.rs` extended — multi-orphan + Review expands + Open spawns window with restored tabs. Closes P3a partial #4. |
| 9 | Fresh launch with no recents shows empty-state hero + sample-data buttons + Open File. | `tests/empty_state.rs` (new) — hero spawns iff `Recents::is_empty()`; button clicks dispatch correct actions; sample dropdown shows three entries. |
| 10 | P3a exit #5 — second `dat0` launch from terminal opens a visual window in the running instance. | `tests/single_instance.rs` extended — assert window count = 2 after second-launch UDS forwarding (closes PD-010). |
| 11 | All P2 + P3a exit criteria still pass. | `cargo test --workspace` green. |

**Non-test exit gates:**
- `docs/deferrals.md`: D-001, D-002, PD-010 closed; **D-014 (Memory Budget Settings) opened**.
- `docs/internal/p3-bench-baselines.md` records new p99 frame-time for the real Table widget (renamed from `p3a-bench-baselines.md`).
- `.github/workflows/ci.yml` unchanged from P3a (no new jobs).

## 7. Out-of-scope + new deferrals

| Item | Why deferred | Target |
|------|------|------|
| Memory Budget Settings section | D-001 wording scopes only Profile + Theme; engine plumbing to re-apply `memory_limit` PRAGMA on change is non-trivial. | **D-014 (new)** — P3c or P9c |
| `.sqlite` drag-drop → ATTACH + table picker | UX surface not in spec §P3. P3a Banner-rejects; behavior preserved. | P5 (alongside D-007) |
| D-008 CancellationToken trait | Import cancel uses ad-hoc `Arc<AtomicBool>` + `engine.interrupt(handle)`. | Stays P5 |
| Non-UTF-8 encoding wizard control (D-010) | Wizard does not expose encoding selector. Sniff still rejects with Banner. | TBD |
| Sparkle / AppImageUpdate bridges (D-003 / D-004) | Notarized pipeline not ready. | P10 |
| Linux Secret Service banner (D-005) | Will consume new Banner shape when its phase arrives. | TBD |
| macOS Intel CI (D-006), self-hosted macOS runner (D-013) | Gated on runner capacity / Mac mini purchase. | TBD |
| sqlite_scanner static bundle (D-009) | No upstream feature change. | TBD |
| MotherDuck ATTACH end-to-end (D-007) | Untouched. | P5 |

**D-014 entry to file at T0:**

> **D-014 — Memory Budget Settings section**
> - **Status:** open
> - **Deferred from:** P3b (T11 scope decision)
> - **Target phase:** P3c (if split) or P9c (settings polish)
> - **Reason:** P3b T11 scope locks to D-001 wording (Profile + Theme widgets). Memory Budget requires engine plumbing to re-apply `memory_limit` PRAGMA on change, not in P3b ad-hoc scope.
> - **What P3b ships:** Profile + Theme editable sections.
> - **What target phase delivers:** Slider/number input for `memory_limit`; engine reapplies on change (or notes "applies next window").
> - **Originating doc:** `docs/specs/2026-05-25-dat0-p3b-ux-polish-design.md` §7.
> - **Last touched:** 2026-05-25.

**Deferrals closed by P3b at retro:**
- **D-001** — editable Profile + Theme widgets (T11).
- **D-002** — theme live-switch (T12).
- **PD-010** — UDS→GPUI cross-thread bridge via `MainThreadDispatcher` (T1).

## 8. Risks

1. **gpui-component Drawer/Modal + fuzzy-list surface unverified at pinned `0f0ab35`.** T0 must confirm usable Drawer/Modal (T9 wizard) and fuzzy-list (T6 palette) primitives exist; fallbacks = hand-rolled `div` overlay (drawer) + hand-rolled `Vec<String>` scoring with `cx.dispatch_action` (palette). Severity: medium.
2. **`MainThreadDispatcher` lifetime + drop semantics.** Sender held by tokio UDS task; receiver loop in `cx.spawn` future. Shutdown signal on `Application::quit` drops the sender; receiver exits cleanly. T1 wires the shutdown handshake + documents in module doc-comment. Severity: medium.
3. **`cx.set_global::<Theme>` cross-window propagation untested in this codebase.** T0 spike verifies by spawning two windows + mutating global + asserting both rerender. Severity: low.
4. **NYC taxi fetch URL stability.** Depends on a GitHub Release asset uploaded before T8 ships. URL + SHA256 stored in single consts; checksum-verified download; Banner with retry on fetch failure; offline disables the row. Severity: low.
5. **Bench p99 regression on real Table widget.** P3a's bench measured `render_cell` dispatch alone (~14.7 µs / iter ≈ 67 kfps). P3b expects 3–5× slowdown from text shaping + GPU upload (per P3a retro Rec #5), still inside 60 fps budget but unverified. T13 records new baseline; if p99 ≥ 16.67 ms (60 fps floor), file as P10-gate concern. Severity: low (no merge gate this phase).
6. **Action dispatch closure thread-safety.** `ActionDescriptor::dispatch: Arc<dyn Fn(&mut App) + Send + Sync>` — some `cx.open_window` paths capture non-`Send` state. T3 verifies against `NewWindow`; non-`Send` actions route through `MainThreadDispatcher`. Severity: medium.
7. **`include_bytes!(chinook.sqlite)` portability.** ATTACH needs a path. T7 extracts the byte slice to `$STATE/samples/chinook.sqlite` on first click. Severity: low.
8. **Plan-vs-toolchain drift recurrence** (P3a Lesson 3). Every plan snippet referencing `gpui-component`, `duckdb-rs`, `interprocess`, `fs4`, `futures::channel::mpsc` must be cross-checked against real source at author-time. Severity: low if discipline holds.

## 9. Testing strategy

No engine work this phase → engine test count stays at 44.

**New integration test files** (one per task as appropriate):

| File | Task | Coverage |
|------|------|------|
| `tests/main_thread_dispatcher.rs` | T1 | Sender→receiver round-trip; closure runs on main thread (assert via `thread::current().name()`); shutdown drops cleanly. |
| `tests/banner_shape.rs` | T2 | Construction + serialization round-trip for `{ title, body, link, primary, secondary, kind }`; primary-only, secondary-only, both, neither. |
| `tests/action_registry.rs` | T3 | Register + iter + dispatch; duplicate ID rejected; idempotent register. |
| `tests/theme_live_switch.rs` | T12 | Mutate `cx.global::<Theme>()`; assert `observe_global` callbacks fire on subscribed views; cross-window propagation. |
| `tests/command_palette.rs` | T6 | `ActionRegistry::iter().count() >= 5`; Cmd-Shift-P opens palette; "New Window" selection increments window count. |
| `tests/empty_state.rs` | T7 | Hero spawns iff `Recents::is_empty()`; "Open File…" dispatches `OpenFile`; sample dropdown shows three entries; Iris click loads bundled CSV. |
| `tests/sample_data_fetch.rs` | T8 | Network mock; successful fetch caches at `$STATE/samples/`; checksum verifies; offline → Banner with retry link. |
| `tests/import_wizard.rs` | T9 | Two-fixture (confident comma, ambiguous tab/comma); wizard opens only on the second. |
| `tests/import_cancel.rs` | T10 | 100 MB fixture via `dat0-fixtures`; kick import via dispatcher; set cancel; assert engine interrupt + no partial table + Banner. |
| `tests/recovery_panel.rs` (extends `scratch_lifecycle.rs`) | T5 | Multi-orphan: spawn 3 orphans, banner shows N=3, Review expands list, Open spawns window with restored tabs, Discard removes dir. |

**Existing tests extended:**
- `tests/single_instance.rs` (T1) — assert window count = 2 after second-launch forwarding.
- `tests/multi_window.rs` (T12) — theme change in window 1 → window 2 re-renders.
- `tests/file_drop_formats.rs` (T4) — assert dropped file mounts on real `Table` (type-id), not placeholder.

**Unit tests** added inline to `banner.rs`, `actions/registry.rs`, `sample_data.rs`.

**Manual UAT** runbook documented in retro:
1. Launch with no recents → empty-state hero appears (split layout).
2. "Try sample data ▾" → dropdown lists three entries; Iris click populates grid.
3. NYC taxi click → fetch progresses → grid populates → scrolls smoothly.
4. Settings → Theme dropdown change → grid + chrome re-render without restart.
5. `kill -9` mid-import in window 2 → relaunch → banner "1 previous session" → Review → Open → window 3 has restored tab.
6. `dat0 ./big.csv` from terminal while app running → new window opens with that file (PD-010 path).
7. Cmd-Shift-P → palette opens → type "new" → "New Window" highlighted → Enter → window count++.
8. Drop ambiguous CSV (tab/comma equally plausible) → wizard drawer opens → toggle delimiter → preview live-updates → Import → grid populates.
9. Drop 1 GB CSV → drawer progress → Cancel → import aborts cleanly → Banner.
10. Drop `.sqlite` → still rejected with Banner (P3a behavior preserved).

**Bench:** T13 re-runs `grid_scroll` against real Table; macOS CI artifact step unchanged.

## 10. Pre-merge gates

- `cargo fmt --all -- --check` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo test --workspace` green (workspace test count grows by ~30 across new integration files).
- `.github/workflows/ci.yml` matrix passes (macos-arm64 hosted + linux-x86_64 runnerkit).
- Bench artifact uploaded on macOS CI (no merge gate per spec line 819).
- `docs/deferrals.md` updated: D-001, D-002, PD-010 closed; D-014 opened.
- `docs/internal/p3-bench-baselines.md` records new real-Table p99.
- NOTICE.md regenerated if new deps added (cargo-about gate is warn-only PD-003).

## 11. Open questions / follow-ups

None. All architectural choices were settled in the 2026-05-25 brainstorm
session:

- Phase scope: single phase, full carry-over (10 surfaces).
- Phase shape: linear hot-path (P3a pattern).
- PD-010 mechanism: futures::channel::mpsc + posted closure (option (b)).
- Banner capacity: primary + secondary (two-action).
- Action registry: central singleton over hand-rolled HashMap.
- Wizard trigger: ambiguous-only (auto-import on confident sniff).
- Wizard layout: inline drawer + live preview.
- Empty-state layout: split (drop-zone + recents column).
- Sample data delivery: bundle small (Iris + chinook), fetch large (NYC taxi).
- Theme channel: `cx.set_global::<Theme>()` + `cx.observe_global`.
- Settings scope: D-001 only (Profile + Theme); Memory Budget → D-014.
- Import cancel: ad-hoc `Arc<AtomicBool>` + `engine.interrupt(handle)`.
- Recovery banner: count-with-expandable-list; Open spawns new window.

---

**Brainstorm session ID:** 0859b8b2-36b0-42d7-ad61-9713a9c07de9 (2026-05-25)
**Author:** Salar Sayyad (brainstorm with Claude)
