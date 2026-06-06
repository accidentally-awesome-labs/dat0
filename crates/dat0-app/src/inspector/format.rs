//! Pure formatting of profile stats into Inspector card strings (P6a T9).
//!
//! Keeps *all* string logic for the Inspector column cards in one place so the
//! panel ([`crate::inspector::panel`]) only arranges divs. Charts are a separate
//! task (T10); these functions produce the formatted stat lines that sit beside
//! (and later under) the inline charts.
use dat0_engine::ColumnProfile;

/// Approximate-distinct line. The count comes from DuckDB `approx_unique`
/// (HyperLogLog), so it is explicitly labeled `approx` to avoid implying an
/// exact `COUNT(DISTINCT)`.
pub fn format_distinct(c: &ColumnProfile) -> String {
    format!("distinct ≈{} (approx)", c.approx_distinct)
}

/// Null-fraction line, e.g. `null 0.3%`.
pub fn format_null(c: &ColumnProfile) -> String {
    format!("null {:.1}%", c.null_pct)
}

/// The per-column summary line: numeric stats when present, else string-length
/// stats, else empty (e.g. a column SUMMARIZE produced no numeric/length stats
/// for — booleans, all-null, etc.).
pub fn format_stats_line(c: &ColumnProfile) -> String {
    match &c.numeric {
        Some(n) => format!(
            "min {} · max {} · μ {:.1} · med {} · σ {:.1}",
            trim(n.min),
            trim(n.max),
            n.avg,
            trim(n.median),
            n.std
        ),
        None => match &c.length {
            Some(l) => format!("len {}–{} (μ{:.1})", l.min, l.max, l.avg),
            None => String::new(),
        },
    }
}

/// Render a float compactly: integers drop the trailing `.0`, fractions keep two
/// decimals (so `28.0 -> "28"`, `41.25 -> "41.25"`).
fn trim(f: f64) -> String {
    if f.fract() == 0.0 {
        format!("{}", f as i64)
    } else {
        format!("{:.2}", f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::{ColumnProfile, NumericStats};
    #[test]
    fn formats_numeric_card() {
        let c = ColumnProfile {
            name: "amount".into(),
            ty: "DOUBLE".into(),
            null_pct: 0.3,
            approx_distinct: 980,
            count: 1000,
            length: None,
            numeric: Some(NumericStats {
                min: 0.0,
                max: 9942.0,
                avg: 41.2,
                std: 63.1,
                q25: 10.0,
                median: 28.0,
                q75: 55.0,
            }),
        };
        let s = format_stats_line(&c);
        assert!(s.contains("min 0"), "{s}");
        assert!(s.contains("med 28"), "{s}");
        assert!(
            format_distinct(&c).contains("approx"),
            "HLL labeled approx"
        );
    }
}
