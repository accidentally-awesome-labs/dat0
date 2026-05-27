//! Deterministic filter fixture for P4a T0 spike + heavy bench.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Distinct cities in the fixture. ~50 entries; uniform random draw per row.
const CITIES: &[&str] = &[
    "SF", "NYC", "LA", "CHI", "HOU", "PHX", "PHI", "SAN", "DAL", "SJ", "AUS", "JAC", "FTW", "COL",
    "SF2", "NYC2", "IND", "CHA", "DEN", "WAS", "BOS", "ELP", "DET", "NAS", "MEM", "POR", "OKL",
    "LAS", "BAL", "LOU", "MIL", "ALB", "TUC", "FRE", "SAC", "MES", "KAN", "ATL", "OMA", "RAL",
    "MIA", "OAK", "MIN", "TUL", "ARL", "NOL", "WIC", "BAK", "AUR", "CIN",
];

/// Generate a deterministic CSV fixture with `rows` rows seeded by `seed`.
/// Schema: id INT, price DOUBLE, city VARCHAR, ts TIMESTAMP, active BOOL.
///
/// Returns the path to the file. The file is written under `dir` with name
/// `filter_{rows}_seed{seed}.csv`; callers can decide caching strategy.
pub fn gen_filter_fixture(dir: &Path, rows: usize, seed: u64) -> Result<PathBuf> {
    let path = dir.join(format!("filter_{}_seed{}.csv", rows, seed));
    if path.exists() {
        return Ok(path);
    }
    let f = File::create(&path).with_context(|| format!("create fixture {}", path.display()))?;
    let mut w = BufWriter::with_capacity(1024 * 1024, f);

    writeln!(w, "id,price,city,ts,active")?;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    // Year-range for synthetic timestamps: 2020-01-01 .. 2026-01-01 UTC.
    let ts_start: u64 = 1_577_836_800; // 2020-01-01 UTC
    let ts_end: u64 = 1_767_225_600; // 2026-01-01 UTC

    for id in 0..rows {
        let price: f64 = rng.r#gen::<f64>() * 10_000.0;
        let city: &str = CITIES[rng.gen_range(0..CITIES.len())];
        let ts_secs: u64 = rng.gen_range(ts_start..ts_end);
        let ts = chrono_naive_string(ts_secs);
        let active: bool = rng.r#gen::<bool>();
        writeln!(
            w,
            "{},{:.2},{},{},{}",
            id,
            price,
            city,
            ts,
            if active { "true" } else { "false" }
        )?;
    }
    w.flush()?;
    Ok(path)
}

/// Format a UTC unix-seconds value as `yyyy-mm-dd hh:mm:ss` without adding a
/// chrono dependency just for this. The fixture is deterministic so this
/// hand-rolled formatter is acceptable.
fn chrono_naive_string(secs: u64) -> String {
    // Civil-time arithmetic (Howard Hinnant's algorithm), Unix-epoch days.
    let days = (secs / 86_400) as i64;
    let secs_of_day = (secs % 86_400) as u32;
    let hour = secs_of_day / 3600;
    let min = (secs_of_day / 60) % 60;
    let sec = secs_of_day % 60;
    let (y, m, d) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y, m, d, hour, min, sec
    )
}

fn days_to_ymd(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn fixture_is_deterministic() {
        let tmp = TempDir::new().unwrap();
        let p1 = gen_filter_fixture(tmp.path(), 1000, 42).unwrap();
        let bytes1 = std::fs::read(&p1).unwrap();

        let tmp2 = TempDir::new().unwrap();
        let p2 = gen_filter_fixture(tmp2.path(), 1000, 42).unwrap();
        let bytes2 = std::fs::read(&p2).unwrap();

        assert_eq!(bytes1, bytes2, "same seed must produce identical bytes");
    }

    #[test]
    fn fixture_row_count_matches() {
        let tmp = TempDir::new().unwrap();
        let p = gen_filter_fixture(tmp.path(), 5000, 7).unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        let row_lines = content.lines().count() - 1; // minus header
        assert_eq!(row_lines, 5000);
    }
}
