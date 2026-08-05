# Slice B9 — dock layout persistence (session v10→v11, settings v2→v3)

**Date:** 2026-08-04
**Branch:** `feat/ui-redesign-b9-dock-layout` off main `1e33559` (B8)
**Master plan:** `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6 row B9
**Predecessors:** B5 `cda2974` (DockArea skeleton) · B6 `921dde8` (right dock) · B7 `9ceff53` (left dock + rail) · B8 `1e33559` (bottom dock)

---

## 1. Goal and scope

Make the dock layout survive: which docks are open, which left-rail panel is
showing, and how big each dock is. Restore it on the next launch.

**In scope**

- Left dock: which panel (catalog / connections / AI) or none, plus the dock's
  outer width.
- Right dock: inspector visible, charts visible, plus the dock's outer width.
- Bottom dock: console open, plus its height (B8 shipped 320 px, unpersisted —
  B8's own note assigns it to B9).
- Session schema v10 → v11; settings schema v2 → v3.

**Explicitly NOT in scope** — each of these is a decision, not an omission:

| Non-goal | Why |
|---|---|
| Calling `DockArea::load` | §3.1 — it always rebuilds the center and re-wraps the grid in a `TabPanel`. |
| Persisting the center | B5's standing constraint; §3.1 makes it structurally impossible here. |
| Inner split sizes (inspector 288 vs charts 560) | Owner-chosen scope. Restores to constants. |
| Drag-rearranged layouts | `set_locked(true)` since B5; there is nothing to persist. |
| Fixing the 7 degraded panel builders | §7 — they are only reachable from `DockArea::load`, which this slice does not call. |

---

## 2. Verified upstream facts (pinned rev `0f0ab35`, gpui `0.2.2`)

Every claim below was read from source during design, not assumed. The four
marked ⚠ each changed the design.

1. ⚠⚠⚠ **`DockAreaState.center` is a non-`Option` field and `DockArea::load`
   unconditionally does `self.items = state.center.to_item(weak_self, window, cx)`**
   (`dock/mod.rs:898-921`). There is no partial-load API. Combined with
   `PanelState::to_item`'s `PanelInfo::Panel(_) => DockItem::tabs(vec![view], ..)`
   (`dock/state.rs:227-236`) — B5's finding — **any** call to `load` re-wraps the
   grid in a `TabPanel`, restoring the 30 px title bar, the nested
   `overflow_y_scroll`, the `.cached()` child and a `.tab_group()` that B5 chose
   `DockItem::Panel` specifically to avoid.
2. ⚠⚠⚠ **`Dock` is not an `EventEmitter` at all** — `dock/dock.rs` contains zero
   `cx.emit`. `Dock::resize` (`:305`) and `Dock::set_open` (`:259`) only
   `cx.notify()`. **`DockEvent::LayoutChanged` never fires for an outer dock
   resize or an open/close**; its only sources are `StackPanel` (inner split
   resize, panel insert/remove) and `TabPanel` (tab churn). The master plan's
   save trigger — "`DockEvent::LayoutChanged` → ~500 ms debounce → `dump`" —
   therefore cannot see either of the two things B9 exists to persist.
3. ⚠⚠ **`DockArea` keeps `left_dock` / `right_dock` / `bottom_dock` private with
   no getter**, and `DockState`'s four fields (`panel`, `placement`, `size`,
   `open`) are private too (`dock/state.rs:25-31`). `Dock::size()` and
   `Dock::set_size()` are `pub` but unreachable from dat0. ⇒ **`DockArea::dump(cx)`
   is the only public read path for a dock's size**, and the value comes out
   only through serialization.
4. `Pixels` is `#[repr(transparent)]` over `f32` with derived
   `Serialize`/`Deserialize` (`gpui-0.2.2/src/geometry.rs:2565-2573`) ⇒ it
   serializes as a bare number, so `serde_json::to_value(&dump)` yields
   `{"left_dock": {"size": 384.0, "open": true, ...}}` and a dat0 mirror struct
   can `from_value` it.
