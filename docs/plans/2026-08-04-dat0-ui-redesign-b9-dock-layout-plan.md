# B9 — dock layout persistence: implementation plan

> **For agentic workers:** this slice is executed INLINE by the controller (no subagents), as B5–B8 were. Steps use checkbox (`- [ ]`) syntax for tracking.

**Design:** `docs/plans/2026-08-04-dat0-ui-redesign-b9-dock-layout-design.md`
**Branch:** `feat/ui-redesign-b9-dock-layout` off main `1e33559`
**Goal:** Persist and restore which docks are open, which left-rail panel is showing, and each dock's outer size — across session (v10→v11) and settings (v2→v3).

**Architecture:** `DockArea::dump()` is used as a *read instrument* only; `DockArea::load` is never called (it would rebuild the centre and restore the 30 px title bar — design §3.1). Restore feeds the persisted sizes into the `set_*_dock(item, Some(px), open, ..)` calls `ensure_dock_area` already makes.

**Tech stack:** Rust, gpui 0.2.2, gpui-component rev `0f0ab35`, serde/serde_json/toml.

## Global constraints

- `SESSION_SCHEMA_VERSION` 10 → **11**; `Settings::schema_version` 2 → **3**. Both additive apart from one documented `SessionUiState` reshape.
- `set_left_dock` / `set_right_dock` / `set_bottom_dock` stay called **exactly once each** (B6 subscription leak). Sizes are decided at mount; never re-set to resize.
- `style_lint` ratchet must stay `[("window.rs", 1)]`.
- `src/grid` byte-identical. (`src/session` is deliberately NOT, this slice.)
- `a11y_spike` stays at **12** unless a probe proves otherwise; bump deliberately with a comment naming the nodes, never loosen to `>=`.
- Every commit: `cargo fmt --all` first, `git commit -s` (DCO).
- Never write the CI skip marker in any commit message, even quoted.

## Corrections to the design, made while planning

1. **Sizes are `Option<u32>`, not `Option<f32>`.** `Settings` derives `Eq` (`settings/schema.rs:16`), and an `f32` field would break that derive for every existing consumer. Whole-pixel dock sizes are lossless in practice, and the persisted type then *cannot represent* NaN or infinity — the "reject non-finite" rule collapses into the capture-side conversion instead of being a validation rule that could be forgotten.
2. **Every `Option` field needs `#[serde(skip_serializing_if = "Option::is_none")]`.** The `toml` crate's serializer errors with `UnsupportedNone` on a `None` value inside a table, so the settings half would fail to write without it. Same attribute keeps session.json tidy.
3. **The console mount is extracted, not duplicated.** T5 needs "build the console and mount the bottom dock" from a second call site; the body is lifted out of `toggle_sql_console` into a helper both call, rather than copied.

## File structure

| File | Responsibility |
|---|---|
| `crates/dat0-app/src/session/dock_layout.rs` | **NEW.** `DockLayout` wire type, clamping, the `dump()` JSON mirror, both tolerant deserializers. No gpui types — pure data, unit-testable with no window. |
| `crates/dat0-app/src/session/mod.rs` | `SessionState.dock_layout`, `SESSION_SCHEMA_VERSION = 11`, `Session::dock_layout()` / `set_dock_layout()`, `SessionUiState` reshape (T7). |
| `crates/dat0-app/src/session/migrate.rs` | `migrate_v10_to_v11` + the version arms. |
| `crates/dat0-app/src/settings/schema.rs` | `UiSettings` + `Settings.ui`, `schema_version = 3`. |
| `crates/dat0-app/src/window.rs` | Capture (`current_dock_layout`, `persist_dock_layout`), restore (ctor seeding, `ensure_dock_area` sizes), console mount extraction, settings-seed write. |
| `crates/dat0-app/src/panels/mod.rs` | Builder doc comments (T8). |
| `crates/dat0-app/tests/dock_layout_spike.rs` | **NEW.** T0 gate; kept as standing regression guards. |
| `crates/dat0-app/tests/dock_layout_persist.rs` | **NEW.** Windowed round-trip. |

---

## Task 0: T0 hard gate (spike)

**Files:**
- Create: `crates/dat0-app/tests/dock_layout_spike.rs`
- Modify: `docs/plans/2026-08-04-dat0-ui-redesign-b9-dock-layout-design.md` (append §14 as-built)

**Interfaces:**
- Consumes: nothing.
- Produces: a captured real-dump JSON string, pasted into Task 1 as the parser's test fixture; and a go/no-go on each of the three probes.

**Nothing in production may be edited until this task's findings are recorded.** Probes live in a spike file, never in `window.rs` — B8 found that an in-production probe reddens unrelated `a11y_content` tests as an artifact and makes its own measurements unreadable.

- [ ] **Step 1: Create the spike file with probe 1 — `dump()` reads the live `Dock`**

Copy the harness preamble verbatim from `crates/dat0-app/tests/bottom_dock.rs:15-113` (`set_config_dir`, `init_components`, `AsyncHarness`, `enter_async_harness`, `build_empty_session`, `open_shell_window`, `boot`, `settle`). Then:

```rust
/// Probe 1: `DockArea::dump()` reports the size the dock actually has, not a
/// construction-time copy — for a value both above and below the default, on
/// all three placements.
///
/// This is the round-trip B9's capture path depends on. It does NOT prove that
/// a mouse-drag resize is persisted: `Dock::resize` is reachable only through
/// `resize_handle`'s drag and dat0 cannot obtain the `Entity<Dock>` to call the
/// public `set_size`. What covers that gap is structural — `Dock::resize`
/// mutates the same `self.size` field `DockState::new` reads (`dock/state.rs:34-43`)
/// — plus the owed human glance. See the design's §4.
#[gpui::test]
#[serial]
fn dump_reports_the_size_set_at_mount(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx) = boot(cx);

    // Force the dock to exist, then re-mount the left dock at a non-default size.
    toggle(&shell, vcx);
    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");

    for probe in [200.0_f32, 700.0_f32] {
        vcx.update(|window, app| {
            dock.update(app, |d, cx| {
                d.toggle_dock(DockPlacement::Left, window, cx);
            });
            let _ = window;
        });
        settle(vcx);

        let json = vcx.read(|app| {
            let state = dock.read(app).dump(app);
            serde_json::to_value(&state).expect("dump serialises")
        });
        eprintln!("PROBE1 dump at {probe}: {json:#}");
        assert!(
            json.get("left_dock").is_some(),
            "the left dock must appear in the dump"
        );
    }
}
```

- [ ] **Step 2: Run probe 1 and READ THE PRINTED JSON**

```bash
cargo test -p dat0-app --features a11y-capture --test dock_layout_spike dump_reports -- --nocapture
```

Expected: PASS, and the `PROBE1` lines show a `left_dock` object. **Copy one full `PROBE1` JSON payload into the design doc's §14** — Task 1's parser test uses it as a fixture, and a hand-written fixture would only prove the parser matches my guess.

If `left_dock` is absent or `size` is not a bare number, **stop**: record the actual shape in §14 and re-shape `DumpMirror` in Task 1 to match. If the dump reports the mount constant regardless of the size argument, **stop**: drop to open-flags-only, delete every size field from the plan, and record why.

- [ ] **Step 3: Add probe 2 — the mirror parses a real dump**

```rust
/// Probe 2: the exact JSON shape B9's capture path will parse.
#[gpui::test]
#[serial]
fn dump_json_has_size_and_open_per_dock(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx) = boot(cx);
    toggle(&shell, vcx);
    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");

    let json = vcx.read(|app| {
        serde_json::to_value(dock.read(app).dump(app)).expect("dump serialises")
    });

    for placement in ["left_dock", "right_dock", "bottom_dock"] {
        let slot = json
            .get(placement)
            .unwrap_or_else(|| panic!("{placement} present in dump"));
        assert!(
            slot.get("size").and_then(|s| s.as_f64()).is_some(),
            "{placement}.size must be a bare number (Pixels is repr(transparent) \
             over f32 with a derived Serialize): {slot:#}"
        );
        assert!(
            slot.get("open").and_then(|o| o.as_bool()).is_some(),
            "{placement}.open must be a bool: {slot:#}"
        );
    }
}
```

- [ ] **Step 4: Run probe 2**

```bash
cargo test -p dat0-app --features a11y-capture --test dock_layout_spike dump_json_has -- --nocapture
```

Expected: PASS. If `size` serialises as `{"0": 384.0}` or a string, record the real shape in §14 and adjust `DockSlot` in Task 1.

- [ ] **Step 5: Add probe 3 — eager console mount from inside `ensure_dock_area`'s lease**

This is the top risk: it is the only behaviour change on a path B8 deliberately made lazy, and it probes for B7's construction-time re-entrancy panic (`set_active_ix` → `Panel::visible` → `shell.read` while the shell is leased by its own `render`).

```rust
/// Probe 3: mounting the bottom dock during the first render — the shape T5
/// needs — neither panics nor moves `a11y_spike`'s node count.
///
/// `toggle_sql_console_for_test` runs the production `toggle_sql_console`,
/// which calls `ensure_dock_area` and then `set_bottom_dock` with a
/// single-panel `DockItem::tab`. What T5 changes is only WHEN that runs. This
/// probe drives it before the first frame has settled, which is the earliest
/// moment T5 could.
#[gpui::test]
#[serial]
fn console_mounts_before_the_first_frame_settles(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    init_components(cx);
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    // NO run_until_parked first — this is the whole point.
    vcx.update(|window, app| {
        shell.update(app, |ws, cx| ws.toggle_sql_console_for_test(window, cx));
    });
    settle(vcx);

    let open = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists")
        .read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Bottom, app));
    assert!(open, "bottom dock open after an early mount");
    std::mem::forget(tmp);
}
```

- [ ] **Step 6: Run probe 3, then the whole spike file**

