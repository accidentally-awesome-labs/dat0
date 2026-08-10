use dat0_core::query::statement::{ResultKind, Span, classify, split_statements, statement_at};

#[test]
fn split_single_statement_no_semicolon() {
    let sql = "SELECT 1";
    assert_eq!(split_statements(sql), vec![Span { start: 0, end: 8 }]);
}

#[test]
fn split_two_statements() {
    let sql = "SELECT 1; SELECT 2";
    let spans = split_statements(sql);
    assert_eq!(spans.len(), 2);
    assert_eq!(&sql[spans[0].start..spans[0].end], "SELECT 1");
    assert_eq!(&sql[spans[1].start..spans[1].end].trim(), &"SELECT 2");
}

#[test]
fn semicolon_inside_single_quote_is_not_a_boundary() {
    let sql = "SELECT ';' AS x; SELECT 2";
    let spans = split_statements(sql);
    assert_eq!(
        spans.len(),
        2,
        "the ';' inside the string literal must not split"
    );
    assert!(sql[spans[0].start..spans[0].end].contains("';'"));
}

#[test]
fn semicolon_inside_line_comment_is_not_a_boundary() {
    let sql = "SELECT 1 -- a; b\n; SELECT 2";
    assert_eq!(split_statements(sql).len(), 2);
}

#[test]
fn semicolon_inside_block_comment_is_not_a_boundary() {
    let sql = "SELECT 1 /* a; b */ ; SELECT 2";
    assert_eq!(split_statements(sql).len(), 2);
}

#[test]
fn statement_at_returns_segment_under_cursor() {
    let sql = "SELECT 1; SELECT 2; SELECT 3";
    // cursor inside "SELECT 2"
    let span = statement_at(sql, 12);
    assert_eq!(sql[span.start..span.end].trim(), "SELECT 2");
}

#[test]
fn statement_at_past_last_semicolon_returns_trailing() {
    let sql = "SELECT 1; SELECT 2";
    let span = statement_at(sql, 18); // end of buffer
    assert_eq!(sql[span.start..span.end].trim(), "SELECT 2");
}

#[test]
fn classify_select_is_result() {
    assert_eq!(classify("SELECT 1"), ResultKind::Result);
}

#[test]
fn classify_with_cte_is_result() {
    assert_eq!(
        classify("  WITH t AS (SELECT 1) SELECT * FROM t"),
        ResultKind::Result
    );
}

#[test]
fn classify_leading_comment_then_select_is_result() {
    assert_eq!(classify("-- run me\nSELECT 1"), ResultKind::Result);
    assert_eq!(classify("/* x */ select 2"), ResultKind::Result);
}

#[test]
fn classify_create_table_is_exec() {
    assert_eq!(classify("CREATE TABLE t AS SELECT 1"), ResultKind::Exec);
}

#[test]
fn classify_insert_is_exec() {
    assert_eq!(classify("INSERT INTO t VALUES (1)"), ResultKind::Exec);
}

#[test]
fn classify_empty_is_exec() {
    assert_eq!(classify("   \n  "), ResultKind::Exec);
}
