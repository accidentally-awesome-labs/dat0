//! T7 — hand-rolled spreadsheet-TSV codec round-trip tests.
//!
//! The clipboard dialect is the de-facto Excel / Google-Sheets TSV: tab-
//! separated columns, CRLF rows, CSV-style quoting for any cell containing a
//! tab / CR / LF / double-quote. The headline P4b exit gate is the Excel /
//! Sheets round-trip; these unit tests pin the pure-logic codec that gate
//! depends on. Parsing must accept BOTH LF and CRLF row terminators (Sheets
//! emits LF on some platforms; Excel emits CRLF).

use dat0_app::grid::clipboard::{tsv_parse, tsv_serialize};

/// Convenience: build a `Vec<Vec<String>>` grid from string literals.
fn grid(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| r.iter().map(|s| s.to_string()).collect())
        .collect()
}

#[test]
fn plain_grid_roundtrips() {
    let grid = vec![
        vec!["a".to_string(), "b".to_string()],
        vec!["c".to_string(), "d".to_string()],
    ];
    let tsv = tsv_serialize(&grid);
    assert_eq!(tsv, "a\tb\r\nc\td");
    assert_eq!(tsv_parse(&tsv), grid);
}

#[test]
fn cell_with_tab_newline_quote_is_quoted() {
    let grid = vec![vec![
        "x\ty".to_string(),
        "li\nne".to_string(),
        "say \"hi\"".to_string(),
    ]];
    let tsv = tsv_serialize(&grid);
    assert_eq!(tsv, "\"x\ty\"\t\"li\nne\"\t\"say \"\"hi\"\"\"");
    assert_eq!(tsv_parse(&tsv), grid);
}

#[test]
fn parse_accepts_lf_and_crlf() {
    let expected = grid(&[&["a", "b"], &["c", "d"]]);
    assert_eq!(tsv_parse("a\tb\nc\td"), expected);
    assert_eq!(tsv_parse("a\tb\r\nc\td"), expected);
}

#[test]
fn roundtrip_unicode_cell() {
    let g = grid(&[&["café", "naïve 日本語 😀", "z"]]);
    assert_eq!(tsv_parse(&tsv_serialize(&g)), g);
}

#[test]
fn roundtrip_empty_cell() {
    // A blank middle cell and a trailing blank cell must survive the round-trip.
    let g = grid(&[&["a", "", "c"], &["", "b", ""]]);
    assert_eq!(tsv_parse(&tsv_serialize(&g)), g);
}

#[test]
fn roundtrip_cell_that_is_just_a_quote() {
    // A cell whose entire content is a single double-quote must be escaped as
    // `""""` (open-quote, escaped-quote, close-quote) and parse back to `"`.
    let g = grid(&[&["\"", "after"]]);
    let tsv = tsv_serialize(&g);
    assert_eq!(tsv, "\"\"\"\"\tafter");
    assert_eq!(tsv_parse(&tsv), g);
}

#[test]
fn roundtrip_multiline_cell() {
    // An embedded newline inside a quoted cell must NOT be treated as a row
    // separator on parse.
    let g = grid(&[&["first\nsecond\nthird", "tail"], &["next-row", "x"]]);
    assert_eq!(tsv_parse(&tsv_serialize(&g)), g);
}

#[test]
fn roundtrip_cell_with_embedded_crlf() {
    // A quoted cell carrying a literal CRLF must round-trip as that exact cell,
    // not split into two rows.
    let g = grid(&[&["a\r\nb", "c"]]);
    assert_eq!(tsv_parse(&tsv_serialize(&g)), g);
}

#[test]
fn discontiguous_selection_bounding_rect_serializes() {
    // The copy handler builds a bounding-rect grid where gaps become empty
    // cells. A 2x2 bounding rect with the two off-diagonal cells blank must
    // serialize as a full 2x2 TSV (blanks preserved) and round-trip.
    let g = grid(&[&["A", ""], &["", "D"]]);
    let tsv = tsv_serialize(&g);
    assert_eq!(tsv, "A\t\r\n\tD");
    assert_eq!(tsv_parse(&tsv), g);
}

#[test]
fn parse_empty_input_yields_single_empty_cell() {
    // Documented invariant: tsv_parse never returns []; an empty payload is
    // [[""]]. The paste handler relies on this (it rejects an all-empty grid,
    // not is_empty()).
    assert_eq!(tsv_parse(""), vec![vec![String::new()]]);
}