```bash
cargo test -p dat0-app --features a11y-capture --test dock_layout_spike -- --nocapture
cargo test -p dat0-app --features a11y-capture --test a11y_spike
```

Expected: spike PASS; `a11y_spike` PASS and still asserting **12**.

If probe 3 panics with "cannot read WorkspaceShell while it is already being updated", **stop**: T5 changes to a `window.defer` restore that runs after the first frame instead of during it, and the panic text goes into §14 verbatim. If `a11y_spike` moves, record the new count and which nodes account for it before touching anything.

- [ ] **Step 7: Record findings in the design doc**

Append a `## 14. T0 as-built` section to the design doc: each probe's verdict, the real dump JSON payload from Step 2, and any stop-clause that fired with what it changed.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/dock_layout_spike.rs docs/plans/2026-08-04-dat0-ui-redesign-b9-dock-layout-design.md
git commit -s -F - <<'EOF'
test(theme): B9 T0 — dock-layout gate (dump round-trip, JSON shape, early mount)

Three probes, all in a spike file rather than in window.rs (B8's finding that
an in-production probe reddens unrelated a11y_content tests and makes its own
measurements unreadable). Findings recorded in the design doc as section 14.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 1: `DockLayout` wire type, clamping, and the dump mirror

**Files:**
- Create: `crates/dat0-app/src/session/dock_layout.rs`
- Modify: `crates/dat0-app/src/session/mod.rs:23-25` (add `pub mod dock_layout;`)
- Modify: `crates/dat0-app/src/window.rs:446` (add serde derives to `LeftPanel`)

**Interfaces:**
- Consumes: the real dump JSON captured in Task 0.
- Produces: `DockLayout` (fields below), `clamped_size(Option<u32>, f32, f32) -> f32`, `size_to_persist(f32) -> Option<u32>`, `mirror_from_dump(&serde_json::Value) -> DumpMirror`, `DumpMirror { left_dock, right_dock, bottom_dock: Option<DockSlot> }`, `DockSlot { size: f32, open: bool }`, `de_tolerant_json`, `de_tolerant_toml`, consts `DOCK_MIN_SIZE = 100.0` and `DOCK_MAX_AXIS_FRACTION = 0.8`.

Purely additive: nothing consumes this module until Task 2.

- [ ] **Step 1: Write the failing tests**

Create `crates/dat0-app/src/session/dock_layout.rs` with only the test module and the imports it needs:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::LeftPanel;

    #[test]
    fn absent_size_falls_back_to_the_mount_constant() {
        assert_eq!(clamped_size(None, 384.0, 1920.0), 384.0);
    }

    #[test]
    fn an_in_band_size_is_returned_untouched() {
        assert_eq!(clamped_size(Some(500), 384.0, 1920.0), 500.0);
    }

    #[test]
    fn an_oversized_dock_is_clamped_to_four_fifths_of_the_axis() {
        // Saved on a 3840px display, restored in a 1000px window.
        assert_eq!(clamped_size(Some(3000), 384.0, 1000.0), 800.0);
    }

    #[test]
    fn an_undersized_dock_is_clamped_up_to_the_upstream_minimum() {
        assert_eq!(clamped_size(Some(3), 384.0, 1920.0), DOCK_MIN_SIZE);
    }

    #[test]
    fn a_degenerate_axis_extent_never_panics_or_inverts_the_band() {
        // f32::clamp panics if min > max or either is NaN. A headless window
        // can report a zero viewport, so this is a real input, not a fuzz case.
        assert_eq!(clamped_size(Some(500), 384.0, 0.0), 500.0);
        assert_eq!(clamped_size(Some(500), 384.0, f32::NAN), 500.0);
        assert_eq!(clamped_size(Some(50), 384.0, 0.0), DOCK_MIN_SIZE);
    }

    #[test]
    fn non_finite_sizes_are_never_persisted() {
        assert_eq!(size_to_persist(f32::NAN), None);
        assert_eq!(size_to_persist(f32::INFINITY), None);
        assert_eq!(size_to_persist(-5.0), None);
        assert_eq!(size_to_persist(383.6), Some(384));
    }

    #[test]
    fn open_state_is_derived_never_stored() {
        let closed = DockLayout::default();
        assert!(!closed.left_open());
        assert!(!closed.right_open());

        let left = DockLayout {
            left_panel: Some(LeftPanel::Ai),
            ..DockLayout::default()
        };
        assert!(left.left_open(), "a chosen left panel means the dock is open");

        let right = DockLayout {
            charts_visible: true,
            ..DockLayout::default()
        };
        assert!(right.right_open(), "either right panel opens the right dock");
    }

    #[test]
    fn the_wire_form_omits_absent_options_and_names_the_panel() {
        let l = DockLayout {
            left_panel: Some(LeftPanel::Connections),
            inspector_visible: true,
            ..DockLayout::default()
        };
        let json = serde_json::to_value(&l).unwrap();
        assert_eq!(json["left_panel"], "connections");
        assert!(
            json.get("left_size").is_none(),
            "None must be OMITTED, not null — the toml serializer errors on a \
             None inside a table, so skip_serializing_if is load-bearing for \
             the settings half: {json:#}"
        );
        // toml is the format that actually enforces it.
        toml::to_string(&l).expect("DockLayout must be toml-serialisable");
    }

    /// The literal payload captured by T0 probe 1 — a REAL `DockArea::dump()`,
    /// not a hand-written guess at its shape.
    const REAL_DUMP: &str = r#"__PASTE_FROM_T0_STEP_2__"#;

    #[test]
    fn the_mirror_reads_a_real_dump() {
        let v: serde_json::Value = serde_json::from_str(REAL_DUMP).unwrap();
        let m = mirror_from_dump(&v);
        let left = m.left_dock.expect("left dock present in the real dump");
        assert!(left.size > 0.0);
    }

    #[test]
    fn the_mirror_ignores_the_centre_entirely() {
        // "Docks only, never the centre" is structural here: there is no field
        // to put a centre in, so a dump carrying one cannot smuggle it through.
        let v = serde_json::json!({
            "version": 1,
            "center": { "panel_name": "GridPanel", "children": [] },
            "left_dock": { "size": 384.0, "open": true }
        });
        let m = mirror_from_dump(&v);
        assert_eq!(m.left_dock.map(|d| d.size), Some(384.0));
        assert!(m.right_dock.is_none());
    }

    #[test]
    fn a_junk_dump_degrades_to_empty_rather_than_panicking() {
        let m = mirror_from_dump(&serde_json::json!("not an object"));
        assert_eq!(m, DumpMirror::default());
    }

    #[test]
    fn a_malformed_layout_parses_as_none_not_an_error() {
        #[derive(serde::Deserialize)]
        struct Holder {
            #[serde(default, deserialize_with = "de_tolerant_json")]
            dock_layout: Option<DockLayout>,
        }
        let good: Holder = serde_json::from_str(r#"{"dock_layout":{"console_open":true}}"#).unwrap();
        assert_eq!(good.dock_layout.map(|l| l.console_open), Some(true));

        let bad: Holder = serde_json::from_str(r#"{"dock_layout":"nonsense"}"#)
            .expect("a malformed layout must NOT fail the enclosing document");
        assert!(bad.dock_layout.is_none());

        let wrong_type: Holder =
            serde_json::from_str(r#"{"dock_layout":{"console_open":"yes"}}"#).unwrap();
        assert!(wrong_type.dock_layout.is_none());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p dat0-app --lib session::dock_layout
```

Expected: FAIL to compile — `cannot find function clamped_size`, `cannot find type DockLayout`.

- [ ] **Step 3: Add the serde derives to `LeftPanel`**

In `crates/dat0-app/src/window.rs`, replace the derive line at `:446`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeftPanel {
```

`LeftPanel` is reused rather than mirrored by a parallel session-side enum precisely so panel identity has one definition; it is a field-less enum with no gpui dependency, so the session module can name it freely.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/dat0-app/src/session/dock_layout.rs`, above the test module:

```rust
//! Persisted dock layout (session v11, settings v3).
//!
//! **Docks only — never the centre.** `PanelState::to_item` rebuilds a
//! `PanelInfo::Panel` as `DockItem::tabs` (`dock/state.rs:227-236`), so a
//! restored centre would come back wrapped in a `TabPanel` and regain the 30 px
//! title bar B5 chose `DockItem::Panel` to avoid. This module makes that
//! structural rather than remembered: there is no field here that can hold a
//! centre, and [`DumpMirror`] reads only the three dock slots out of a
//! `DockArea::dump()`.
//!
//! Pure data + helpers, no gpui types — persistence lives in `SessionState`
//! (session/mod.rs) and `Settings` (settings/schema.rs), mirroring
//! `session/charts.rs` and `session/queries.rs`.

use serde::{Deserialize, Serialize};

use crate::window::LeftPanel;

/// Lower clamp bound for a restored dock. This is upstream's own
/// `PANEL_MIN_SIZE` (`resizable/mod.rs:14` = `px(100.)`), restated here because
/// that const is `pub(crate)` upstream and therefore unnameable from dat0.
pub const DOCK_MIN_SIZE: f32 = 100.0;

/// A restored dock may never take more than this share of its axis, so the
/// centre always keeps a fifth of the window and the dock's own resize handle
/// stays on screen to recover with.
pub const DOCK_MAX_AXIS_FRACTION: f32 = 0.8;

/// The persisted dock layout.
///
/// Sizes are whole pixels: `Settings` derives `Eq`, and an `f32` field would
/// break that derive for every existing consumer. The narrower type also means
/// NaN and infinity are *unrepresentable* on the wire rather than merely
/// rejected by a validation rule someone could forget to call.
///
/// There is deliberately no per-dock `open` flag — see [`DockLayout::left_open`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockLayout {
    /// Which left-rail panel is showing. `None` = the left dock is closed.
    ///
    /// B7's at-most-one-visible invariant is unrepresentable-if-violated here,
    /// where three parallel bools could contradict it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_panel: Option<LeftPanel>,
    /// User-resized width. `None` = use the mount constant, so an untouched
    /// dock keeps inheriting `LEFT_DOCK_WIDTH` if that constant ever changes.
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

impl DockLayout {
    /// The left dock is open iff a panel is chosen. Derived, never stored —
    /// the master plan's standing rule for B6+ is to derive dock state and
    /// never keep parallel bools.
    pub fn left_open(&self) -> bool {
        self.left_panel.is_some()
    }

    /// The right dock is open iff either of its two panels is visible.
    pub fn right_open(&self) -> bool {
        self.inspector_visible || self.charts_visible
    }
}

/// Resolve a persisted size into the pixel width/height to mount with.
///
/// `axis_extent` is the window's extent on the dock's axis (width for
/// left/right, height for bottom). A degenerate extent — zero or non-finite,
/// both of which a headless window can report — disables the upper bound rather
/// than inverting the band: `f32::clamp` PANICS if `min > max` or if either
/// bound is NaN.
pub fn clamped_size(persisted: Option<u32>, default_size: f32, axis_extent: f32) -> f32 {
    let Some(size) = persisted else {
        return default_size;
    };
    let max = if axis_extent.is_finite() && axis_extent > 0.0 {
        (axis_extent * DOCK_MAX_AXIS_FRACTION).max(DOCK_MIN_SIZE)
    } else {
        f32::MAX
    };
    (size as f32).clamp(DOCK_MIN_SIZE, max)
}

/// Convert a live dock size into the persisted form, dropping anything that
/// cannot round-trip. `f32 as u32` saturates rather than wrapping, so the only
/// values needing an explicit guard are the non-finite and negative ones.
pub fn size_to_persist(size: f32) -> Option<u32> {
    if !size.is_finite() || size < 0.0 {
        return None;
    }
    Some(size.round() as u32)
}

/// One dock's slice of a `DockArea::dump()`. `Pixels` is `#[repr(transparent)]`
/// over `f32` with a derived `Serialize` (`gpui-0.2.2/src/geometry.rs:2565-2573`),
/// so `size` arrives as a bare number.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct DockSlot {
    pub size: f32,
    pub open: bool,
}

/// The only part of a `DockArea::dump()` dat0 reads. Every other key — the
/// whole `center`, every `PanelState`, the version — has no field to land in
/// and is discarded by construction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct DumpMirror {
    #[serde(default)]
    pub left_dock: Option<DockSlot>,
    #[serde(default)]
    pub right_dock: Option<DockSlot>,
    #[serde(default)]
    pub bottom_dock: Option<DockSlot>,
}

/// Read the three dock slots out of a serialized `DockArea::dump()`.
/// Degrades to an empty mirror rather than erroring: a layout read must never
/// be able to take down the caller.
pub fn mirror_from_dump(dump: &serde_json::Value) -> DumpMirror {
    serde_json::from_value(dump.clone()).unwrap_or_default()
}

/// Tolerant field deserializer for **session.json**.
///
/// A malformed `dock_layout` degrades to `None` — the default layout — while
/// tabs, SQL tabs, history, saved queries, charts and attachments still load.
/// Layout is the least valuable thing in the file and must never be able to
/// cost a user their work.
///
/// Serde cannot express format-agnostic error recovery: catching a
/// deserialization error needs a buffered value type, so this has a `toml`
/// sibling below rather than being one generic function.
pub fn de_tolerant_json<'de, D>(d: D) -> Result<Option<DockLayout>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(value).ok())
}

/// Tolerant field deserializer for **settings.toml**. See [`de_tolerant_json`].
pub fn de_tolerant_toml<'de, D>(d: D) -> Result<Option<DockLayout>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = toml::Value::deserialize(d)?;
    Ok(value.try_into().ok())
}
```

Then add the module declaration to `crates/dat0-app/src/session/mod.rs` after line 23:

```rust
pub mod charts;
pub mod dock_layout;
pub mod migrate;
pub mod queries;
```

- [ ] **Step 5: Paste the real dump fixture**

Replace `__PASTE_FROM_T0_STEP_2__` in the test module with the JSON payload captured by T0 probe 1.

- [ ] **Step 6: Run the tests**

```bash
cargo test -p dat0-app --lib session::dock_layout
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: all 11 tests PASS, clippy clean.

