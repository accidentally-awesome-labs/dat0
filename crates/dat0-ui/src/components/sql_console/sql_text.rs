//! What the console does to SQL text: the statement under the caret, the
//! view definition that holds its rows, and what to show when DuckDB refuses
//! it. Everything here is a pure function of its text, apart from reading an
//! engine error.

use dat0_core::query::statement::{leading_keyword, statement_at};
use dat0_engine::EngineError;

/// The definition of the view that holds a statement's rows, and the text it
/// puts before the statement. `None` for PRAGMA and EXPLAIN, which DuckDB
/// will neither define a view as nor select from; they run as statements.
pub(super) fn view_body(stmt: &str) -> Option<(String, &'static str)> {
    match leading_keyword(stmt).as_str() {
        "PRAGMA" | "EXPLAIN" => None,
        // A view cannot be defined as one of these, but can select from it.
        // The newline keeps a trailing `--` comment from eating the `)`.
        "SHOW" | "DESCRIBE" | "DESC" | "SUMMARIZE" => {
            Some((format!("SELECT * FROM ({stmt}\n)"), "SELECT * FROM ("))
        }
        _ => Some((stmt.to_string(), "")),
    }
}

/// The statement to run: the one under the caret, or the last one when the
/// caret has not been reported. `None` when there is nothing but whitespace
/// and comments' separators.
pub fn statement(doc: &str, caret: Option<(usize, usize)>) -> Option<String> {
    let at = caret.map_or(doc.len(), |(line, col)| offset(doc, line, col));
    let span = statement_at(doc, at);
    let stmt = doc[span.start..span.end].trim();
    // With no statement in it at all, `statement_at` hands back the whole
    // buffer, which may be nothing but separators.
    let blank = stmt
        .trim_matches(|c: char| c == ';' || c.is_whitespace())
        .is_empty();
    (!blank).then(|| stmt.to_string())
}

/// The byte offset of a 1-based (line, column) caret. CodeMirror counts
/// columns in UTF-16 code units. Clamped to the document, since a caret can
/// be reported against a longer document than the one the run sees.
pub fn offset(doc: &str, line: usize, col: usize) -> usize {
    let mut start = 0;
    for (i, text) in doc.split('\n').enumerate() {
        if i + 1 == line {
            let mut units = 0;
            for (at, ch) in text.char_indices() {
                if units >= col.saturating_sub(1) {
                    return start + at;
                }
                units += ch.len_utf16();
            }
            return start + text.len();
        }
        start += text.len() + 1;
    }
    doc.len()
}

/// Whether the run was interrupted rather than failed.
pub(super) fn interrupted(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        matches!(
            c.downcast_ref::<EngineError>(),
            Some(EngineError::Interrupted)
        )
    })
}

/// What DuckDB said, without the layers of context wrapped around it on the
/// way up: a person fixing a query needs the parser's message, not the name
/// of the function that counted the rows.
pub(super) fn message(e: &anyhow::Error, wrapper: &str) -> String {
    let raw = match e.chain().find_map(|c| c.downcast_ref::<EngineError>()) {
        Some(EngineError::DuckDb(duckdb::Error::DuckDBFailure(_, Some(msg)))) => msg.clone(),
        Some(other) => other.to_string(),
        None => format!("{e:#}"),
    };
    tidy(&raw, wrapper)
}

