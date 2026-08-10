//! Dock splitters (S5).
//!
//! Three resizable edges — the sidebar, the right column, the console — each a
//! 4px track that the pointer drags.
//!
//! **Event-driven, not polled.** The GPUI shell discovered dock resizes by
//! serializing `DockArea::dump()` every frame and diffing it
//! (`window/dock.rs:182-211`), because `Dock` emitted nothing on resize. Here
//! the drag *is* the event: mousedown opens a gesture, the shield reports every
//! move, mouseup ends it. Nothing runs when nothing is being dragged.
//!
//! The shield is the same device the grid's column resize uses: one
//! full-window overlay that captures the move and the release, so a drag that
//! leaves the 4px track — which every real drag does — keeps tracking without
//! a `document`-level JS listener to install, leak or hide from tests.

use dioxus::prelude::*;

use dat0_core::session::dock_layout::{DOCK_MAX_AXIS_FRACTION, DOCK_MIN_SIZE};

/// Which edge a drag is resizing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Edge {
    /// Left column. Grows rightwards, so `+dx` widens it.
    Sidebar,
    /// Right column. Grows leftwards, so `-dx` widens it.
    Right,
    /// Console. Grows upwards, so `-dy` heightens it.
    Bottom,
}

impl Edge {
    /// The gesture's axis: true for horizontal (a width).
    pub fn is_horizontal(self) -> bool {
        !matches!(self, Edge::Bottom)
    }

    /// How a pointer delta maps to a size delta on this edge.
    pub fn size_delta(self, dx: f64, dy: f64) -> f64 {
        match self {
            Edge::Sidebar => dx,
            Edge::Right => -dx,
            Edge::Bottom => -dy,
        }
    }
}

/// A live splitter gesture.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SplitDrag {
    pub edge: Edge,
    /// Pointer position where the gesture started.
    pub origin: (f64, f64),
    /// The edge's size when the gesture started.
    pub start: u32,
}

/// Apply a pointer delta to a starting size, clamped to the same band a
/// restored layout is clamped to.
///
/// `extent` is the window's size on the gesture's axis. A degenerate extent —
/// which a window reports before its first layout — disables the upper bound
/// rather than inverting the band, exactly as `clamped_size` does: `f32::clamp`
/// panics when `min > max`.
pub fn resized(edge: Edge, start: u32, dx: f64, dy: f64, extent: f64) -> u32 {
    let next = start as f64 + edge.size_delta(dx, dy);
    let max = if extent.is_finite() && extent > 0.0 {
        (extent * f64::from(DOCK_MAX_AXIS_FRACTION)).max(f64::from(DOCK_MIN_SIZE))
    } else {
        f64::from(u32::MAX)
    };
    next.clamp(f64::from(DOCK_MIN_SIZE), max).round() as u32
}

/// A 4px drag track.
#[component]
pub fn Splitter(edge: Edge, id: String, size: u32, drag: Signal<Option<SplitDrag>>) -> Element {
    let mut drag = drag;
    rsx! {
        div {
            class: if edge.is_horizontal() { "d0-splitter is-col" } else { "d0-splitter is-row" },
            "data-a11y-id": "{id}",
            role: "separator",
            "aria-orientation": if edge.is_horizontal() { "vertical" } else { "horizontal" },
            onmousedown: move |e| {
                e.prevent_default();
                let c = e.data().client_coordinates();
                drag.set(Some(SplitDrag { edge, origin: (c.x, c.y), start: size }));
            },
        }
    }
}

/// The full-window capture surface, rendered only while a gesture is live.
///
/// `extent` is the window's `(width, height)`, supplied by the shell — the
/// shield cannot measure it from a mouse event, and the clamp needs it to keep
/// the centre from being dragged away.
#[component]
pub fn DragShield(
    drag: Signal<Option<SplitDrag>>,
    extent: (f64, f64),
    on_size: EventHandler<(Edge, u32)>,
) -> Element {
    let mut drag = drag;
    rsx! {
        div {
            class: "d0-drag-shield",
            "data-a11y-id": "dock-drag-shield",
            style: if drag.read().is_some_and(|d| d.edge.is_horizontal()) {
                "cursor: col-resize"
            } else {
                "cursor: row-resize"
            },
            onmousemove: move |e| {
                let Some(d) = *drag.read() else { return };
                let c = e.data().client_coordinates();
                let axis = if d.edge.is_horizontal() { extent.0 } else { extent.1 };
                let next = resized(d.edge, d.start, c.x - d.origin.0, c.y - d.origin.1, axis);
                on_size.call((d.edge, next));
            },
            onmouseup: move |_| drag.set(None),
            onmouseleave: move |_| drag.set(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_edge_grows_in_its_own_direction() {
        // Dragging right widens the sidebar and narrows the right column; the
        // signs are the entire content of this mapping and a flipped one is
        // invisible until someone drags.
        assert_eq!(Edge::Sidebar.size_delta(40.0, 0.0), 40.0);
        assert_eq!(Edge::Right.size_delta(40.0, 0.0), -40.0);
        assert_eq!(Edge::Bottom.size_delta(0.0, 40.0), -40.0);
    }

    #[test]
    fn a_drag_cannot_collapse_or_swallow_the_window() {
        // Below the minimum it pins, not disappears.
        assert_eq!(resized(Edge::Sidebar, 238, -400.0, 0.0, 1400.0), 100);
        // Above 80% of the axis it pins too, so the centre always survives.
        assert_eq!(resized(Edge::Sidebar, 238, 5000.0, 0.0, 1000.0), 800);
    }

    #[test]
    fn a_window_with_no_measured_extent_still_resizes() {
        // A degenerate extent disables the upper bound rather than inverting
        // the clamp band, which would panic.
        assert_eq!(resized(Edge::Right, 320, -80.0, 0.0, 0.0), 400);
        assert_eq!(resized(Edge::Bottom, 260, 0.0, -60.0, f64::NAN), 320);
    }
}