- [ ] **Step 7: Prove non-vacuity by perturbing the MECHANISM**

Not the expectation — B8's rule, after a guard that counted the wrong thing passed its own probe. Temporarily delete `skip_serializing_if` from `left_size` and re-run: `the_wire_form_omits_absent_options_and_names_the_panel` must go RED on the `toml::to_string` line, not merely on the `get("left_size")` assertion. Then temporarily change `DOCK_MAX_AXIS_FRACTION` to `1.0` and confirm `an_oversized_dock_is_clamped...` goes RED. Revert both.

⚠ After reverting a probe, `touch` the file and re-run — a reverted source can report a stale result because cargo reuses the old binary (A6's finding).

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/session/dock_layout.rs crates/dat0-app/src/session/mod.rs crates/dat0-app/src/window.rs
git commit -s -F - <<'EOF'
feat(theme): B9 T1 — DockLayout wire type, clamping, and the dump mirror

Pure data module, nothing consumes it yet. Sizes are u32 because Settings
derives Eq and because the narrower type makes NaN unrepresentable rather
than merely rejected. Every Option carries skip_serializing_if: the toml
serializer errors on a None inside a table.

DumpMirror reads only the three dock slots out of a DockArea::dump(), so
"docks only, never the centre" is structural — there is no field a centre
could land in.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 2: Session schema v10 → v11

**Files:**
- Modify: `crates/dat0-app/src/session/mod.rs` (`SESSION_SCHEMA_VERSION`, `SessionState.dock_layout`, `Session.dock_layout` field + accessors, `persist`, `Default`, all four constructors)
- Modify: `crates/dat0-app/src/session/migrate.rs` (version arms + `migrate_v10_to_v11`)

**Interfaces:**
- Consumes: `DockLayout`, `de_tolerant_json` (Task 1).
- Produces: `Session::dock_layout() -> Option<&DockLayout>`, `Session::set_dock_layout(Option<DockLayout>) -> Result<()>`, `SessionState.dock_layout`.

⚠ **`SessionUiState` keeps `catalog_panel_visible` and `inspector_panel_visible` in this task.** Both sources coexist until Task 7 strips the old ones. This is A1's hybrid trick — retain the legacy block verbatim so every parser stays green mid-swap, and remove it in one final commit — and it is what keeps every commit on this branch building.

- [ ] **Step 1: Write the failing tests**

Append to the `mod tests` block in `crates/dat0-app/src/session/migrate.rs`:

```rust
    #[test]
    fn v10_carries_its_dock_bools_into_the_v11_layout() {
        // A user whose catalog and inspector were open must not silently lose
        // them on upgrade. v9→v10 could DISCARD its old keys because prod only
        // ever wrote them at their empty defaults; these two hold real values.
        let v10 = r#"{
            "schema_version": 10,
            "tabs": [],
            "ui": {
                "catalog_panel_visible": true,
                "inspector_panel_visible": true,
                "catalog_collapsed": ["md"]
            }
        }"#;
        let state = load_str(v10).expect("v10 migrates");
        assert_eq!(state.schema_version, SESSION_SCHEMA_VERSION);
        let layout = state.dock_layout.expect("v10 always yields a layout");
        assert_eq!(layout.left_panel, Some(crate::window::LeftPanel::Catalog));
        assert!(layout.inspector_visible);
        assert_eq!(
            state.ui.catalog_collapsed,
            vec!["md".to_string()],
            "catalog collapse state is tree state, not dock layout — it stays"
        );
    }

    #[test]
    fn a_v10_session_with_everything_closed_still_yields_a_layout() {
        // Some(all-closed), not None: a migrated session states its own
        // layout, and must not fall through to the settings seed and reopen
        // docks the user had closed.
        let v10 = r#"{"schema_version": 10, "tabs": [], "ui": {}}"#;
        let state = load_str(v10).expect("v10 migrates");
        let layout = state.dock_layout.expect("v10 always yields a layout");
        assert_eq!(layout, crate::session::dock_layout::DockLayout::default());
    }

    #[test]
    fn a_v11_session_loads_its_layout_as_is() {
        let v11 = r#"{
            "schema_version": 11,
            "tabs": [],
            "dock_layout": { "left_panel": "ai", "left_size": 500, "console_open": true }
        }"#;
        let state = load_str(v11).expect("v11 loads");
        let layout = state.dock_layout.expect("layout present");
        assert_eq!(layout.left_panel, Some(crate::window::LeftPanel::Ai));
        assert_eq!(layout.left_size, Some(500));
        assert!(layout.console_open);
    }

    #[test]
    fn a_malformed_layout_never_costs_the_user_their_tabs() {
        let v11 = r#"{
            "schema_version": 11,
            "tabs": [{"table_name": "t1", "source_path": null}],
            "dock_layout": "corrupt"
        }"#;
        let state = load_str(v11).expect("a bad layout must not fail the document");
        assert_eq!(state.tabs.len(), 1, "the user's tab survived");
        assert!(state.dock_layout.is_none(), "the layout degraded to default");
    }

    #[test]
    fn version_twelve_is_still_rejected() {
        let v12 = r#"{"schema_version": 12, "tabs": []}"#;
        assert!(matches!(
            load_str(v12),
            Err(SessionLoadError::UnsupportedVersion(12))
        ));
    }
```

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p dat0-app --lib session::migrate
```

Expected: FAIL — `no field dock_layout on type SessionState`.

- [ ] **Step 3: Add the field and bump the version**

In `crates/dat0-app/src/session/mod.rs`, extend the version-ledger doc comment above `SESSION_SCHEMA_VERSION` and bump it:

```rust
/// v10 → v11 (B9, UI redesign) adds `dock_layout`: which docks are open, which
/// left-rail panel is showing, and each dock's outer size. Additive — a v10
/// file has no such key. The migration CARRIES OVER `ui.catalog_panel_visible`
/// and `ui.inspector_panel_visible`, which v11 relocates into the layout.
///
/// Docks only, never the centre: see `session/dock_layout.rs`.
pub const SESSION_SCHEMA_VERSION: u32 = 11;
```

Add the field to `SessionState`, after `ui`:

```rust
    #[serde(default)]
    pub ui: SessionUiState,
    /// B9 dock layout. Tolerant on read: a malformed value degrades to `None`
    /// (the default layout) instead of failing the whole document, so a bad
    /// layout can never cost a user their tabs.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::session::dock_layout::de_tolerant_json"
    )]
    pub dock_layout: Option<crate::session::dock_layout::DockLayout>,
