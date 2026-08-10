//! The read-only gate.
//!
//! A workspace opened read-only refuses every mutation. The predicate is one
//! line, but it is the only thing between a `.dat0` package opened for
//! inspection and a write, so it lives where it can be tested without a window
//! and named by every mutation site.

/// Returns `true` when a mutation should be refused because the shell is in
/// Inspect (read-only) mode.
///
/// Every data-mutation entry point in this module calls this predicate as its
/// **first** statement so the gate logic lives in exactly one place. `window.rs`
/// also calls it for `save_view_as_table` and the SQL-console DDL/DML path.
pub fn mutation_blocked(read_only: bool) -> bool {
    read_only
}

/// Parse a cell editor's raw text into a typed [`Scalar`] for `column_type`.
/// `None` means the text is not a value of that type — an un-parseable number,
/// a malformed date — and the caller must suppress the commit rather than
/// write `Scalar::Int("abc")` into a numeric column.
///
/// Lives here, beside the read-only gate, because it is the other half of
/// "may this write happen": one asks whether the workspace allows a write, the
/// other whether the value is one. Both are pure, so a headless test can pin
/// them without a toolkit.
pub fn parse_cell_text(
    column_type: crate::view::filter_popover::ColumnType,
    raw: &str,
) -> Option<dat0_engine::Scalar> {
    use crate::view::filter_popover::ColumnType;
    use dat0_engine::Scalar;

    match column_type {
        // Any text is a string, including the empty one.
        ColumnType::String => Some(Scalar::Str(raw.to_string())),
        ColumnType::Numeric => {
            let trimmed = raw.trim();
            // Integer first: `1` must not become `1.0` and widen the column.
            if let Ok(i) = trimmed.parse::<i64>() {
                Some(Scalar::Int(i))
            } else {
                trimmed.parse::<f64>().ok().map(Scalar::Float)
            }
        }
        ColumnType::Date => Scalar::validate_date(raw.trim())
            .ok()
            .map(|s| Scalar::Date(s.to_string())),
        ColumnType::Timestamp => Scalar::validate_timestamp(raw.trim())
            .ok()
            .map(|s| Scalar::Timestamp(s.to_string())),
        // A bool column edits through a picker, but text still has to parse:
        // a pasted or typed value reaches the same commit.
        ColumnType::Bool => match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "t" | "yes" => Some(Scalar::Bool(true)),
            "false" | "0" | "f" | "no" => Some(Scalar::Bool(false)),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cell_text;
    use crate::view::filter_popover::ColumnType;
    use dat0_engine::Scalar;

    #[test]
    fn a_numeric_column_refuses_text() {
        assert_eq!(
            parse_cell_text(ColumnType::Numeric, "42"),
            Some(Scalar::Int(42))
        );
        assert_eq!(
            parse_cell_text(ColumnType::Numeric, " 1.5 "),
            Some(Scalar::Float(1.5))
        );
        assert_eq!(parse_cell_text(ColumnType::Numeric, "abc"), None);
        assert_eq!(parse_cell_text(ColumnType::Numeric, ""), None);
    }

    #[test]
    fn a_date_column_refuses_a_date_it_cannot_store() {
        assert_eq!(
            parse_cell_text(ColumnType::Date, "2026-05-30"),
            Some(Scalar::Date("2026-05-30".into()))
        );
        assert_eq!(parse_cell_text(ColumnType::Date, "2026/05/30"), None);
        assert_eq!(parse_cell_text(ColumnType::Date, "nope"), None);
    }

    #[test]
    fn a_timestamp_column_refuses_a_bare_date() {
        assert_eq!(
            parse_cell_text(ColumnType::Timestamp, "2026-05-30 12:30:00"),
            Some(Scalar::Timestamp("2026-05-30 12:30:00".into()))
        );
        assert_eq!(parse_cell_text(ColumnType::Timestamp, "2026-05-30"), None);
    }

    #[test]
    fn a_bool_column_takes_the_forms_people_actually_type() {
        assert_eq!(
            parse_cell_text(ColumnType::Bool, "true"),
            Some(Scalar::Bool(true))
        );
        assert_eq!(
            parse_cell_text(ColumnType::Bool, "FALSE"),
            Some(Scalar::Bool(false))
        );
        assert_eq!(parse_cell_text(ColumnType::Bool, "maybe"), None);
    }

    #[test]
    fn a_string_column_takes_anything_including_nothing() {
        assert_eq!(
            parse_cell_text(ColumnType::String, ""),
            Some(Scalar::Str(String::new()))
        );
    }
}