5. ⚠ **`set_locked(true)` does NOT disable resizing.** `is_locked()` is consulted
   in exactly one place, `tab_panel.rs:384` (closable / drag). `Dock::render`
   calls `render_resize_handle` unconditionally (`dock/dock.rs:396`). The docks
   are user-resizable today — which is what makes size worth persisting.
6. `set_left_dock` / `set_right_dock` / `set_bottom_dock` already take
   `Option<Pixels>` size **and** an `open: bool` (`dock/mod.rs:603-669`). The
   restore path needs no new upstream API.
7. `DockArea::is_dock_open(placement, cx)` is public (`dock/mod.rs:691`) — open
   state is readable cheaply, without a `dump`.

---

## 3. Architecture

### 3.1 Why there is no `DockArea::load`

Facts 1 and 3 together decide the whole slice. Upstream's round-trip is
all-or-nothing: `dump` gives a full tree, `load` restores a full tree including
a center it will always rebuild through `to_item`. dat0 cannot take the half it
wants.

It also does not need to. dat0's dock *structure* is fixed and built by
`ensure_dock_area` (window.rs:6607): a `DockItem::Panel` center, a right split of
two single-panel tabs, a left split of three, a bottom single tab. Nothing the
user can do changes that structure — `set_locked(true)` since B5, no drag, no
tab moves. **The only user-mutable layout state is, per dock, `(open, size)`.**