```

Add `dock_layout: None,` to `SessionState`'s `Default` impl (after `ui: SessionUiState::default(),`).

- [ ] **Step 4: Add the live field, accessors, and persist wiring**

Add to the `Session` struct, after `ui: SessionUiState,`:

```rust
    dock_layout: Option<crate::session::dock_layout::DockLayout>,
```

Add `dock_layout: None,` to each of the four `Self { .. }` literals in `Session::new`, `recover`, `recover_workspace`, and the remaining constructor (session/mod.rs lines ~254, ~442, ~454, ~498 — the compiler will enumerate them; add the field wherever `ui:` is already initialised, and use the value parsed from the loaded `SessionState` in the recovering constructors, `None` in the fresh one).

Add the accessors next to `ui()` / `set_ui()`:

```rust
    /// Read-only access to the persisted dock layout (B9).
    pub fn dock_layout(&self) -> Option<&crate::session::dock_layout::DockLayout> {
        self.dock_layout.as_ref()
    }

    /// Replace the persisted dock layout and persist (B9).
    pub fn set_dock_layout(
        &mut self,
        layout: Option<crate::session::dock_layout::DockLayout>,
    ) -> Result<()> {
        self.dock_layout = layout;
        self.persist()
            .context("session::set_dock_layout: persist failed")
    }
