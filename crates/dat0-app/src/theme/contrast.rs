//! WCAG 2.1 relative-luminance + contrast-ratio math over `#rrggbb` hex
//! strings. Pure (no GPUI), so the a11y gate test runs headless in CI.

/// Parse `#rrggbb` into linearized sRGB relative luminance (WCAG 2.1 §def).
pub fn relative_luminance(hex: &str) -> f64 {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
    fn lin(c: u8) -> f64 {
        let c = c as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// WCAG contrast ratio between two `#rrggbb` colors, in `[1.0, 21.0]`.
pub fn contrast_ratio(a: &str, b: &str) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_on_white_is_21() {
        assert!((contrast_ratio("#000000", "#ffffff") - 21.0).abs() < 0.01);
    }

    #[test]
    fn known_pairs_match_reference() {
        // dark fg/bg, computed reference = 12.25:1
        assert!((contrast_ratio("#c9d1d9", "#0e1116") - 12.25).abs() < 0.05);
        // light warning/bg, the tightest passing pair = 4.87:1
        assert!((contrast_ratio("#9a6700", "#ffffff") - 4.87).abs() < 0.05);
        // order-independent
        assert!((contrast_ratio("#0e1116", "#c9d1d9") - 12.25).abs() < 0.05);
    }
}
