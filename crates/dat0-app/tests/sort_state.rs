//! Smoke tests for `view::sort_header::ActiveSort` and the
//! `grid::zone_from_x` geometry helper.  Full sort state-machine tests
//! (Asc → Desc → None cycle, shift-click multi-sort, rank subscripts) land
//! in T12.

use dat0_app::grid::{
    ColumnHeaderZone, HEADER_FUNNEL_PX, HEADER_GRIP_PX, HEADER_SORT_PX, zone_from_x,
};
use dat0_app::view::sort_header::ActiveSort;
use dat0_engine::{SortDirection, SortKey};

// ── ActiveSort ────────────────────────────────────────────────────────────────

#[test]
fn find_returns_rank_and_direction() {
    let s = ActiveSort {
        keys: vec![
            SortKey {
                column: "a".into(),
                direction: SortDirection::Asc,
            },
            SortKey {
                column: "b".into(),
                direction: SortDirection::Desc,
            },
        ],
    };
    assert_eq!(s.find("a"), Some((1, SortDirection::Asc)));
    assert_eq!(s.find("b"), Some((2, SortDirection::Desc)));
    assert_eq!(s.find("c"), None);
}

#[test]
fn find_on_empty_returns_none() {
    let s = ActiveSort::default();
    assert_eq!(s.find("x"), None);
}

// ── zone_from_x geometry ─────────────────────────────────────────────────────
//
// Cell width chosen as 200 px for clarity.
// Expected zones:
//   Grip   : x in [0, HEADER_GRIP_PX)     → [0, 6)
//   Sort   : x in [cell - SORT - FUNNEL, cell - FUNNEL) → [160, 180)
//   Funnel : x in [cell - FUNNEL, cell)   → [180, 200)
//   Body   : everything else              → [6, 160)

#[test]
fn grip_zone_at_left_edge() {
    assert_eq!(zone_from_x(0.0, 200.0), ColumnHeaderZone::Grip);
    assert_eq!(
        zone_from_x(HEADER_GRIP_PX - 0.1, 200.0),
        ColumnHeaderZone::Grip
    );
}

#[test]
fn body_zone_in_middle() {
    // x = 100 is clearly in the body region for a 200px cell
    assert_eq!(zone_from_x(100.0, 200.0), ColumnHeaderZone::Body);
    // x = HEADER_GRIP_PX is the first body pixel
    assert_eq!(zone_from_x(HEADER_GRIP_PX, 200.0), ColumnHeaderZone::Body);
}

#[test]
fn sort_zone_right_of_body() {
    let cell = 200.0_f32;
    let sort_start = cell - HEADER_SORT_PX - HEADER_FUNNEL_PX;
    assert_eq!(zone_from_x(sort_start, cell), ColumnHeaderZone::Sort);
    assert_eq!(zone_from_x(sort_start + 1.0, cell), ColumnHeaderZone::Sort);
}

#[test]
fn funnel_zone_at_right_edge() {
    let cell = 200.0_f32;
    let funnel_start = cell - HEADER_FUNNEL_PX;
    assert_eq!(zone_from_x(funnel_start, cell), ColumnHeaderZone::Funnel);
    assert_eq!(zone_from_x(cell - 1.0, cell), ColumnHeaderZone::Funnel);
    // x >= cell also lands in funnel (clamped)
    assert_eq!(zone_from_x(cell + 1.0, cell), ColumnHeaderZone::Funnel);
}

#[test]
fn grip_px_body_px_sort_px_funnel_px_sum_fits_typical_column() {
    // Sanity-check: the fixed zones don't exceed a typical narrow column (80px).
    let fixed = HEADER_GRIP_PX + HEADER_SORT_PX + HEADER_FUNNEL_PX;
    assert!(
        fixed < 80.0,
        "fixed zones ({fixed}px) exceed a typical narrow column width"
    );
}