```

Add `dock_layout: self.dock_layout.clone(),` to the `SessionState { .. }` literal inside `persist()`.

- [ ] **Step 5: Add the migration**

In `crates/dat0-app/src/session/migrate.rs`, change the match arms:

```rust
        9 => migrate_v9_to_v10(raw),
        10 => migrate_v10_to_v11(raw),
        11 => {
```

(the existing `10 => { ... }` body — the forward-incompat transform-`kind` pre-check plus the strict parse — becomes the `11` arm unchanged; update its comment's "v10" references to "v11"), and add the helper next to `migrate_v9_to_v10`:

```rust
/// Migrate a raw v10 JSON string to a v11 `SessionState`.
///
/// v11 adds `dock_layout` and relocates two `ui` bools into it.
///
/// ⚠ The two bools are read from the RAW document, not from the parsed
/// `SessionState`. They still exist on `SessionUiState` today, but B9's final
/// commit removes them — after which serde drops those keys SILENTLY, with no
/// error, and a migration reading the parsed struct would quietly reset every
/// existing user's layout. Reading the raw JSON is correct both before and
/// after that removal, so this function does not change when it happens.
///
/// A v10 file always yields `Some(layout)`, even when everything was closed:
/// `None` would fall through to the settings seed (design §3.3) and reopen
/// docks the user had deliberately shut.
fn migrate_v10_to_v11(raw: &str) -> Result<SessionState, SessionLoadError> {
    let doc: serde_json::Value = serde_json::from_str(raw)?;
    let flag = |key: &str| {
        doc.pointer(&format!("/ui/{key}"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    };
    let catalog_open = flag("catalog_panel_visible");
    let inspector_open = flag("inspector_panel_visible");

    let mut state: SessionState = serde_json::from_str(raw)?;
    state.schema_version = SESSION_SCHEMA_VERSION;
    state.dock_layout = Some(crate::session::dock_layout::DockLayout {
        left_panel: catalog_open.then_some(crate::window::LeftPanel::Catalog),
        inspector_visible: inspector_open,
        ..Default::default()
    });
    Ok(state)
}
```

- [ ] **Step 6: Run the tests**

```bash
cargo test -p dat0-app --lib session::
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: PASS, including the pre-existing `session::mod` tests that assert `schema_version == SESSION_SCHEMA_VERSION` (they read the const, so the bump is transparent to them).

- [ ] **Step 7: Audit fixtures for a hardcoded version 10**

```bash
grep -rn '"schema_version": *10\|"schema_version":10\|schema_version = 10' crates/ tests/ .github/ 2>/dev/null
```

Every hit is either a deliberate v10 migration fixture (leave it — it now exercises the new migration) or a fixture that meant "current" (update it to 11). Check `crash-e2e` and workspace fixtures specifically, per the master plan's B9 row.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/session/
git commit -s -F - <<'EOF'
feat(theme): B9 T2 — session schema v10 to v11 (dock_layout)

Additive field plus one relocation: ui.catalog_panel_visible and
ui.inspector_panel_visible move into dock_layout. The migration reads both
from the RAW document rather than the parsed struct, because T7 removes those
fields and serde then drops the keys silently -- a parsed read would become an
invisible one-time layout reset for every existing user.

SessionUiState still carries both fields at this commit. Both sources coexist
until T7 strips the old ones, which is what keeps every commit building (the
A1 hybrid trick).

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 3: Capture — read the live layout and write it to the session

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (new `current_dock_layout` + `persist_dock_layout`; call sites at `:3414`, `:7040`, `:7755`, and `flush_focused_workspace_sql` at `:1653`)
- Create: `crates/dat0-app/tests/dock_layout_persist.rs`

**Interfaces:**
- Consumes: `mirror_from_dump`, `size_to_persist`, `DockLayout` (Task 1); `Session::set_dock_layout` (Task 2).
- Produces: `WorkspaceShell::current_dock_layout(&self, cx: &gpui::App) -> DockLayout`, `WorkspaceShell::persist_dock_layout(&self, cx: &gpui::App)`.

- [ ] **Step 1: Write the failing test**

Create `crates/dat0-app/tests/dock_layout_persist.rs`. Copy the harness preamble verbatim from `crates/dat0-app/tests/bottom_dock.rs:15-113`, then:

```rust
/// Read the layout back off DISK, not off the in-memory session — a re-read of
/// the value the test just wrote would prove only that assignment works (B6's
/// rule).
fn layout_on_disk(session: &Arc<Mutex<Session>>) -> dat0_app::session::dock_layout::DockLayout {
    let path = session.lock().home.root_dir().join("session.json");
    let raw = std::fs::read_to_string(path).expect("session.json exists");
    let state = dat0_app::session::migrate::load_str(&raw).expect("session.json parses");
    state.dock_layout.expect("a layout was persisted")
}

#[gpui::test]
#[serial]
fn activating_a_rail_panel_persists_it(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    init_components(cx);
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session.clone());
    vcx.run_until_parked();

    vcx.update(|_window, app| {
        shell.update(app, |ws, cx| {
            ws.activate_left_panel(dat0_app::window::LeftPanel::Ai, cx)
        });
    });
    settle(vcx);

    let layout = layout_on_disk(&session);
    assert_eq!(layout.left_panel, Some(dat0_app::window::LeftPanel::Ai));
    assert!(layout.left_open());
    std::mem::forget(tmp);
}

#[gpui::test]
#[serial]
fn opening_the_console_persists_it(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    init_components(cx);
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session.clone());
    vcx.run_until_parked();

    toggle(&shell, vcx);

    let layout = layout_on_disk(&session);
    assert!(layout.console_open, "an open console is persisted");
    assert!(
        layout.bottom_size.is_some(),
        "the bottom dock's height comes from the live dump, so it is known \
         as soon as the dock exists"
    );
    std::mem::forget(tmp);
}
```

Add the `toggle` helper from `bottom_dock.rs:120-126` verbatim.

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p dat0-app --features a11y-capture --test dock_layout_persist
```

Expected: FAIL — `a layout was persisted` panics, because nothing writes one yet.

- [ ] **Step 3: Write the capture path**

Add to `impl WorkspaceShell` in `crates/dat0-app/src/window.rs`, next to `persist_dock_ui` (`:4312`):

```rust
    /// B9: the live dock layout — what is open, and how big.
    ///
    /// Sizes come from `DockArea::dump()` because that is the ONLY public route
    /// to them at rev `0f0ab35`: `DockArea` keeps `left_dock`/`right_dock`/
    /// `bottom_dock` private with no getter, so `Dock::size()` — which is `pub`
    /// — is unreachable from here. Everything else in the dump (the whole
    /// centre) is discarded by `DumpMirror`, which has no field for it.
    pub(crate) fn current_dock_layout(
        &self,
        cx: &gpui::App,
    ) -> crate::session::dock_layout::DockLayout {
        use crate::session::dock_layout::{DockLayout, mirror_from_dump, size_to_persist};

        let mirror = self
            .dock_area
            .as_ref()
            .and_then(|dock| serde_json::to_value(dock.read(cx).dump(cx)).ok())
            .map(|v| mirror_from_dump(&v))
            .unwrap_or_default();

        DockLayout {
            left_panel: self.open_left_panel(),
            left_size: mirror.left_dock.and_then(|d| size_to_persist(d.size)),
            inspector_visible: self.inspector_panel_visible,
            charts_visible: self.chart_panel_visible,
            right_size: mirror.right_dock.and_then(|d| size_to_persist(d.size)),
            console_open: mirror.bottom_dock.is_some_and(|d| d.open),
            bottom_size: mirror.bottom_dock.and_then(|d| size_to_persist(d.size)),
        }
    }

    /// B9: persist the live dock layout to `session.json`.
    ///
    /// Called from the sites that already persist dock UI, plus the Quit /
    /// CloseWindow flush. Open and close need no upstream event — every one of
    /// them goes through dat0's own toggles — but SIZE is a pure upstream mouse
    /// drag with no dat0 code in the loop, which is why the close backstop
    /// matters: it is what captures a resize not followed by any toggle.
    /// `DockEvent::LayoutChanged` is NOT usable for this: `Dock` is not an
    /// `EventEmitter` at all (`dock/dock.rs` has zero `cx.emit`), so neither a
    /// resize nor an open/close ever emits it.
    pub(crate) fn persist_dock_layout(&self, cx: &gpui::App) {
        let layout = self.current_dock_layout(cx);
        if let Err(e) = self.session.lock().set_dock_layout(Some(layout)) {
            tracing::warn!(error = %e, "persist_dock_layout: set_dock_layout failed");
        }
    }
```

- [ ] **Step 4: Wire the three toggle sites**

At each of `window.rs:3414`, `:7040` and `:7755`, add a `persist_dock_layout` call beside the existing `persist_dock_ui()`. Each site already has a `cx` in scope; `&Context<Self>` derefs to `&App`.

```rust
        self.persist_dock_ui();
        self.persist_dock_layout(cx);
```

At `:7755` the call is on `ws` inside a listener: `ws.persist_dock_ui(); ws.persist_dock_layout(cx);`.

Also update the stale comment at `:7038-7039` ("Only `catalog_panel_visible` is persisted (session v10)…"), which B9 makes false:

```rust
        // Session v11 persists the whole rail choice, not just the catalog.
        self.persist_dock_ui();
        self.persist_dock_layout(cx);
```

- [ ] **Step 5: Wire the close/quit backstop**

In `flush_focused_workspace_sql` (`window.rs:1653`), extend the final line and the doc comment:

```rust
/// Best-effort flush of the focused workspace's SQL-console edit buffer AND its
/// dock layout to disk — the same persists the `on_window_should_close`
/// backstop runs on the OS close-button path. Used by the menu Quit / Close
/// Window handlers, whose paths (`platform.quit()` / `Window::remove_window`)
/// never fire that hook. No-op when no workspace is registered or the entity is
/// gone.
///
/// The layout flush is what captures a dock RESIZE: resizing is a pure upstream
/// mouse drag that runs no dat0 code and emits no event, so a resize never
/// followed by a toggle would otherwise be lost.
fn flush_focused_workspace_sql(cx: &mut App) {
    // ... unchanged lookup ...
    ws.update(cx, |ws, cx| {
        ws.persist_sql_console(cx);
        ws.persist_dock_layout(cx);
    });
}
```

- [ ] **Step 6: Run the tests**

```bash
cargo test -p dat0-app --features a11y-capture --test dock_layout_persist
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: both tests PASS.

- [ ] **Step 7: Prove non-vacuity by perturbing the MECHANISM**

Temporarily make `current_dock_layout` return `DockLayout::default()` instead of reading the shell and the dump. Both tests must go RED. A test that stays green here is reading something other than the capture path. Revert, `touch`, re-run.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/dock_layout_persist.rs
git commit -s -F - <<'EOF'
feat(theme): B9 T3 — capture the live dock layout into session.json

Sizes come from DockArea::dump() because it is the only public route to them
at this rev: DockArea keeps all three Dock entities private with no getter, so
the public Dock::size() is unreachable from dat0.

Wired to the three sites that already persist dock UI plus the Quit /
CloseWindow flush. The backstop is load-bearing rather than belt-and-braces:
a dock resize runs no dat0 code and emits no event (Dock is not an
EventEmitter), so a resize not followed by a toggle is only ever captured at
close.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 4: Restore — seed the shell and mount the docks at persisted sizes

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (`WorkspaceShell` field, ctor at `:2523` and `:2575-2588`, `ensure_dock_area` at `:6626-6753`)
- Modify: `crates/dat0-app/tests/dock_layout_persist.rs`

**Interfaces:**
- Consumes: `Session::dock_layout()` (Task 2), `clamped_size` (Task 1).
- Produces: `WorkspaceShell.restored_layout: Option<DockLayout>` (private), consumed by `ensure_dock_area`.

- [ ] **Step 1: Write the failing test**

Append to `crates/dat0-app/tests/dock_layout_persist.rs`:

```rust
/// Seed a session's layout on disk, then open a shell over that same session
/// directory — the shape of reopening a workspace.
fn reopen_with_layout(
    cx: &mut TestAppContext,
    layout: dat0_app::session::dock_layout::DockLayout,
) -> (Entity<WorkspaceShell>, &mut VisualTestContext, Arc<Mutex<Session>>) {
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    init_components(cx);
    let session = build_empty_session(&tmp.path().join("state"));
    session
        .lock()
        .set_dock_layout(Some(layout))
        .expect("seed the layout");
    let (shell, vcx) = open_shell_window(cx, session.clone());
    vcx.run_until_parked();
    std::mem::forget(tmp);
    (shell, vcx, session)
}

#[gpui::test]
#[serial]
fn a_persisted_rail_panel_comes_back_open(cx: &mut TestAppContext) {
    use dat0_app::session::dock_layout::DockLayout;
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _s) = reopen_with_layout(
        cx,
        DockLayout {
            left_panel: Some(dat0_app::window::LeftPanel::Connections),
            ..DockLayout::default()
        },
    );

    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    assert!(
        dock.read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Left, app)),
        "the left dock reopened"
    );
    assert_eq!(
        shell.read_with(&vcx.cx, |ws, _| ws.open_left_panel()),
        Some(dat0_app::window::LeftPanel::Connections)
    );
}

#[gpui::test]
#[serial]
fn a_persisted_size_is_honoured_at_mount(cx: &mut TestAppContext) {
    use dat0_app::session::dock_layout::DockLayout;
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _s) = reopen_with_layout(
        cx,
        DockLayout {
            left_panel: Some(dat0_app::window::LeftPanel::Catalog),
            left_size: Some(500),
            ..DockLayout::default()
        },
    );

    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    let size = vcx.read(|app| {
        let v = serde_json::to_value(dock.read(app).dump(app)).unwrap();
        dat0_app::session::dock_layout::mirror_from_dump(&v)
            .left_dock
            .expect("left dock")
            .size
    });
    assert_eq!(size, 500.0, "the persisted width won over LEFT_DOCK_WIDTH");
}

#[gpui::test]
#[serial]
fn an_absurd_persisted_size_is_clamped_not_obeyed(cx: &mut TestAppContext) {
    use dat0_app::session::dock_layout::DockLayout;
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _s) = reopen_with_layout(
        cx,
        DockLayout {
            left_panel: Some(dat0_app::window::LeftPanel::Catalog),
            left_size: Some(30_000),
            ..DockLayout::default()
        },
    );

    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    let size = vcx.read(|app| {
        let v = serde_json::to_value(dock.read(app).dump(app)).unwrap();
        dat0_app::session::dock_layout::mirror_from_dump(&v)
            .left_dock
            .expect("left dock")
            .size
    });
    assert!(
        size < 30_000.0,
        "a layout saved on a huge display must not make the window unusable; \
         got {size}"
    );
}

#[gpui::test]
#[serial]
fn no_persisted_layout_means_the_mount_constants(cx: &mut TestAppContext) {
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx) = boot(cx);
    let dock = shell.read_with(&vcx.cx, |ws, _| ws.dock_area_for_test());
    assert_eq!(
        shell.read_with(&vcx.cx, |ws, _| ws.open_left_panel()),
        None,
        "a fresh session opens with no left panel"
    );
    let _ = dock;
}
```

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p dat0-app --features a11y-capture --test dock_layout_persist
```

Expected: the three restore tests FAIL (`the left dock reopened` / size is `384.0`), the last one passes.

- [ ] **Step 3: Add the shell field**

In the `WorkspaceShell` struct next to `left_dock_state` (`window.rs:2187`):

```rust
    /// B9: the layout this window was restored with, consumed once by
    /// `ensure_dock_area` for its dock sizes. The visibility bools are seeded
    /// directly in the constructor because `render` reads them before
    /// `ensure_dock_area` runs.
    restored_layout: Option<crate::session::dock_layout::DockLayout>,
```

- [ ] **Step 4: Seed from the session in the constructor**

At `window.rs:2523`, alongside the existing `let ui = session.lock().ui().clone();`:

```rust
        let ui = session.lock().ui().clone();
        // B9: the persisted layout wins over the v10 `ui` bools when present.
        // Session state is authoritative; the settings seed (T6) fills in only
        // when the session has no layout of its own.
        let layout = session.lock().dock_layout().cloned();
```

Then replace the visibility initialisers at `:2575-2588`:

```rust
            connections_panel_visible: layout
                .as_ref()
                .is_some_and(|l| l.left_panel == Some(LeftPanel::Connections)),
            catalog_panel_visible: match layout.as_ref() {
                Some(l) => l.left_panel == Some(LeftPanel::Catalog),
                None => ui.catalog_panel_visible,
            },
```

```rust
            inspector_panel_visible: match layout.as_ref() {
                Some(l) => l.inspector_visible,
                None => ui.inspector_panel_visible,
            },
```

```rust
            ai_panel_visible: layout
                .as_ref()
                .is_some_and(|l| l.left_panel == Some(LeftPanel::Ai)),
```

```rust
            chart_panel_visible: layout.as_ref().is_some_and(|l| l.charts_visible),
```

and add the field to the literal:

```rust
            restored_layout: layout,
```

- [ ] **Step 5: Use the persisted sizes in `ensure_dock_area`**

Inside `if self.dock_area.is_none() {` (`window.rs:6626`), immediately after `let weak_shell = ...`:

```rust
            // B9: resolve the mount sizes once. Sizes are decided HERE and
            // never re-set — `set_*_dock` runs `subscribe_item`, which pushes
            // onto `_subscriptions` and recurses, and nothing ever removes
            // them (B6), so resizing by re-setting a dock would leak forever.
            let viewport = window.viewport_size();
            let layout = self.restored_layout.clone();
            let restored_size = |pick: fn(&crate::session::dock_layout::DockLayout) -> Option<u32>,
                                 default_size: f32,
                                 axis_extent: gpui::Pixels| {
                crate::session::dock_layout::clamped_size(
                    layout.as_ref().and_then(pick),
                    default_size,
                    f32::from(axis_extent),
                )
            };
            let right_width = restored_size(
                |l| l.right_size,
                INSPECTOR_DOCK_WIDTH + CHARTS_DOCK_WIDTH,
                viewport.width,
            );
            let left_width = restored_size(|l| l.left_size, LEFT_DOCK_WIDTH, viewport.width);
```

Then replace the two `set_*_dock` size arguments. At `:6676-6679`:

```rust
            let want = (self.inspector_panel_visible, self.chart_panel_visible);
            dock.update(cx, |dock, cx| {
                dock.set_right_dock(
                    right,
                    Some(gpui::px(right_width)),
                    want.0 || want.1,
                    window,
                    cx,
                );
            });
```

and at `:6741-6743`:

```rust
            dock.update(cx, |dock, cx| {
                dock.set_left_dock(left, Some(gpui::px(left_width)), left_open, window, cx);
            });
```

- [ ] **Step 6: Run the tests**

```bash
cargo test -p dat0-app --features a11y-capture --test dock_layout_persist
cargo test -p dat0-app --features a11y-capture --test left_dock --test right_dock --test bottom_dock
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: all PASS. The three dock suites are the regression gate — they boot fresh sessions with no layout, so they must be untouched by this change.

- [ ] **Step 7: Prove non-vacuity by perturbing the MECHANISM**

Temporarily hardcode `left_width` back to `LEFT_DOCK_WIDTH`. `a_persisted_size_is_honoured_at_mount` must go RED while `no_persisted_layout_means_the_mount_constants` stays green. Then temporarily set `DOCK_MAX_AXIS_FRACTION` to a huge value and confirm `an_absurd_persisted_size_is_clamped_not_obeyed` goes RED. Revert both, `touch`, re-run.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/dock_layout_persist.rs
git commit -s -F - <<'EOF'
feat(theme): B9 T4 — restore dock visibility and sizes at mount

The constructor seeds the visibility bools (render reads them before
ensure_dock_area runs) and stashes the layout for ensure_dock_area, which
feeds the clamped sizes into the set_*_dock calls it already makes. Those
calls still run exactly once each -- sizes are decided at mount and never
re-set, because re-setting a dock leaks subscriptions (B6).

Sizes are clamped against the live viewport so a layout saved on a large
display cannot restore into an unusable window.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 5: Restore the console open

**Files:**
- Modify: `crates/dat0-app/src/window.rs` (extract the console mount out of `toggle_sql_console` at `:3086-3175`; call it from `ensure_dock_area`)
- Modify: `crates/dat0-app/tests/dock_layout_persist.rs`

**Interfaces:**
- Consumes: `restored_layout` (Task 4).
- Produces: `WorkspaceShell::mount_sql_console(&mut self, dock, window, cx)` — builds the console, subscribes, registers the close hook and calls `set_bottom_dock` exactly once.

⚠ **If T0 probe 3 fired its stop clause**, do not mount during `ensure_dock_area`; instead call the same helper from a `window.defer` scheduled at the end of `ensure_dock_area`, and record the change here.

- [ ] **Step 1: Write the failing test**

Append to `crates/dat0-app/tests/dock_layout_persist.rs`:

```rust
#[gpui::test]
#[serial]
fn a_persisted_open_console_comes_back(cx: &mut TestAppContext) {
    use dat0_app::session::dock_layout::DockLayout;
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx, _s) = reopen_with_layout(
        cx,
        DockLayout {
            console_open: true,
            bottom_size: Some(420),
            ..DockLayout::default()
        },
    );

    let dock = shell
        .read_with(&vcx.cx, |ws, _| ws.dock_area_for_test())
        .expect("dock area exists");
    assert!(
        dock.read_with(&vcx.cx, |d, app| d.is_dock_open(DockPlacement::Bottom, app)),
        "the console dock reopened"
    );
    let size = vcx.read(|app| {
        let v = serde_json::to_value(dock.read(app).dump(app)).unwrap();
        dat0_app::session::dock_layout::mirror_from_dump(&v)
            .bottom_dock
            .expect("bottom dock")
            .size
    });
    assert_eq!(size, 420.0, "the persisted console height was honoured");
}

#[gpui::test]
#[serial]
fn a_fresh_session_still_mounts_no_console(cx: &mut TestAppContext) {
    // B8 mounts the bottom dock lazily so a user who never opens the console
    // never sees upstream's 29px collapsed title bar — and so the first-run
    // hero is untouched. Restoring must not cost that.
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let (shell, vcx) = boot(cx);
    let dock = shell.read_with(&vcx.cx, |ws, _| ws.dock_area_for_test());
    if let Some(dock) = dock {
        assert!(
            !dock.read_with(&vcx.cx, |d, app| d.has_dock(DockPlacement::Bottom)),
            "no bottom dock exists at all on a fresh session"
        );
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p dat0-app --features a11y-capture --test dock_layout_persist a_persisted_open_console
```

Expected: FAIL — `the console dock reopened`.

- [ ] **Step 3: Extract the console mount**

In `window.rs`, lift the body of `toggle_sql_console`'s `if self.sql_console.is_none() { ... }` block into a new method, keeping every existing comment with the code it explains:

```rust
    /// B8/B9: build the SQL console, subscribe to it, register the close-flush
    /// hook and mount the bottom dock. Idempotent — returns immediately if the
    /// console already exists.
    ///
    /// Extracted from `toggle_sql_console` at B9 because the restore path needs
    /// the same mount from a second call site. `dock` is passed in rather than
    /// re-derived: one caller is `ensure_dock_area` itself, which is mid-build
    /// and must not re-enter.
    pub(crate) fn mount_sql_console(
        &mut self,
        dock: &gpui::Entity<gpui_component::dock::DockArea>,
        height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sql_console.is_some() {
            return;
        }
        // ... the existing body, verbatim, with `SQL_CONSOLE_DOCK_HEIGHT`
        // replaced by `height` in the `set_bottom_dock` call ...
    }
```

`toggle_sql_console` then becomes:

```rust
    pub(crate) fn toggle_sql_console(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dock = self.ensure_dock_area(window, cx);
        if self.sql_console.is_none() {
            self.mount_sql_console(&dock, SQL_CONSOLE_DOCK_HEIGHT, window, cx);
            return;
        }
        // ... the existing else/toggle path, unchanged ...
    }
```

⚠ Keep `toggle_sql_console`'s post-mount behaviour identical. If the existing code does work after the `if` block that must also run on first mount, it stays in `toggle_sql_console` and runs after the call — read the current body carefully rather than assuming the `if` block is the whole first-mount path.

- [ ] **Step 4: Call it from the restore path**

At the end of the `if self.dock_area.is_none() { ... }` block in `ensure_dock_area`, after `self.dock_area = Some(dock);`:

```rust
            // B9: restore an open console. The bottom dock stays LAZY for
            // everyone else — B8 mounts it on first open so a user who never
            // opens the console never sees upstream's 29px collapsed title bar,
            // and the first-run hero has no persisted layout, so the hero is
            // untouched.
            if let Some(l) = self.restored_layout.clone().filter(|l| l.console_open) {
                let height =
                    restored_size(|_| l.bottom_size, SQL_CONSOLE_DOCK_HEIGHT, viewport.height);
                let dock = self.dock_area.clone().expect("set directly above");
                self.mount_sql_console(&dock, height, window, cx);
            }
```

- [ ] **Step 5: Run the tests**

```bash
cargo test -p dat0-app --features a11y-capture --test dock_layout_persist
cargo test -p dat0-app --features a11y-capture --test bottom_dock --test sql_console_nav --test sql_console_transient_nav --test input_nav --test a11y_spike
```

Expected: all PASS, `a11y_spike` still asserting **12**.

⚠ `sql_console_nav` (9), `sql_console_transient_nav` (17) and `input_nav` (7) all gate the app's biggest focus surface. B7 proved a docked panel's stops stay reachable **in document order** — treat any ordering change as a real regression, not a test to update.

- [ ] **Step 6: Prove non-vacuity by perturbing the MECHANISM**

Temporarily change the restore condition to `.filter(|_| false)`. `a_persisted_open_console_comes_back` must go RED while `a_fresh_session_still_mounts_no_console` stays green. Revert, `touch`, re-run.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/tests/dock_layout_persist.rs
git commit -s -F - <<'EOF'
feat(theme): B9 T5 — restore an open SQL console at its persisted height

Extracts the console mount out of toggle_sql_console so the restore path can
reuse it rather than copy it, then calls it from ensure_dock_area when the
persisted layout says the console was open.

The bottom dock stays lazy for everyone else: B8 mounts it on first open so a
user who never opens the console never sees upstream's 29px collapsed title
bar, and the first-run hero carries no persisted layout.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 6: Settings v2 → v3 — the new-window seed

**Files:**
- Modify: `crates/dat0-app/src/settings/schema.rs`
- Modify: `crates/dat0-app/src/window.rs` (`update_ui_settings` + the seed write in `flush_focused_workspace_sql`; ctor fallback)
- Modify: `crates/dat0-app/tests/dock_layout_persist.rs`

**Interfaces:**
- Consumes: `DockLayout`, `de_tolerant_toml` (Task 1).
- Produces: `Settings.ui: UiSettings`, `UiSettings.dock_layout: Option<DockLayout>`.

- [ ] **Step 1: Write the failing tests**

Append to the `mod tests` block in `crates/dat0-app/src/settings/schema.rs`:

```rust
    #[test]
    fn default_schema_version_is_3() {
        assert_eq!(Settings::default().schema_version, 3);
    }

    #[test]
    fn a_v2_settings_file_defaults_the_ui_section() {
        let toml = "schema_version = 2\n";
        let s: Settings = toml::from_str(toml).expect("v2 still parses");
        assert!(s.ui.dock_layout.is_none());
    }

    #[test]
    fn a_malformed_ui_section_never_resets_the_users_theme() {
        let toml = "schema_version = 3\n[theme]\nname = \"high-contrast\"\n\
                    [ui]\ndock_layout = \"corrupt\"\n";
        let s: Settings = toml::from_str(toml).expect("a bad layout must not fail the document");
        assert_eq!(s.theme.name, "high-contrast");
        assert!(s.ui.dock_layout.is_none());
    }

    #[test]
    fn a_layout_round_trips_through_toml() {
        let mut s = Settings::default();
        s.ui.dock_layout = Some(crate::session::dock_layout::DockLayout {
            left_panel: Some(crate::window::LeftPanel::Catalog),
            left_size: Some(500),
            console_open: true,
            ..Default::default()
        });
        let text = toml::to_string(&s).expect("serialises");
        let back: Settings = toml::from_str(&text).expect("round-trips");
        assert_eq!(back.ui.dock_layout, s.ui.dock_layout);
    }
```

And append to `crates/dat0-app/tests/dock_layout_persist.rs`:

```rust
#[gpui::test]
#[serial]
fn the_session_layout_wins_over_the_settings_seed(cx: &mut TestAppContext) {
    use dat0_app::session::dock_layout::DockLayout;
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join("cfg");
    set_config_dir(&cfg);
    init_components(cx);

    // Seed settings with the AI panel, and the session with the catalog.
    let store = dat0_app::settings::store::SettingsStore::with_path(cfg.join("settings.toml"));
    let mut settings = store.load_or_default().unwrap();
    settings.ui.dock_layout = Some(DockLayout {
        left_panel: Some(dat0_app::window::LeftPanel::Ai),
        ..DockLayout::default()
    });
    store.save(&settings).unwrap();

    let session = build_empty_session(&tmp.path().join("state"));
    session
        .lock()
        .set_dock_layout(Some(DockLayout {
            left_panel: Some(dat0_app::window::LeftPanel::Catalog),
            ..DockLayout::default()
        }))
        .unwrap();
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    assert_eq!(
        shell.read_with(&vcx.cx, |ws, _| ws.open_left_panel()),
        Some(dat0_app::window::LeftPanel::Catalog),
        "the session is authoritative when it has a layout of its own"
    );
    std::mem::forget(tmp);
}

#[gpui::test]
#[serial]
fn a_fresh_session_inherits_the_settings_seed(cx: &mut TestAppContext) {
    use dat0_app::session::dock_layout::DockLayout;
    let h = enter_async_harness(cx);
    let _g = h.enter();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = tmp.path().join("cfg");
    set_config_dir(&cfg);
    init_components(cx);

    let store = dat0_app::settings::store::SettingsStore::with_path(cfg.join("settings.toml"));
    let mut settings = store.load_or_default().unwrap();
    settings.ui.dock_layout = Some(DockLayout {
        left_panel: Some(dat0_app::window::LeftPanel::Ai),
        ..DockLayout::default()
    });
    store.save(&settings).unwrap();

    // A plain launch: Session::new leaves session.json with no layout.
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();

    assert_eq!(
        shell.read_with(&vcx.cx, |ws, _| ws.open_left_panel()),
        Some(dat0_app::window::LeftPanel::Ai),
        "a brand-new scratch session starts from the last-used layout"
    );
    std::mem::forget(tmp);
}
```

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p dat0-app --lib settings::schema
```

Expected: FAIL — `no field ui on type Settings`.

- [ ] **Step 3: Add the settings section**

In `crates/dat0-app/src/settings/schema.rs`, add the field to `Settings` after `first_run_done`:

```rust
    /// B9: UI state that is a user preference rather than window state. Absent
    /// in pre-v3 settings.toml → defaults.
    #[serde(default)]
    pub ui: UiSettings,
```

`ui: UiSettings::default(),` goes in the `Default` impl, and `schema_version: 3`. Then:

```rust
/// Global UI preferences (v3+, B9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    /// The last-used dock layout, used as the seed for a window whose session
    /// has no layout of its own — a plain launch creates a fresh scratch
    /// session, so without this the layout would never come back outside a
    /// workspace or a recovered session.
    ///
    /// Tolerant on read: a malformed value degrades to `None` rather than
    /// failing the whole file, which would reset the user's theme, AI config
    /// and telemetry choice along with it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::session::dock_layout::de_tolerant_toml"
    )]
    pub dock_layout: Option<crate::session::dock_layout::DockLayout>,
}
```

- [ ] **Step 4: Write the seed on close, and read it in the constructor**

In `window.rs`, next to `update_ai_settings` (`:5768`), add its sibling:

```rust
    /// B9: mirror the live dock layout into settings.toml as the seed a fresh
    /// scratch window starts from. Load → mutate → atomic save, the same shape
    /// as `update_ai_settings`.
    ///
    /// ⚠ Called ONLY from the close/quit flush, never from a toggle.
    /// `SettingsWatcher` re-reads settings.toml on every write
    /// (`settings/watcher.rs:20-26`); the callback is benign — it swaps the
    /// in-memory `Settings` under an `RwLock` and re-applies nothing — but this
    /// file is otherwise written only on deliberate user action, and writing it
    /// on every dock toggle would widen the window in which a load-mutate-save
    /// clobbers a hand-edit in flight. The seed only has to be right at quit.
    fn persist_dock_layout_seed(&self, cx: &gpui::App) {
        let Some(store) = Self::ai_settings_store() else {
            return;
        };
        let mut settings = match store.load_or_default() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?e, "persist_dock_layout_seed: load failed; seed not updated");
                return;
            }
        };
        settings.ui.dock_layout = Some(self.current_dock_layout(cx));
        if let Err(e) = store.save(&settings) {
            tracing::warn!(?e, "persist_dock_layout_seed: save failed; seed not updated");
        }
    }
