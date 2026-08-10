use dat0_core::view::sort_header::ActiveSort;
use dat0_engine::{SortDirection, SortKey};

fn empty() -> ActiveSort {
    ActiveSort::default()
}

fn with_keys(keys: Vec<(&str, SortDirection)>) -> ActiveSort {
    ActiveSort::new(
        keys.into_iter()
            .map(|(c, d)| SortKey {
                column: c.into(),
                direction: d,
            })
            .collect(),
    )
}

// ── ActiveSort::find (preserved from T9 smoke test) ──────────────────────────

#[test]
fn find_returns_rank_and_direction() {
    let s = with_keys(vec![("a", SortDirection::Asc), ("b", SortDirection::Desc)]);
    assert_eq!(s.find("a"), Some((1, SortDirection::Asc)));
    assert_eq!(s.find("b"), Some((2, SortDirection::Desc)));
    assert_eq!(s.find("c"), None);
}

#[test]
fn find_on_empty_returns_none() {
    let s = empty();
    assert_eq!(s.find("x"), None);
}

// ── zone_from_x geometry (preserved from T9 smoke test) ──────────────────────

use dat0_core::grid::{
    ColumnHeaderZone, HEADER_FUNNEL_PX, HEADER_GRIP_PX, HEADER_SORT_PX, zone_from_x,
};

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
    assert_eq!(zone_from_x(100.0, 200.0), ColumnHeaderZone::Body);
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
    assert_eq!(zone_from_x(cell + 1.0, cell), ColumnHeaderZone::Funnel);
}

#[test]
fn grip_px_body_px_sort_px_funnel_px_sum_fits_typical_column() {
    let fixed = HEADER_GRIP_PX + HEADER_SORT_PX + HEADER_FUNNEL_PX;
    assert!(
        fixed < 80.0,
        "fixed zones ({fixed}px) exceed a typical narrow column width"
    );
}

// ── ActiveSort::click state machine ──────────────────────────────────────────

#[test]
fn click_on_empty_becomes_asc() {
    let s = empty().click("price");
    assert_eq!(s.find("price"), Some((1, SortDirection::Asc)));
}

#[test]
fn click_on_asc_cycles_to_desc() {
    let s = with_keys(vec![("price", SortDirection::Asc)]).click("price");
    assert_eq!(s.find("price"), Some((1, SortDirection::Desc)));
}

#[test]
fn click_on_desc_cycles_to_none() {
    let s = with_keys(vec![("price", SortDirection::Desc)]).click("price");
    assert!(s.find("price").is_none());
    assert_eq!(s.keys.len(), 0);
}

#[test]
fn click_on_other_column_replaces_existing() {
    let s = with_keys(vec![
        ("city", SortDirection::Asc),
        ("price", SortDirection::Desc),
    ])
    .click("ts");
    assert!(s.find("city").is_none());
    assert!(s.find("price").is_none());
    assert_eq!(s.find("ts"), Some((1, SortDirection::Asc)));
}

// ── ActiveSort::shift_click state machine ────────────────────────────────────

#[test]
fn shift_click_appends_when_absent() {
    let s = with_keys(vec![("city", SortDirection::Asc)]).shift_click("price");
    assert_eq!(s.find("city"), Some((1, SortDirection::Asc)));
    assert_eq!(s.find("price"), Some((2, SortDirection::Asc)));
}

#[test]
fn shift_click_cycles_within_rank_asc_to_desc() {
    let s = with_keys(vec![
        ("city", SortDirection::Asc),
        ("price", SortDirection::Asc),
    ])
    .shift_click("price");
    assert_eq!(s.find("price"), Some((2, SortDirection::Desc)));
}

#[test]
fn shift_click_removes_at_desc() {
    let s = with_keys(vec![
        ("city", SortDirection::Asc),
        ("price", SortDirection::Desc),
    ])
    .shift_click("price");
    assert!(s.find("price").is_none());
    assert_eq!(s.find("city"), Some((1, SortDirection::Asc)));
}

#[test]
fn removing_middle_rank_shifts_later_up() {
    let s = with_keys(vec![
        ("a", SortDirection::Asc),
        ("b", SortDirection::Desc),
        ("c", SortDirection::Asc),
    ])
    .shift_click("b");
    assert_eq!(s.find("a"), Some((1, SortDirection::Asc)));
    assert!(s.find("b").is_none());
    assert_eq!(s.find("c"), Some((2, SortDirection::Asc)));
}

#[test]
fn shift_click_on_empty_appends_new_key() {
    let s = empty().shift_click("city");
    assert_eq!(s.find("city"), Some((1, SortDirection::Asc)));
}

#[test]
fn click_after_shift_click_replaces_entire_sort() {
    let s = with_keys(vec![("a", SortDirection::Asc), ("b", SortDirection::Asc)]).click("c");
    assert_eq!(s.find("c"), Some((1, SortDirection::Asc)));
    assert!(s.find("a").is_none());
    assert!(s.find("b").is_none());
}
