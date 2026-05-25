//! Import wizard drawer.
//!
//! The pure trigger predicate [`should_show_wizard`] is unit-tested in
//! `tests/import_wizard.rs`. The drawer GPUI view itself is stubbed for T9
//! and will be wired in a follow-up that mounts the verified Sheet pattern
//! (or hand-rolled overlay) from the T0 spike.
//!
//! ## PD-011 substitute heuristic (P3b T9)
//!
//! `duckdb::sniff_csv` does NOT expose `top_score` / `next_score` /
//! `encoding_supported` / per-column confidence. The plan's three-clause rule
//! references fields that don't exist in DuckDB 1.4.x output. See
//! `docs/internal/duckdb-arrow-api-notes.md` §SniffCsv and `docs/deferrals.md`
//! PD-011 for the full drift analysis.
//!
//! T9 implements the **dual-sniff + UTF-8 check** substitute documented in
//! PD-011 §5:
//!
//! 1. Run `sniff_csv(path, sample_size := 4096)` and
//!    `sniff_csv(path, sample_size := 65536)`. If the inferred `Delimiter`
//!    differs between the two runs → ambiguous. We synthesise
//!    `top_score = 0.55, next_score = 0.53` so the 5% predicate fires.
//!    Otherwise → `top_score = 1.0, next_score = 0.0`.
//! 2. Read the first 8 KB of the file; if `std::str::from_utf8` fails →
//!    `encoding_supported = false`.
//! 3. Per-column confidence is intentionally skipped for v1 (Option C from
//!    PD-011 §4) — flagged as a follow-up; the field stays `false` here.
//!
//! PD-011 is **closed at T13 retro**, not here.

use std::path::Path;

use anyhow::Context;

/// Numerical summary of a CSV sniff. The `top_score` / `next_score` fields are
/// synthesised by [`sniff`] from the dual-sniff agreement check (see module
/// docstring + PD-011). They are not pulled from `sniff_csv` directly because
/// DuckDB 1.4.x does not expose candidate-delimiter scores.
#[derive(Debug, Clone)]
pub struct SniffSummary {
    pub top_delimiter: char,
    pub top_score: f64,
    pub next_score: f64,
    pub encoding_supported: bool,
    pub any_low_confidence_column: bool,
}

/// Returns true if the wizard should open (ambiguous sniff).
///
/// Rule: top within 5% of next, OR non-UTF-8 encoding, OR any low-confidence
/// column. Matches the plan-verbatim test expectations.
pub fn should_show_wizard(s: &SniffSummary) -> bool {
    if !s.encoding_supported {
        return true;
    }
    if s.any_low_confidence_column {
        return true;
    }
    if s.top_score > 0.0 && (s.top_score - s.next_score) / s.top_score < 0.05 {
        return true;
    }
    false
}

/// Run DuckDB's `sniff_csv` over `path` and convert to our summary shape.
///
/// Uses the PD-011 substitute heuristic — dual `sniff_csv` calls at different
/// `sample_size` values to derive a top/next agreement score, plus a first-8KB
/// UTF-8 check for encoding. Per-column confidence is not derived (PD-011
/// option C); `any_low_confidence_column` is always `false` from this path.
///
/// We open a side-channel in-memory duckdb connection rather than borrowing
/// the engine's mutex — `sniff_csv` is read-only and contention-free this way,
/// and it sidesteps Arrow STRUCT decoding through the engine's typed
/// `QueryResult` surface.
///
/// Returns an `Err` if duckdb can't open or sniff the file at all. Callers
/// (see `file_drop`) should treat that as "assume confident, log warn" rather
/// than blocking the drop.
pub fn sniff(path: &Path) -> anyhow::Result<SniffSummary> {
    // 1. Encoding heuristic: first 8 KB UTF-8 check. We do this BEFORE the
    //    sniff_csv calls because DuckDB's CSV reader errors hard on non-UTF-8
    //    input ("Invalid unicode … This file is not utf-8 encoded"). When the
    //    head is not UTF-8 we short-circuit with a confident-delimiter summary
    //    flagged `encoding_supported = false` — `should_show_wizard` then
    //    triggers on the encoding clause alone, which is the correct UX.
    let encoding_supported = read_head_is_utf8(path)?;
    if !encoding_supported {
        return Ok(SniffSummary {
            top_delimiter: ',',
            top_score: 1.0,
            next_score: 0.0,
            encoding_supported: false,
            any_low_confidence_column: false,
        });
    }

    // 2. Dual-sniff: agreement on `Delimiter` between two sample sizes.
    let conn =
        duckdb::Connection::open_in_memory().context("open in-memory duckdb for sniff_csv")?;
    let path_str = path
        .to_str()
        .context("sniff_csv: path is not valid UTF-8")?;

    let delim_small =
        sniff_delimiter(&conn, path_str, 4_096).context("sniff_csv: small-sample run failed")?;
    let delim_large =
        sniff_delimiter(&conn, path_str, 65_536).context("sniff_csv: large-sample run failed")?;

    let (top_delimiter, top_score, next_score) = if delim_small == delim_large {
        // Confident: both sample sizes agree on the delimiter.
        (delim_small, 1.0_f64, 0.0_f64)
    } else {
        // Ambiguous: synthesise a within-5% top/next pair so the predicate
        // fires. Score values match the plan's ambiguity test (0.55 / 0.53).
        (delim_large, 0.55_f64, 0.53_f64)
    };

    Ok(SniffSummary {
        top_delimiter,
        top_score,
        next_score,
        encoding_supported,
        // Per PD-011 option C: skipped for T9 v1 — wizard still surfaces a
        // dialect-overrides UI; per-column confidence is a follow-up.
        any_low_confidence_column: false,
    })
}

/// Run `sniff_csv(path, sample_size := N)` and return the inferred delimiter
/// as a `char`. Falls back to `','` if the column comes back empty.
fn sniff_delimiter(
    conn: &duckdb::Connection,
    path: &str,
    sample_size: u64,
) -> duckdb::Result<char> {
    let mut stmt = conn.prepare("SELECT Delimiter FROM sniff_csv(?, sample_size := ?)")?;
    let delim: String = stmt.query_row(duckdb::params![path, sample_size], |row| {
        row.get::<_, String>(0)
    })?;
    Ok(delim.chars().next().unwrap_or(','))
}

/// Read the first 8 KB of `path` and check if it is valid UTF-8. Returns
/// `true` when valid (or when the file is shorter than 8 KB and still valid).
fn read_head_is_utf8(path: &Path) -> anyhow::Result<bool> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).context("open file for utf-8 head check")?;
    let mut buf = [0_u8; 8 * 1024];
    let n = f
        .read(&mut buf)
        .context("read head bytes for utf-8 check")?;
    Ok(std::str::from_utf8(&buf[..n]).is_ok())
}

/// Open the wizard drawer for the given file + initial sniff. Entry point
/// invoked by `file_drop` when sniff is ambiguous.
///
/// **Stub for T9.** The real drawer view follows the T0 spike Sheet pattern
/// and will be wired in a follow-up (tracked in P3b T13 retro). For now the
/// function logs and returns — the user-visible UX falls back to direct
/// register, matching P3a behaviour.
pub fn open(_app: &mut gpui::App, path: &Path, initial_sniff: SniffSummary) {
    tracing::info!(
        path = %path.display(),
        top_delimiter = %initial_sniff.top_delimiter,
        encoding_supported = initial_sniff.encoding_supported,
        "import_wizard::open invoked (drawer view stubbed — follow-up wires the Sheet)",
    );
}
