//! Inline inspector charts (P6a T10): pure binning + GPUI-quad render. No chart lib.
//!
//! The *binning* (`histogram_bins`, `bar_fraction`) is pure and unit-tested — it
//! is the spine of this module. The `render_*` functions arrange GPUI quads
//! (plain `div`s with a fixed `bg`) scaled to the data; they are exercised only
//! in a real window (headless render is untestable), so the pure functions carry
//! the test weight.
use gpui::{IntoElement, ParentElement, Styled, div, px};

pub mod data;
pub mod query;
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

/// Histogram as a row of vertical quads, each scaled to the tallest bin.
pub fn render_histogram(bins: &[Bin]) -> impl IntoElement {
    let max = bins.iter().map(|b| b.count).max().unwrap_or(0);
    let mut row = div().flex().flex_row().items_end().gap(px(1.0)).h(px(28.0));
    for b in bins {
        let frac = bar_fraction(b.count, max);
        row = row.child(
            div()
                .w(px(9.0))
                .h(px((4.0 + frac * 24.0) as f32))
                .bg(gpui::rgb(0x55bb88)),
        );
    }
    row
}

/// Horizontal top-N bars: `(label, count)` → a labelled bar scaled to the max
/// count in the set.
pub fn render_topn(items: &[(String, u64)]) -> impl IntoElement {
    let max = items.iter().map(|(_, c)| *c).max().unwrap_or(0);
    let mut col = div().flex().flex_col().gap_1();
    for (label, c) in items {
        let frac = bar_fraction(*c, max);
        col = col.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().w(px(70.0)).text_size(px(11.0)).child(label.clone()))
                .child(
                    div()
                        .h(px(8.0))
                        .w(px((6.0 + frac * 80.0) as f32))
                        .bg(gpui::rgb(0x6699cc)),
                ),
        );
    }
    col
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