So B9 uses `dump` as a *read instrument* and the existing `set_*_dock` calls as
the restore path. This sidesteps, in one decision: the center clobber, the
transient panel construction `to_item` would perform, the re-entrancy hazard of
building panels from inside `WorkspaceShell::render` (B7's panic class), and a
second subscription-leak site (B6).

### 3.2 Data model

New module `src/session/dock_layout.rs`, sibling to the existing
`session/charts.rs` and `session/queries.rs`.

```rust
/// Persisted dock layout. Docks ONLY — never the center (see §3.1).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DockLayout {
    /// Which left-rail panel is showing. `None` = the left dock is closed.
    #[serde(default)]
    pub left_panel: Option<LeftPanel>,
    /// User-resized width. `None` = use the mount constant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_size: Option<u32>,

    #[serde(default)]
    pub inspector_visible: bool,
    #[serde(default)]
    pub charts_visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_size: Option<u32>,

    #[serde(default)]
    pub console_open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom_size: Option<u32>,
}
```

⚠ **Amended while planning — sizes are `u32`, not `f32`, and every `Option`
carries `skip_serializing_if`.** `Settings` derives `Eq`
(`settings/schema.rs:16`), so an `f32` field would break that derive for every
existing consumer; and the `toml` serializer errors with `UnsupportedNone` on a
`None` inside a table, so the settings half would fail to write without the skip
attribute. The narrower type also means NaN and infinity become
*unrepresentable* on the wire rather than merely rejected — §3.6's "reject
non-finite" rule collapses into the capture-side conversion instead of being a
validation step someone could forget to call.

Two shape decisions worth stating:

- **No per-dock `open` flag.** Left-open ≡ `left_panel.is_some()`; right-open ≡
  `inspector_visible || charts_visible`; bottom-open ≡ `console_open`. B7's
  at-most-one-left-panel invariant is therefore **unrepresentable-if-violated on
  the wire**, where three parallel bools could contradict it. This mirrors the
  master plan's own standing rule for B6+: derive dock state, never keep
  parallel bools.
- **`Option<u32>` sizes.** `None` means "never resized, use the constant", so an
  untouched dock keeps inheriting `LEFT_DOCK_WIDTH` (384) /
  `INSPECTOR_DOCK_WIDTH + CHARTS_DOCK_WIDTH` (848) / `SQL_CONSOLE_DOCK_HEIGHT`
  (320) if those ever change. A `0.0`-means-default sentinel would not.

`LeftPanel` (window.rs:447) gains `Serialize, Deserialize` with
`#[serde(rename_all = "snake_case")]` → `"catalog" | "connections" | "ai"` on the
wire. It is a plain field-less enum with no gpui dependency, so the session
module can reference it without a layering problem, and reusing it avoids a
second source of truth for panel identity.

### 3.3 Two homes, one precedence rule

A plain launch calls `Session::new` (window.rs:1177) → a **fresh scratch dir with
an empty session.json**. A session's persisted state returns only via
`Session::recover` (orphan recovery) or `Session::recover_workspace`. A
session-only `dock_layout` would therefore never come back on the ordinary
launch path.

So the layout is written to two places:

| Home | Field | Role |
|---|---|---|
| `session.json` (v11) | `dock_layout: Option<DockLayout>` | Authoritative when present. A workspace carries its own layout. |
| `settings.toml` (v3) | `[ui] dock_layout` | The seed a fresh scratch window starts from. Last write wins across windows. |

**Precedence, applied once at shell construction:** session value if present →
else settings seed → else today's constants. Both are written by the same
capture path (§4), so they cannot drift while dat0 is running; they diverge only
when a workspace is re-opened after the user changed the layout elsewhere, which
is the intended behaviour.

### 3.4 Capture

```
persist_dock_layout(&self, cx: &App):
    dock_area? -> read(cx).dump(cx)            // fact 3: only public route
      -> serde_json::to_value                  // fact 4: Pixels -> bare number
      -> from_value::<DumpMirror>              // dat0-local {left,right,bottom}{size,open}
      -> DockLayout { .. }                     // + shell bools already known
      -> session.set_dock_layout(..)  AND  settings seed write
```

`DumpMirror` is a dat0-side `#[derive(Deserialize)]` shape reading only
`{"size": f32, "open": bool}` per dock — everything else in the dump (the whole
`center`, every `PanelState`) is ignored by construction. This is the second
place the "docks only, never the center" rule is made structural rather than
remembered.

**Triggers — and the two homes are written at different rates:**

| Site | Today | Writes session | Writes settings seed |
|---|---|---|---|
| window.rs:3414 | `persist_dock_ui()` (console / dock toggle) | yes | no |
| window.rs:7040 | `persist_dock_ui()` (`activate_left_panel`) | yes | no |
| window.rs:7755 | `persist_dock_ui()` (test/menu path) | yes | no |
| window.rs:1653 | `flush_focused_workspace_sql`, already wired to Quit + CloseWindow | yes | **yes** |

Open/close needs no upstream event: every open and close in dat0 goes through
dat0's own toggles. Only *size* is a pure upstream mouse drag with no dat0 code
in the loop, and the close backstop is what captures a resize that is not
followed by any toggle. **Known miss: resize → hard crash with neither a toggle
nor a clean close.** Accepted; the alternative was a per-window forever-timer or
a `dump()` on every frame.

⚠ **The settings seed is written ONLY on the close/quit backstop, never on a
toggle.** `SettingsWatcher` watches settings.toml for `Modify`/`Create` and
re-reads the file on every write (`settings/watcher.rs:20-26`), swapping the
in-memory `Settings` under an `RwLock` (`boot.rs:68-70`). That callback is
benign — it does not re-apply the theme or refresh windows — but settings.toml is
today written only on deliberate user action, and writing it on every dock toggle
would turn it into a file written dozens of times a session, widening the window
in which a load-modify-save clobbers a hand-edit in flight. The seed only has to
be correct at quit, so that is when it is written. Session state, which is
per-window and already written on every toggle, keeps its existing rate.

The settings write follows the established writer discipline — a typed
`SettingsStore` method doing load → mutate → atomic `save`, the same shape as
`SettingsStore::set` (`store.rs:170`), which is how `settings_ui/panel.rs:261`
already writes `theme.id`. The KV setter itself takes `&str` and cannot carry a
struct, so B9 adds a typed sibling rather than abusing it.

### 3.5 Restore

1. `WorkspaceShell::new` resolves the layout by §3.3 precedence and seeds the
   existing shell fields — extending the `ui.catalog_panel_visible` /
   `ui.inspector_panel_visible` reads already at window.rs:2575 and :2581 — plus
   `chart_panel_visible`, `connections_panel_visible`, `ai_panel_visible`, and
   the three sizes.
2. `ensure_dock_area` (window.rs:6607) passes the resolved sizes into the
   `set_*_dock(item, Some(px(size)), open, ..)` calls it already makes. Those
   calls stay **exactly once each** — B6's subscription-leak constraint is
   untouched, since sizes are decided at mount and never re-set.
3. Bottom dock: when `console_open`, mount it eagerly at first render instead of
   waiting for the first `toggle_sql_console`. B8's derived-visibility model
   (`sql_console_visible` ≡ `is_dock_open(Bottom)`) is unchanged; only the mount
   moment moves, and only when a persisted layout says the console was open.
   The first-run hero has no persisted layout, so the hero still mounts no
   console and `a11y_spike` stays at **12**.

### 3.6 Failure modes

| Failure | Behaviour |
|---|---|
| Malformed `dock_layout` (hand-edit, truncated write, future dat0) | Tolerant **field-level** deserializer → `None` → default layout. Tabs, SQL tabs, history, saved queries, charts and attachments still load. Layout is the least valuable thing in the file and must never cost a user their work. |
| Non-finite size (NaN, inf) | Rejected → treated as `None` → mount constant. |
| Absurd size (20000 px from a larger display; 3 px) | Clamped to **`[100.0, 0.8 × the window's extent on that axis]`** before it reaches `set_*_dock` — width for left/right, height for bottom. The lower bound is upstream's own `PANEL_MIN_SIZE` (`resizable/mod.rs:14` = `px(100.)`), restated as a dat0 constant so the clamp is testable without reaching into a `pub(crate)` item. The upper bound guarantees the center always keeps a fifth of the axis, so a restored window is always operable and always has a visible resize handle to recover with. A size saved on a bigger screen legitimately differs on restore. |
| `dock_layout` absent (v10 file, or first run) | `#[serde(default)]` → `None` → §3.3 precedence falls through. |

⚠ The tolerant deserializer needs a buffered value type and therefore has **two
implementations, not one**: `serde_json::Value` for session.json and
`toml::Value` for settings.toml. Serde cannot express format-agnostic
error-recovery without such a buffer. Both are thin and both are unit-tested.

### 3.7 Migration v10 → v11

`SessionUiState` **loses** `catalog_panel_visible` and `inspector_panel_visible`
(they move into `DockLayout`); it keeps `catalog_collapsed`, which is catalog
tree state, not dock layout. `SessionState` gains
`dock_layout: Option<DockLayout>`.

`migrate.rs` gains `10 => migrate_v10_to_v11(raw)`, and the current-version arm
(with its forward-incompat transform-`kind` pre-check) moves to `11`. The
literal-arm discipline documented at `migrate.rs:186-191` is preserved.

⚠⚠ **The one place this differs from the v9→v10 reshape it otherwise copies:**
v9→v10 could *discard* the old keys because production only ever wrote them at
their empty defaults. Here production wrote **real** values — a user with the
catalog dock open has `catalog_panel_visible: true` on disk right now. So
`migrate_v10_to_v11` must read both flags from the **raw `serde_json::Value`**
and carry them into `dock_layout`. Reading them off the parsed `SessionState` is
impossible: the v11 struct no longer has those fields and serde drops the keys
silently, with no error — the failure would be an invisible one-time layout
reset for every existing user.

Settings v2→v3 is purely additive (`[ui]` absent → default), so it needs no
value carry-over.

---

## 4. T0 hard gate

Per the standing series practice, T0 runs before any production edit and each
probe has a stop clause. The gate lives in a spike test file, not in
`window.rs` — B8's finding that an in-production probe reddens unrelated
`a11y_content` tests as an artifact and makes its own measurements unreadable.

| # | Probe | Stop clause |
|---|---|---|
| 1 | `dump()` reads the **live** `Dock`, not a construction-time copy: `set_left_dock(.., Some(px(N)), ..)` at mount, then `dump()` reports `N` — for `N` both above and below the default, and for all three placements. | If `dump` echoes a stale or default value, size persistence is not implementable at this rev → drop to open-flags-only and record it. |
| 2 | `serde_json::to_value(dump)` really yields `{"left_dock":{"size":<number>,"open":<bool>}}`, and `DumpMirror::from_value` succeeds against a real dump. | If `Pixels` does not serialize transparently, the mirror shape changes (fact 4 says it will). |
| 3 | Eager bottom-dock mount at first render does not re-trip B7's `set_active_ix` → `shell.read` panic, and leaves `a11y_spike` at **12** on the hero. | If it panics, console restore is deferred to the first frame after mount, or dropped. |

Probe 3 is the top risk: it is the only behaviour change on a path B8
deliberately made lazy, and the B7 panic it probes for is a construction-time
re-entrancy that no amount of reading can rule out.

⚠ **What T0 deliberately does NOT prove, and why.** A real mouse-drag resize
cannot be driven headlessly at this rev: `Dock::resize` is reachable only through
`resize_handle`'s drag (`dock/dock.rs:291-305`), and dat0 cannot even obtain the
`Entity<Dock>` to call the public `set_size` (fact 3). So no probe here proves
"the user drags, and the new size is persisted" end to end. What covers that gap
instead:

- **Structurally:** `Dock::resize` mutates the same `self.size` field that
  `DockState::new` reads when building the dump (`dock/state.rs:34-43`). Probe 1
  proves the dump reflects that field's live value; the drag writes that field.
  There is no third state for the two paths to disagree about.
- **By hand:** the owed human glance (§9) includes restoring a console at a
  *resized* height, which is the end-to-end check no test can make.

Writing a probe that pretends to drive a drag would be worse than admitting the
gap — B7 recorded two consecutive probes that "passed" while measuring nothing,
and the more convincing one was the more wrong.

---

## 5. Tests

**New binary `tests/dock_layout_persist.rs`** — the windowed round-trip:
open a shell over a real session dir, change layout (activate a rail panel,
toggle inspector, open the console), persist, re-open a shell over the same
session, assert the layout came back. Plus the negative: a shell over a session
with no `dock_layout` gets the constants.

**Unit tests, in-module** (no new binaries):

- `session/migrate.rs` — v10 fixture with `catalog_panel_visible: true` +
  `inspector_panel_visible: true` migrates to v11 with both carried into
  `dock_layout`; a v10 fixture with neither set migrates to a default layout;
  v11 loads as-is.
- `session/dock_layout.rs` — clamping (non-finite rejected, out-of-band clamped,
  in-band untouched); the derived-open rules.
- `session/mod.rs` — tolerant parse: a `dock_layout` of the wrong JSON type
  leaves tabs/sql_tabs/charts intact and the layout `None`.
- `settings/schema.rs` — v2 → v3 additive default; tolerant `[ui]` parse.

**Non-vacuity** is mandatory per the series' standing rule, and B8's lesson
applies directly: perturb the **mechanism**, not the expectation. For the
round-trip that means reverting the restore wiring and confirming the test goes
red — not merely renaming a field.

---

## 6. Local gate

Unchanged from B5–B8: `cargo fmt --all --check`;
`cargo clippy --workspace --all-targets -D warnings`; `-p dat0-app` across
{plain, `a11y-capture`, `a11y-capture,gallery`} — **116 binaries as of B8**,
expected **117** here; `style_lint` ratchet UNCHANGED at `[("window.rs", 1)]`;
`src/grid` byte-identical; `cargo build -p dat0-app --bin dat0` **and boot it**,
diffing the log against a `main` build (this is how B5's tour regression was
found). `cargo test --workspace` and `cargo bench` remain unrunnable on this
machine (macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift, reproduces on `main`).

⚠ `src/session` is **no longer** byte-identical — B5–B8 all asserted that, and
B9 is the slice that deliberately ends it. The substitute check is that the
session diff is additive plus the one documented `SessionUiState` reshape.

---

## 7. The seven degraded panel builders

`register_panels` (`src/panels/mod.rs`) registers all seven panels with builders
that hand back a shell-less panel (`WeakEntity::new_invalid()`, or for the
console a tabs-less `SqlConsole`). Their doc comments currently promise that
"B9 replaces all seven with builders that resolve the live shell".

That promise was premised on B9 calling `DockArea::load`. It does not (§3.1), and
`PanelRegistry::build_panel` is reachable from nowhere else — so the builders
stay unreachable, and making them resolve the live shell would add untestable
code whose only call path runs from inside `WorkspaceShell::render`, where the
shell is leased (B7's panic class).

B9 therefore **keeps the registration and rewrites the doc comments** to state
plainly that nothing calls them, why B9 chose not to, and what a future
drag-rearrange slice would have to do first. Registration is what makes
`panel_name` meaningful and keeps that future slice cheap; deleting it would
undo a B5–B8 investment to save nothing.

---

## 8. Risks

| Risk | Mitigation |
|---|---|
| Eager console mount re-trips B7's construction-time re-entrancy panic | T0 probe 4, with an explicit fallback |
| `a11y_spike`'s exact-12 moves | T0 probe 4 asserts it on the hero; if a restored-open dock moves it, bump deliberately with a comment naming the nodes, never loosen to `>=` |
| The console is the app's biggest focus surface (`sql_console_nav` 9 + `sql_console_transient_nav` 17 + `input_nav` 7) | Eager mount changes *when* it mounts, not its structure; B8's `root_focus` law is untouched. Treat any tab-order change as a real regression, not a test to update |
| Two schemas in one slice | They are independent files with independent version ledgers; the settings half is purely additive |
| A settings write races a hand-edit, or churns the file | Seed written once per window close, not per toggle (§3.4); the watcher callback is read-only into an `RwLock` and re-applies nothing |
| Restored layout differs from saved on a smaller display | Intended — §3.6 clamping. Called out for the human glance |
| No test can prove the drag → persist path end to end | Acknowledged in §4 with the structural argument that covers it and the human glance that checks it |

---

## 9. Owed human glance

A restored window in all 3 themes, HC most of all: a restored left panel + rail
selection agreeing; both right-dock panels restored; the console restored open at
a **resized** height; a layout saved on a large display restored on a small
window (the clamping path); and a first run, which must still show the hero with
no console and no open docks.

Carried forward and still owed: B8's double-chrome / narrow-window pass, B7's
rail pass, B6's title bars, B5's diff-the-pixels + file-drop, B4's palette.

---

## 14. T0 as-built (2026-08-04)

**All three probes passed on the first run. No stop clause fired, and the
design is unchanged by this gate.** `tests/dock_layout_spike.rs`, 3 tests,
kept as standing regression guards (the `dock_chrome_spike.rs` precedent).
`tests/a11y_spike.rs` re-run alongside: still green, still asserting **12**.

### Probe 1 — `dump()` reads the live `Dock` ✅

Rewritten from the plan's version, which would have measured nothing useful.
The plan proposed re-mounting a dock at a probe size and reading it back;
instead the probe asserts all **three** mount sizes at once — 384 left / 848
right / 320 bottom. Those constants are distinct, and **one shared constant
cannot produce three different numbers**, so reading all three back proves the
dump resolves each dock separately from live state. A single-value probe would
have passed just as happily against a hardcoded echo.

### Probe 2 — the serialized shape ✅

Confirmed exactly as predicted from the source read (design §2 facts 3-4). The
real payload, captured live and now the fixture for the capture-path parser:

```json
{"version":1,
 "center":{"panel_name":"GridPanel","children":[],"info":{"panel":null}},
 "left_dock":{"panel":{...},"placement":"left","size":384.0,"open":false},
 "right_dock":{"panel":{...},"placement":"right","size":848.0,"open":false},
 "bottom_dock":{"panel":{...},"placement":"bottom","size":320.0,"open":true}}
```

- `size` is a **bare number**, confirming `Pixels`' `#[repr(transparent)]` +
  derived `Serialize` reaches the wire as `f32`.
- Each dock is `{panel, placement, size, open}`. `placement` is a lowercase
  string (`"left"`); dat0 ignores it — the key already names the placement.
- ★ **`center.panel_name` is `"GridPanel"`**, which makes the §3.1 argument
  concrete rather than theoretical: a `DockArea::load` of this very dump would
  hand `"GridPanel"` to `PanelRegistry::build_panel` and re-wrap the result as
  `DockItem::tabs`, restoring the title bar. The probe now asserts the centre is
  PRESENT, so the capture path's job is explicit — the centre exists in the dump
  and is dropped on purpose, not absent by luck.
- ★ The left dock's inner `stack.sizes` is `[384.0, 384.0, 384.0]` and the
  right's is `[288.0, 560.0]` — the inner split sizes B9 deliberately does not
  persist are right there and readable. Noted so a future slice that wants them
  knows they cost only a parser change, not a new API.
- Top-level `version` is `1`, from `DockArea::new("dat0-workspace", Some(1), ..)`.
  Nothing reads it today; a structural change to dat0's dock tree could bump it.

### Probe 3 — early bottom-dock mount ✅

The top risk did not materialise. Mounting the console **before the first frame
settles** — no `run_until_parked` before the toggle — neither panicked with
B7's "cannot read WorkspaceShell while it is already being updated" nor moved
the a11y node count. The `DockItem::tab` single-panel shape is immune for the
documented reason (`set_active_ix(0)` early-returns on an unchanged index,
`tab_panel.rs:208-211`), and that immunity holds at the earliest moment the
restore path could run. **The T5 fallback (defer the mount to after the first
frame) is therefore NOT needed.**

---

## 15. Slice as-built (2026-08-04)

Executed INLINE (no subagents), T0 → T8, one commit per task off main `1e33559`.
Every design decision survived contact; the corrections below are all
implementation-level.

### Deviations from the plan, and why

1. **⚠⚠ The migration carry-over is per-ARM, not inside `migrate_v10_to_v11`.**
   The plan put it in the v10 helper. `ui` has existed since **v8**, so a v8 or
   v9 session with the catalog open would have silently lost it — the exact
   class of failure the raw-document read exists to prevent, reintroduced one
   level up. Now every pre-v11 arm runs `with_carried_layout`; v1–v7 have no
   `ui` and correctly derive an all-closed layout. Guarded by
   `even_a_v8_file_carries_its_dock_bools_over`.
2. **The capture-site list was wrong in both directions.** The plan named three
   `persist_dock_ui` sites. One of them (`toggle_catalog_collapsed`) changes
   catalog TREE state, not dock state, and was NOT wired. `toggle_chart_panel`
   was missing and had to be added — **chart-panel visibility has never been
   persisted by any schema**, so v11 is the first to carry it.
3. **Sizes are `u32`, not `f32`** (amended into §3.2 before implementation).
   `Settings` derives `Eq`. The narrower type also makes NaN and infinity
   unrepresentable on the wire.
4. **Every `Option` needs `skip_serializing_if`** — the `toml` serializer errors
   with `UnsupportedNone` on a `None` inside a table, so settings.toml would be
   unwritable without it. Verified by deleting the attribute and watching the
   `toml::to_string` line fail; the test now asserts toml FIRST so that is the
   line that reddens.
5. **`ai_settings_store` → `settings_store`** (7 callers). It always returned a
   plain `SettingsStore` over settings.toml; B9 is its second non-AI consumer.
6. **`toggle_chart_panel_for_test` added.** `chart_bind_for_test` assigns the
   visibility bool directly (the a11y-shim pattern) and therefore bypasses the
   toggle and its persist entirely — it could not exercise this path.
7. **T0 probe 1 was rewritten before it ran.** The plan proposed re-mounting one
   dock at a probe size; that would have proved little. Asserting all **three**
   distinct mount sizes (384 / 848 / 320) is the stronger claim — one shared
   constant cannot produce three different numbers.

### What T0 settled

All three probes passed first run; **no stop clause fired and the T5 deferred-mount
fallback was never needed**. Probe 3 is the notable one: mounting the bottom dock
*before the first frame settles* does not trip B7's `set_active_ix` →
`Panel::visible` → `shell.read` re-entrancy panic, because a single-panel
`DockItem::tab` early-returns from `set_active_ix(0)`.

### Facts worth not re-deriving

- **`DockArea::load` is unusable for a partial restore, and this is now proven
  live, not just read.** T0 probe 2 captured a real dump whose `center.panel_name`
  is `"GridPanel"` — `load` would hand that to the registry and re-wrap it as
  `DockItem::tabs`. The seven registered panel builders stay deliberately
  unreachable; `src/panels/mod.rs` now says so instead of promising B9 would fix
  them.
- **`Dock` is not an `EventEmitter`** — `dock/dock.rs` has zero `cx.emit`. The
  master plan's `DockEvent::LayoutChanged` save trigger cannot see a resize or
  an open/close. The Quit/CloseWindow flush is the ONLY thing that captures a
  resize not followed by a toggle.
- **The left dock's inner `stack.sizes` is right there in the dump**
  (`[384,384,384]`; right is `[288,560]`). A future slice that wants inner split
  sizes needs a parser change, not a new API.
- **Three separate wire-format snapshots gate this schema** — session, chart,
  and settings — and each caught a fixture a targeted grep had missed. The
  settings one lives in `onboarding_gpui.rs` and was invisible to an audit
  scoped to the session version.
- Settings v2→v3 needs no migration: `Settings` carries container-level
  `serde(default)` and nothing gates on its `schema_version`.

### Local gate — ALL GREEN

`cargo fmt --all --check` clean · `cargo clippy --workspace --all-targets -D
warnings` exit 0 · **118 binaries × {plain, a11y-capture, a11y-capture+gallery},
0 failures** (116 at B8, +`dock_layout_spike`, +`dock_layout_persist`) ·
`style_lint` 4/4 with the ratchet UNCHANGED at `[("window.rs", 1)]` ·
`src/grid` byte-identical to main · `a11y_spike` still **12** ·
`cargo build --bin dat0` builds AND boots with a log **identical** to a `main`
build (zero WARN/ERROR on either).

⚠ `src/session` is deliberately NOT byte-identical — B5–B8 all asserted that,
and B9 is the slice that ends it.

`cargo test --workspace` and `cargo bench` remain unrunnable on this machine
(macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift; reproduces on `main`).

### Test surface added

`tests/dock_layout_spike.rs` (3, standing guards) + `tests/dock_layout_persist.rs`
(20) + 9 unit tests in `session/migrate.rs` + 13 in `session/dock_layout.rs` +
3 in `settings/schema.rs`.

Every task's non-vacuity probe perturbed the MECHANISM. Notably: neutralising
`current_dock_layout` reddens 6 of 9 capture tests, and
`closing_the_rail_panel_persists_the_closed_state` stays green because "closed"
and "default" are the same value — recorded in that test's own doc comment
rather than deleted or trusted (B7's precedent).
