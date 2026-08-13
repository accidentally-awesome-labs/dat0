//! Header hit-testing geometry.
//!
//! A header cell is four zones — grip, body, sort, funnel — and where the
//! pointer lands decides whether a click resizes, selects, sorts or filters.
//! The mapping is pure arithmetic over the cell width, so it belongs here
//! rather than inside whichever widget happens to draw the header.

/// Width of the left-edge drag-grip in logical pixels.
/// P4c (column resize) will replace the invisible stub with a real handle.
/// Kept here as a single source of truth for the geometry constant.
pub const HEADER_GRIP_PX: f32 = 6.0;

/// Width of the right-edge funnel icon hit-target in logical pixels.
pub const HEADER_FUNNEL_PX: f32 = 20.0;

/// Width of the sort-icon hit-target in logical pixels (sits left of funnel).
pub const HEADER_SORT_PX: f32 = 20.0;

/// Classify an x-offset (measured from the left edge of the header cell) into
/// a zone, given the total cell width.  Used for unit tests; the actual render
/// uses flex children rather than raw x offsets.
///
/// Zone boundaries (left → right):
///   `0 .. HEADER_GRIP_PX`                              → Grip
///   `HEADER_GRIP_PX .. (cell_width - HEADER_SORT_PX - HEADER_FUNNEL_PX)` → Body
///   `(cell_width - HEADER_SORT_PX - HEADER_FUNNEL_PX) .. (cell_width - HEADER_FUNNEL_PX)` → Sort
///   `(cell_width - HEADER_FUNNEL_PX) .. cell_width`    → Funnel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnHeaderZone {
    /// Left-edge resize grip.  Stub: no-op click (P4c column-resize will fill).
    Grip,
    /// Column-name / body click area.  Stub: no-op click (future P4b row-selection will fill).
    Body,
    /// Sort-direction toggle icon.  Live in P4a (cycles Asc/Desc via T12).
    Sort,
    /// Filter funnel icon.  Live in P4a (opens popover via T10).
    Funnel,
}

/// Map an x-offset within a header cell to the appropriate [`ColumnHeaderZone`].
///
/// `cell_width` is the full logical-pixel width of the cell including padding.
/// `x` is the cursor/click offset measured from the left edge of the cell.
///
/// Values outside `[0, cell_width]` clamp to the nearest edge zone (Grip or
/// Funnel) rather than panicking — callers should not rely on out-of-range
/// inputs but the function is total.
pub fn zone_from_x(x: f32, cell_width: f32) -> ColumnHeaderZone {
    let funnel_start = cell_width - HEADER_FUNNEL_PX;
    let sort_start = funnel_start - HEADER_SORT_PX;
    if x < HEADER_GRIP_PX {
        ColumnHeaderZone::Grip
    } else if x < sort_start.max(HEADER_GRIP_PX) {
        ColumnHeaderZone::Body
    } else if x < funnel_start.max(sort_start.max(HEADER_GRIP_PX)) {
        ColumnHeaderZone::Sort
    } else {
        ColumnHeaderZone::Funnel
    }
}
