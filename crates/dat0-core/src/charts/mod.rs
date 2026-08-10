//! Charts: a pure kernel (`spec` / `query` / `data` / `render` / `export`).
//!
//! The GPUI/Dioxus shell (`panel`, plus the WorkspaceShell wiring) lives in the
//! UI crate; nothing here knows what is drawing. The kernel is
//! headless-unit-tested.
//!
//! The inline inspector histogram / top-N binning below is a separate, smaller
//! concern: `histogram_bins` and `bar_fraction` are the spine, and the UI turns
//! their output into bars.

pub mod data;
pub mod export;
pub mod panel;
pub mod query;
pub mod render;
pub mod spec;

#[derive(Debug, Clone, PartialEq)]
pub struct Bin {
    pub lo: f64,
    pub hi: f64,
    pub count: u64,
}

/// Even-width histogram bins over `[min, max]`. Values `== max` land in the last
/// bin; values `< min` clamp to the first, `> max` to the last. NaNs are skipped.
pub fn histogram_bins(min: f64, max: f64, values: &[f64], n: usize) -> Vec<Bin> {
    let n = n.max(1);
    let span = (max - min).max(f64::MIN_POSITIVE);
    let width = span / n as f64;
    let mut bins: Vec<Bin> = (0..n)
        .map(|i| Bin {
            lo: min + i as f64 * width,
            hi: min + (i as f64 + 1.0) * width,
            count: 0,
        })
        .collect();
    for &v in values {
        if v.is_nan() {
            continue;
        }
        let mut idx = ((v - min) / width).floor() as isize;
        if idx < 0 {
            idx = 0;
        }
        if idx as usize >= n {
            idx = n as isize - 1;
        }
        bins[idx as usize].count += 1;
    }
    bins
}

/// Fraction of `count` against the largest count in the set (`max`). Returns
/// `0.0` when `max == 0` so an all-empty set renders as flat baselines.
pub fn bar_fraction(count: u64, max: u64) -> f64 {
    if max == 0 {
        0.0
    } else {
        count as f64 / max as f64
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bins_split_range_evenly() {
        let bins = histogram_bins(0.0, 10.0, &[0.0, 1.0, 5.0, 9.9], 5);
        assert_eq!(bins.len(), 5);
        assert_eq!(bins.iter().map(|b| b.count).sum::<u64>(), 4);
        assert_eq!(bins[0].count, 2, "0.0 and 1.0 fall in first bin [0,2)");
    }
    #[test]
    fn normalized_bar_widths() {
        let w = bar_fraction(3, 6);
        assert!((w - 0.5).abs() < 1e-9);
    }
}