/// DuckDB quotes the statement it failed on, and for a query that statement
/// is the view around it. Quote the user's own text instead, with the caret
/// moved to match; and keep each block once, since parser errors repeat
/// theirs.
fn tidy(raw: &str, wrapper: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut shift = None;
    for line in raw.lines() {
        if let Some(by) = shift.take()
            && line.trim() == "^"
        {
            let pad = line.len() - line.trim_start().len();
            lines.push(format!("{}^", " ".repeat(pad.saturating_sub(by))));
            continue;
        }
        if line.starts_with("LINE ")
            && let (Some(colon), Some(at)) = (line.find(": "), line.find(wrapper))
            && colon + 2 <= at
        {
            let (from, to) = (colon + 2, at + wrapper.len());
            lines.push(format!("{}{}", &line[..from], &line[to..]));
            shift = Some(to - from);
            continue;
        }
        lines.push(line.to_string());
    }
    let text = lines.join("\n");
    let mut kept: Vec<&str> = Vec::new();
    for block in text.split("\n\n") {
        if !kept.contains(&block) {
            kept.push(block);
        }
    }
    kept.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caret_picks_the_statement_it_sits_in() {
        let doc = "SELECT 1;\nSELECT 2;\nSELECT 3";
        assert_eq!(statement(doc, Some((1, 3))).as_deref(), Some("SELECT 1"));
        assert_eq!(statement(doc, Some((2, 1))).as_deref(), Some("SELECT 2"));
        assert_eq!(statement(doc, Some((3, 9))).as_deref(), Some("SELECT 3"));
        // Unknown caret: the last statement, as if the caret were at the end.
        assert_eq!(statement(doc, None).as_deref(), Some("SELECT 3"));
    }

    #[test]
    fn an_empty_buffer_has_nothing_to_run() {
        assert_eq!(statement("", None), None);
        assert_eq!(statement("   \n  ", Some((2, 1))), None);
        assert_eq!(statement(";;", None), None);
    }

    #[test]
    fn a_caret_column_counts_utf16_units() {
        // "é" is one UTF-16 unit and two bytes; "😀" is two units and four.
        let doc = "é😀x";
        assert_eq!(offset(doc, 1, 1), 0);
        assert_eq!(offset(doc, 1, 2), 2, "after é");
        assert_eq!(offset(doc, 1, 4), 6, "after the emoji's two units");
        assert_eq!(offset(doc, 1, 5), 7, "end of line");
    }

    #[test]
    fn a_stale_caret_is_clamped_to_the_document() {
        let doc = "SELECT 1\nSELECT 2";
        assert_eq!(offset(doc, 9, 1), doc.len(), "a line past the end");
        assert_eq!(
            offset(doc, 1, 99),
            "SELECT 1".len(),
            "a column past the end"
        );
    }

    #[test]
    fn an_interrupt_is_told_apart_from_a_failure() {
        let cancelled = anyhow::Error::from(EngineError::Interrupted).context("count(*)");
        assert!(interrupted(&cancelled));
        let failed = anyhow::anyhow!("no such table");
        assert!(!interrupted(&failed));
    }

    #[test]
    fn the_message_is_duckdbs_not_the_wrapper_chain() {
        let e = anyhow::Error::from(EngineError::Interrupted)
            .context("count(*) for row_count")
            .context("GridDataSource::new — count_rows failed");
        assert_eq!(message(&e, "x"), EngineError::Interrupted.to_string());
    }

    // The texts below are DuckDB 1.4.4's, verbatim, for these statements run
    // through `create_or_replace_view("__dat0_qr_abcd1234_0", …)`.
    const WRAP: &str = "\"__dat0_qr_abcd1234_0\" AS ";

    fn caret(at: usize) -> String {
        format!("{}^", " ".repeat(at))
    }

    #[test]
    fn an_error_quotes_the_users_sql_not_the_view_around_it() {
        // `LINE 1: ` is 8 wide and the truncated wrapper 51, so DuckDB's caret
        // under WHERE is at 71; without the wrapper it is at 20.
        let context =
            "LINE 1: ... OR REPLACE TEMP VIEW \"__dat0_qr_abcd1234_0\" AS SELECT FROM WHERE";
        assert_eq!(context.find("WHERE"), Some(71));
        let block = format!("{context}\n{}", caret(71));
        let raw = format!("Parser Error: syntax error at or near \"WHERE\"\n\n{block}\n\n{block}");
        assert_eq!(
            tidy(&raw, WRAP),
            format!(
                "Parser Error: syntax error at or near \"WHERE\"\n\nLINE 1: SELECT FROM WHERE\n{}",
                caret(20)
            ),
            "the caret still sits under WHERE, and the block appears once"
        );
    }

    #[test]
    fn a_statement_selected_from_quotes_without_its_wrapper_either() {
        let context = "LINE 1: ...VIEW \"__dat0_qr_abcd1234_0\" AS SELECT * FROM (DESCRIBE nope";
        let at = context.find("nope").unwrap();
        let raw = format!(
            "Catalog Error: Table with name nope does not exist!\n\n{context}\n{}",
            caret(at)
        );
        let wrap = format!("{WRAP}SELECT * FROM (");
        assert_eq!(
            tidy(&raw, &wrap),
            format!(
                "Catalog Error: Table with name nope does not exist!\n\nLINE 1: DESCRIBE nope\n{}",
                caret("LINE 1: DESCRIBE ".len())
            )
        );
    }

    #[test]
    fn an_error_on_a_later_line_is_left_alone_but_said_once() {
        let block = format!("LINE 2:   FROM x\n{}", caret(10));
        let raw = format!("Parser Error: syntax error at or near \"FROM\"\n\n{block}\n\n{block}");
        assert_eq!(
            tidy(&raw, WRAP),
            format!("Parser Error: syntax error at or near \"FROM\"\n\n{block}")
        );
    }

    #[test]
    fn statements_a_view_cannot_be_are_selected_from_instead() {
        assert_eq!(view_body("SELECT 1"), Some(("SELECT 1".into(), "")));
        assert_eq!(
            view_body("show tables -- all of them"),
            Some((
                "SELECT * FROM (show tables -- all of them\n)".into(),
                "SELECT * FROM ("
            )),
            "and a trailing comment cannot swallow the parenthesis"
        );
        assert_eq!(view_body("PRAGMA version"), None);
        assert_eq!(view_body("/* plan */ EXPLAIN SELECT 1"), None);
    }
}
