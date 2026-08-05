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
/// Every `Option` carries `skip_serializing_if`: the `toml` serializer errors
/// with `UnsupportedNone` on a `None` inside a table, so the settings half
/// would fail to write without it.
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
    /// The left dock is open iff a panel is chosen. Derived, never stored — the
    /// master plan's standing rule for B6+ is to derive dock state and never
    /// keep parallel bools.
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
/// so `size` arrives as a bare number — confirmed live by B9's T0 probe 2.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct DockSlot {
    pub size: f32,
    pub open: bool,
}

/// The only part of a `DockArea::dump()` dat0 reads.
///
/// Every other key — the whole `center`, every nested `PanelState`, the
/// top-level `version`, each dock's `placement` — has no field to land in and is
/// discarded by construction. T0 probe 2 confirmed the dump really does carry a
/// `center` whose `panel_name` is `"GridPanel"`, so "docks only" is a live
/// constraint here, not a theoretical one.
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
///
/// Degrades to an empty mirror rather than erroring: a layout read must never be
/// able to take down the caller.
pub fn mirror_from_dump(dump: &serde_json::Value) -> DumpMirror {
    serde_json::from_value(dump.clone()).unwrap_or_default()
}

/// Tolerant field deserializer for **session.json**.
///
/// A malformed `dock_layout` degrades to `None` — the default layout — while
/// tabs, SQL tabs, history, saved queries, charts and attachments still load.
/// Layout is the least valuable thing in the file and must never be able to cost
/// a user their work.
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
///
/// A malformed `[ui]` section must not reset the user's theme, AI configuration
/// and telemetry choice along with it — `SettingsStore::load_or_default` falls
/// back to a fully default `Settings` on any parse error.
pub fn de_tolerant_toml<'de, D>(d: D) -> Result<Option<DockLayout>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = toml::Value::deserialize(d)?;
    Ok(value.try_into().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // f32::clamp panics if min > max or either bound is NaN. A headless
        // window can report a zero viewport, so this is a real input rather
        // than a fuzz case.
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
        assert!(
            left.left_open(),
            "a chosen left panel means the dock is open"
        );

        let right = DockLayout {
            charts_visible: true,
            ..DockLayout::default()
        };
        assert!(
            right.right_open(),
            "either right panel opens the right dock"
        );
    }

    #[test]
    fn the_wire_form_omits_absent_options_and_names_the_panel() {
        let l = DockLayout {
            left_panel: Some(LeftPanel::Connections),
            inspector_visible: true,
            ..DockLayout::default()
        };
        // toml FIRST: it is the format that actually enforces the omission, so
        // it is the assertion that must fail if `skip_serializing_if` is ever
        // dropped. The `toml` serializer errors with `UnsupportedNone` on a
        // `None` inside a table, which would make settings.toml unwritable —
        // verified by deleting the attribute and watching this line, not the
        // JSON one below, go red.
        toml::to_string(&l).expect(
            "DockLayout must be toml-serialisable — every Option field needs \
             skip_serializing_if or the settings half cannot be written",
        );

        let json = serde_json::to_value(&l).unwrap();
        assert_eq!(json["left_panel"], "connections");
        assert!(
            json.get("left_size").is_none(),
            "None is OMITTED, not null: {json:#}"
        );
    }

    /// The payload captured live by T0 probe 1 — a REAL `DockArea::dump()` of
    /// the production dock tree, not a hand-written guess at its shape. A
    /// fixture I invented would only prove the parser matches my guess.
    const REAL_DUMP: &str = r#"{
      "version": 1,
      "center": {"panel_name": "GridPanel", "children": [], "info": {"panel": null}},
      "left_dock": {"panel": {"panel_name": "StackPanel", "children": [
          {"panel_name": "TabPanel", "children": [{"panel_name": "CatalogPanel", "children": [], "info": {"panel": null}}], "info": {"tabs": {"active_index": 0}}},
          {"panel_name": "TabPanel", "children": [{"panel_name": "ConnectionsPanel", "children": [], "info": {"panel": null}}], "info": {"tabs": {"active_index": 0}}},
          {"panel_name": "TabPanel", "children": [{"panel_name": "AiDockPanel", "children": [], "info": {"panel": null}}], "info": {"tabs": {"active_index": 0}}}
        ], "info": {"stack": {"sizes": [384.0, 384.0, 384.0], "axis": 0}}},
        "placement": "left", "size": 384.0, "open": false},
      "right_dock": {"panel": {"panel_name": "StackPanel", "children": [
          {"panel_name": "TabPanel", "children": [{"panel_name": "InspectorPanel", "children": [], "info": {"panel": null}}], "info": {"tabs": {"active_index": 0}}},
          {"panel_name": "TabPanel", "children": [{"panel_name": "ChartsPanel", "children": [], "info": {"panel": null}}], "info": {"tabs": {"active_index": 0}}}
        ], "info": {"stack": {"sizes": [288.0, 560.0], "axis": 0}}},
        "placement": "right", "size": 848.0, "open": false},
      "bottom_dock": {"panel": {"panel_name": "TabPanel", "children": [{"panel_name": "SqlConsolePanel", "children": [], "info": {"panel": null}}], "info": {"tabs": {"active_index": 0}}},
        "placement": "bottom", "size": 320.0, "open": true}
    }"#;

    #[test]
    fn the_mirror_reads_a_real_dump() {
        let v: serde_json::Value = serde_json::from_str(REAL_DUMP).unwrap();
        let m = mirror_from_dump(&v);
        assert_eq!(m.left_dock.map(|d| (d.size, d.open)), Some((384.0, false)));
        assert_eq!(m.right_dock.map(|d| (d.size, d.open)), Some((848.0, false)));
        assert_eq!(m.bottom_dock.map(|d| (d.size, d.open)), Some((320.0, true)));
    }

    #[test]
    fn the_mirror_drops_the_centre_and_every_nested_panel() {
        // "Docks only, never the centre" is structural: there is no field a
        // centre could land in, so a dump carrying one — and the real one
        // does, named "GridPanel" — cannot smuggle it through.
        let v: serde_json::Value = serde_json::from_str(REAL_DUMP).unwrap();
        assert_eq!(
            v.pointer("/center/panel_name").and_then(|c| c.as_str()),
            Some("GridPanel"),
            "the fixture really does carry a centre"
        );
        let m = mirror_from_dump(&v);
        let round_tripped = serde_json::to_value(serde_json::json!({
            "left": m.left_dock.map(|d| d.size),
            "right": m.right_dock.map(|d| d.size),
            "bottom": m.bottom_dock.map(|d| d.size),
        }))
        .unwrap();
        assert!(
            !round_tripped.to_string().contains("GridPanel"),
            "nothing about the centre survives the mirror"
        );
    }

    #[test]
    fn a_junk_dump_degrades_to_empty_rather_than_panicking() {
        assert_eq!(
            mirror_from_dump(&serde_json::json!("not an object")),
            DumpMirror::default()
        );
        assert_eq!(
            mirror_from_dump(&serde_json::json!({"left_dock": {"size": "wide"}})),
            DumpMirror::default()
        );
    }

    #[test]
    fn a_malformed_layout_parses_as_none_not_an_error() {
        #[derive(serde::Deserialize)]
        struct Holder {
            #[serde(default, deserialize_with = "de_tolerant_json")]
            dock_layout: Option<DockLayout>,
        }

        let good: Holder =
            serde_json::from_str(r#"{"dock_layout":{"console_open":true}}"#).unwrap();
        assert_eq!(good.dock_layout.map(|l| l.console_open), Some(true));

        let bad: Holder = serde_json::from_str(r#"{"dock_layout":"nonsense"}"#)
            .expect("a malformed layout must NOT fail the enclosing document");
        assert!(bad.dock_layout.is_none());

        let wrong_type: Holder =
            serde_json::from_str(r#"{"dock_layout":{"console_open":"yes"}}"#).unwrap();
        assert!(wrong_type.dock_layout.is_none());

        let absent: Holder = serde_json::from_str("{}").unwrap();
        assert!(absent.dock_layout.is_none());
    }

    #[test]
    fn the_toml_tolerant_reader_behaves_like_its_json_sibling() {
        #[derive(serde::Deserialize)]
        struct Holder {
            #[serde(default, deserialize_with = "de_tolerant_toml")]
            dock_layout: Option<DockLayout>,
        }

        let good: Holder = toml::from_str("[dock_layout]\nconsole_open = true\n").unwrap();
        assert_eq!(good.dock_layout.map(|l| l.console_open), Some(true));

        let bad: Holder = toml::from_str("dock_layout = \"nonsense\"\n")
            .expect("a malformed layout must NOT fail the enclosing document");
        assert!(bad.dock_layout.is_none());
    }
}