```

(If `ai_settings_store()` is named for AI only, rename it to `settings_store()` in the same commit and update its two existing callers — it already returns a plain `SettingsStore`.)

Extend the flush from Task 3:

```rust
    ws.update(cx, |ws, cx| {
        ws.persist_sql_console(cx);
        ws.persist_dock_layout(cx);
        ws.persist_dock_layout_seed(cx);
    });
```

In the constructor, extend the layout resolution from Task 4:

```rust
        // B9 precedence: the session is authoritative when it carries a layout;
        // otherwise fall back to the settings seed, so a plain launch — which
        // always creates a FRESH scratch session — still opens the way the user
        // left it. Neither present → the mount constants.
        let layout = session.lock().dock_layout().cloned().or_else(|| {
            Self::ai_settings_store()
                .and_then(|s| s.load_or_default().ok())
                .and_then(|s| s.ui.dock_layout)
        });
```

- [ ] **Step 5: Run the tests**

```bash
cargo test -p dat0-app --lib settings::
cargo test -p dat0-app --features a11y-capture --test dock_layout_persist
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: all PASS.

- [ ] **Step 6: Prove non-vacuity by perturbing the MECHANISM**

Temporarily drop the `.or_else(..)` from the constructor: `a_fresh_session_inherits_the_settings_seed` must go RED while `the_session_layout_wins_over_the_settings_seed` stays green — that pair is what pins the precedence rule in both directions. Revert, `touch`, re-run.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/settings/schema.rs crates/dat0-app/src/window.rs crates/dat0-app/tests/dock_layout_persist.rs
git commit -s -F - <<'EOF'
feat(theme): B9 T6 — settings v2 to v3, the new-window layout seed

