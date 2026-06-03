//! Pure statement splitting + classification for the SQL console (P5a §3.1).
//!
//! `split_statements` finds `;`-delimited statement spans, ignoring `;` inside
//! single-quoted strings, `--` line comments, and `/* */` block comments.
//! Known-naive on dollar-quoting and nested block comments (P5a §10) — hardened
//! later. `classify` decides whether a statement produces a result set (VIEW
//! path) or not (EXEC path) by its leading keyword after stripping leading
//! whitespace/comments.

/// A byte range `[start, end)` into the source SQL buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Whether a statement yields rows (run as a TEMP VIEW) or not (run via execute()).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultKind {
    Result,
    Exec,
}

#[derive(Clone, Copy, PartialEq)]
enum ScanState {
    Normal,
    SingleQuote,
    DoubleQuote,
    LineComment,
    BlockComment,
}

/// Split `sql` into non-empty statement spans separated by top-level `;`.
/// A trailing segment with no terminating `;` is included. Whitespace-only
/// segments are dropped. Spans cover the original bytes (callers may trim).
pub fn split_statements(sql: &str) -> Vec<Span> {
    let bytes = sql.as_bytes();
    let mut spans = Vec::new();
    let mut state = ScanState::Normal;
    let mut seg_start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied();
        match state {
            ScanState::Normal => match (b, next) {
                (b'\'', _) => state = ScanState::SingleQuote,
                (b'"', _) => state = ScanState::DoubleQuote,
                (b'-', Some(b'-')) => {
                    state = ScanState::LineComment;
                    i += 1;
                }
                (b'/', Some(b'*')) => {
                    state = ScanState::BlockComment;
                    i += 1;
                }
                (b';', _) => {
                    push_span(sql, &mut spans, seg_start, i);
                    seg_start = i + 1;
                }
                _ => {}
            },
            ScanState::SingleQuote => {
                if b == b'\'' {
                    state = ScanState::Normal;
                }
            }
            ScanState::DoubleQuote => {
                if b == b'"' {
                    state = ScanState::Normal;
                }
            }
            ScanState::LineComment => {
                if b == b'\n' {
                    state = ScanState::Normal;
                }
            }
            ScanState::BlockComment => {
                if b == b'*' && next == Some(b'/') {
                    state = ScanState::Normal;
                    i += 1;
                }
            }
        }
        i += 1;
    }
    push_span(sql, &mut spans, seg_start, bytes.len());
    spans
}

fn push_span(sql: &str, spans: &mut Vec<Span>, start: usize, end: usize) {
    if sql[start..end].trim().is_empty() {
        return;
    }
    spans.push(Span { start, end });
}

/// Return the span of the statement containing byte offset `cursor`. If the
/// cursor sits on a boundary or past the last statement, returns the nearest
/// preceding non-empty statement (trailing segment for end-of-buffer).
pub fn statement_at(sql: &str, cursor: usize) -> Span {
    let spans = split_statements(sql);
    if spans.is_empty() {
        return Span {
            start: 0,
            end: sql.len(),
        };
    }
    for s in &spans {
        if cursor >= s.start && cursor <= s.end {
            return *s;
        }
    }
    // Cursor beyond the last span end (e.g. trailing whitespace) -> last span.
    *spans.last().unwrap()
}

/// Classify a single statement as result-producing or exec-only by leading keyword.
pub fn classify(stmt: &str) -> ResultKind {
    let head = strip_leading_noise(stmt);
    let word: String = head
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();
    const RESULT_KW: &[&str] = &[
        "SELECT",
        "WITH",
        "VALUES",
        "TABLE",
        "FROM",
        "PRAGMA",
        "SHOW",
        "DESCRIBE",
        "DESC",
        "EXPLAIN",
        "SUMMARIZE",
    ];
    if RESULT_KW.contains(&word.as_str()) {
        ResultKind::Result
    } else {
        ResultKind::Exec
    }
}

/// Strip leading whitespace and leading `--` / `/* */` comments from `stmt`.
fn strip_leading_noise(stmt: &str) -> &str {
    let mut s = stmt.trim_start();
    loop {
        if let Some(rest) = s.strip_prefix("--") {
            match rest.find('\n') {
                Some(nl) => s = rest[nl + 1..].trim_start(),
                None => return "",
            }
        } else if let Some(rest) = s.strip_prefix("/*") {
            match rest.find("*/") {
                Some(end) => s = rest[end + 2..].trim_start(),
                None => return "",
            }
        } else {
            return s;
        }
    }
}