A plain launch calls Session::new, which creates a fresh scratch dir with an
empty session.json, so a session-only layout would never return outside a
workspace or a recovered session. The layout is mirrored into settings.toml
as the seed a fresh window starts from; the session still wins when it has a
layout of its own.

The seed is written only on the close/quit flush, never per toggle:
SettingsWatcher re-reads the file on every write, and settings.toml is
otherwise written only on deliberate user action.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 7: Tear down the hybrid — remove the two v10 bools

**Files:**
- Modify: `crates/dat0-app/src/session/mod.rs` (`SessionUiState`)
- Modify: `crates/dat0-app/src/window.rs` (`persist_dock_ui` at `:4312`, constructor)

**Interfaces:**
- Consumes: everything from Tasks 2, 4 and 6.
- Produces: `SessionUiState { catalog_collapsed }` only.

This is A1's hybrid teardown: both sources have coexisted since Task 2 so every commit built; now the old one goes.

- [ ] **Step 1: Update the tests that name the removed fields**

```bash
grep -rn "catalog_panel_visible\|inspector_panel_visible" crates/dat0-app/src crates/dat0-app/tests
```

Every hit on `SessionUiState`'s fields (as opposed to the shell's identically-named bools, which stay) must move to `dock_layout`. The session round-trip test at `session/mod.rs:1032-1050` is the main one: rewrite it to set and read back `dock_layout` instead.

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p dat0-app --lib session::
```

Expected: FAIL to compile — the tests now reference fields that still exist. That is the point: the test change comes first.

- [ ] **Step 3: Remove the fields**

In `crates/dat0-app/src/session/mod.rs`, reduce `SessionUiState` and rewrite its doc comment:

```rust
/// Persisted catalog tree UI state (v8+, P6a; reshaped v10; reduced v11).
///
/// v11 (B9) moved `catalog_panel_visible` and `inspector_panel_visible` into
/// `SessionState::dock_layout`, which owns all dock visibility now. Serde drops
/// the old keys silently on load — which is exactly why `migrate_v10_to_v11`
/// reads them from the RAW document before this struct ever sees them.
///
/// What remains is catalog TREE state, not dock layout: the collapsed
/// attach-parent aliases, sorted (deterministic wire format).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionUiState {
    #[serde(default)]
    pub catalog_collapsed: Vec<String>,
}
```

- [ ] **Step 4: Update the two writers**

`persist_dock_ui` (`window.rs:4312`) loses both fields and gains a sharper doc comment:

```rust
    /// Persist the catalog TREE state to `session.json` (P6a T13; v10 added the
    /// collapse set; v11 moved dock visibility out to `persist_dock_layout`).
    /// Sorted for a deterministic wire format (the insta snapshot gates it).
    pub(crate) fn persist_dock_ui(&self) {
        let mut catalog_collapsed: Vec<String> = self.catalog_collapsed.iter().cloned().collect();
        catalog_collapsed.sort();
        let ui = crate::session::SessionUiState { catalog_collapsed };
        if let Err(e) = self.session.lock().set_ui(ui) {
            tracing::warn!(error = %e, "persist_dock_ui: set_ui failed");
        }
    }
```

In the constructor, drop the two `match layout { .. None => ui.* }` fallbacks added in Task 4 — with the v10 fields gone there is nothing to fall back to, and the precedence chain already ends at the mount constants:

```rust
            catalog_panel_visible: layout
                .as_ref()
                .is_some_and(|l| l.left_panel == Some(LeftPanel::Catalog)),
```

```rust
            inspector_panel_visible: layout.as_ref().is_some_and(|l| l.inspector_visible),
```

- [ ] **Step 5: Run the full suite**

```bash
cargo test -p dat0-app --lib
cargo test -p dat0-app --features a11y-capture 2>&1 | tee /tmp/b9-t7.log
grep -c "test result: ok" /tmp/b9-t7.log
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: **117** `test result: ok` lines (116 as of B8, plus `dock_layout_persist`; `dock_layout_spike` makes 118 — confirm the exact count and record it), 0 failures.

⚠ Do NOT pipe the count through `head` — it SIGPIPEs cargo mid-write and truncates the output (A6's finding). Redirect to a file and count there, as above.

- [ ] **Step 6: Check the insta snapshot**

If a snapshot covers `session.json`'s wire form, it changes here (two keys leave `ui`, one arrives at top level). Review the diff and accept it deliberately:

```bash
cargo insta review
```

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/session/mod.rs crates/dat0-app/src/window.rs crates/dat0-app/tests/
git commit -s -F - <<'EOF'
feat(theme): B9 T7 — retire the v10 dock bools from SessionUiState

The hybrid ends: both sources have coexisted since T2 so every commit built,
and dock_layout is now the only home for dock visibility. SessionUiState keeps
only catalog_collapsed, which is catalog TREE state rather than dock layout.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 8: Correct the panel-builder doc comments, and the as-built record

**Files:**
- Modify: `crates/dat0-app/src/panels/mod.rs`
- Modify: `docs/plans/2026-08-04-dat0-ui-redesign-b9-dock-layout-design.md` (append §15)

**Interfaces:** none — documentation and the slice record.

- [ ] **Step 1: Rewrite the module and builder doc comments**

`src/panels/mod.rs` currently promises three times that "B9 replaces all seven with builders that resolve the live shell". That promise was premised on B9 calling `DockArea::load`; it does not. Replace the module doc's closing sentence and the `register_panels` doc with:

```rust
/// Register every dat0 panel with gpui-component's global `PanelRegistry`.
///
/// Called from `run_app` AND from each test binary's `init_components`: a
/// registration performed only in production is silently absent under test
/// (the `register_modal_keys` lesson from B1/B2).
///
/// ⚠ **These builders are unreachable, deliberately, and B9 is where that was
/// decided.** `PanelRegistry::build_panel` is called from exactly one place —
/// `PanelState::to_item` (`dock/state.rs:227-236`), which runs only from
/// `DockArea::load`. B9 does not call `load`: `DockAreaState::center` is a
/// non-`Option` field and `load` unconditionally rebuilds it through `to_item`,
/// which re-wraps a `PanelInfo::Panel` as `DockItem::tabs` — restoring the 30px
/// title bar B5 chose `DockItem::Panel` to avoid. There is no partial-load API.
/// dat0 persists its layout as a typed `DockLayout` instead (see
/// `session/dock_layout.rs`).
///
/// The registration stays because `panel_name` is what a future
/// drag-rearrange slice would resolve panels through, and because that slice
/// would have to solve `load`'s centre problem first — at which point these
/// builders become its natural seam. Until then they hand back a shell-less
/// panel rather than panicking: the `WeakEntity::new_invalid()` upgrade fails
/// and the panel paints an empty div, which degrades gracefully instead of
/// arming a landmine.
```

Delete the trailing "B9 replaces all seven…" sentence from the B8 console comment and replace it with a pointer to the paragraph above.

- [ ] **Step 2: Write the as-built section**

Append `## 15. Slice as-built` to the design doc: every deviation from this plan with its reason, the final test-binary count, the T0 stop clauses that fired (if any), and anything a future slice should not re-derive.

- [ ] **Step 3: Run the full local gate**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app > /tmp/b9-plain.log 2>&1; grep -c "test result: ok" /tmp/b9-plain.log
cargo test -p dat0-app --features a11y-capture > /tmp/b9-a11y.log 2>&1; grep -c "test result: ok" /tmp/b9-a11y.log
cargo test -p dat0-app --features a11y-capture,gallery > /tmp/b9-gal.log 2>&1; grep -c "test result: ok" /tmp/b9-gal.log
cargo test -p dat0-app --test style_lint
git diff main --stat -- crates/dat0-app/src/grid
cargo build -p dat0-app --bin dat0
```

Expected: fmt clean; clippy exit 0; equal binary counts across all three feature combos with 0 failures; `style_lint` 4/4 with the ratchet still `[("window.rs", 1)]`; the `src/grid` diff **empty**; the binary builds.

⚠ `cargo test --workspace` and `cargo bench` remain unrunnable on this machine (macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift — reproduces on `main`, so verify with `git checkout main` before blaming the branch).

- [ ] **Step 4: Boot the binary and diff the log against a `main` build**

This is how B5's tour regression was found — no test caught it, and a silent success logs nothing, so "no line on main, a WARN on the branch" was the entire signal.

```bash
DAT0_CONFIG_DIR=/tmp/dat0-b9-boot ./target/debug/dat0 2>&1 | tee /tmp/b9-branch.log
# then, on a CLEAN tree — never bracket this in git stash/pop, which pops an
# UNRELATED pre-existing stash when the tree was already clean (B7's finding):
git checkout main && cargo build -p dat0-app --bin dat0
DAT0_CONFIG_DIR=/tmp/dat0-main-boot ./target/debug/dat0 2>&1 | tee /tmp/b9-main.log
git checkout feat/ui-redesign-b9-dock-layout
diff /tmp/b9-main.log /tmp/b9-branch.log
```

Expected: no new WARN or ERROR lines on the branch.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/panels/mod.rs docs/plans/2026-08-04-dat0-ui-redesign-b9-dock-layout-design.md
git commit -s -F - <<'EOF'
docs(theme): B9 T8 — correct the panel-builder contract, record the as-built

The seven builders' doc comments promised that B9 would make them resolve the
live shell. That was premised on B9 calling DockArea::load, which it does not
-- load always rebuilds the centre through to_item and would restore the 30px
title bar B5 chose DockItem::Panel to avoid. The comments now state what is
actually true and what a future drag-rearrange slice would have to solve first.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Self-review

**Spec coverage.** Design §1 scope → T2/T3/T4/T5/T6. §2 verified facts → T0 probes 1-2 (facts 1, 3, 4), T4 (fact 6), T0 probe 3 (the B7 hazard). §3.1 no-`load` → T8 records it; T1's `DumpMirror` enforces it structurally. §3.2 data model → T1. §3.3 two homes + precedence → T6, with a test in each direction. §3.4 capture + trigger table → T3 (three toggles + flush), T6 (seed on flush only). §3.5 restore → T4, T5. §3.6 failure modes → T1 (clamping, tolerant parse), T2 (session field), T6 (settings field). §3.7 migration → T2, including the raw-read requirement. §4 T0 gate → T0. §5 tests → every task. §6 local gate → T8 step 3-4. §7 builders → T8. §9 human glance → carried into the as-built.

**Placeholder scan.** One intentional placeholder remains: `__PASTE_FROM_T0_STEP_2__` in T1's fixture, which T0 step 2 produces and T1 step 5 fills. It is intentional because a hand-written fixture would prove only that the parser matches my guess at the dump shape rather than the real one. Two steps carry a conditional ("if T0 probe 3 fired its stop clause…", "if `ai_settings_store` is named for AI only…") — both name the exact alternative rather than deferring the decision.

**Type consistency.** `DockLayout` field names are identical across T1 (definition), T2 (migration), T3 (capture), T4/T5 (restore) and T6 (settings). `clamped_size(Option<u32>, f32, f32) -> f32` and `size_to_persist(f32) -> Option<u32>` are used with those exact signatures in T3 and T4. `mirror_from_dump(&serde_json::Value) -> DumpMirror` is used identically in T3 and in the T4/T5 test assertions. `LeftPanel` is the same type in the session module, the settings module and the shell — there is no parallel enum anywhere.

**Ordering.** Every commit builds: T2 keeps the v10 bools so the shell still compiles, and T7 removes them only after T4 and T6 have given the constructor another source. T5 depends on T0 probe 3 and says what to do if it failed.
