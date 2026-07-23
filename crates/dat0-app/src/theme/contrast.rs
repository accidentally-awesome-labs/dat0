//! WCAG 2.1 relative-luminance + contrast-ratio math over `#rrggbb` hex
//! strings. Pure (no GPUI), so the a11y gate test runs headless in CI.
//! Slice A3 adds source-over compositing for 8-digit tinted tokens.

/// Parse `#rrggbb` into linearized sRGB relative luminance (WCAG 2.1 §def).
pub fn relative_luminance(hex: &str) -> f64 {
    let h = hex.trim_start_matches('#');
    assert!(
        h.len() == 6,
        "relative_luminance needs opaque #rrggbb; use composite_over for alpha colors (got {hex})"
    );
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

/// Source-over composite of `#rrggbbaa` fg onto an opaque `#rrggbb` bg,
/// returning the effective opaque `#rrggbb`. A 6-digit fg passes through
/// unchanged, so call sites need not care whether a token is tinted.
pub fn composite_over(fg: &str, bg: &str) -> String {
    let f = fg.trim_start_matches('#');
    if f.len() == 6 {
        return format!("#{f}");
    }
    assert!(f.len() == 8, "composite_over fg must be #rrggbb or #rrggbbaa (got {fg})");
    let b = bg.trim_start_matches('#');
    assert!(b.len() == 6, "composite_over bg must be opaque #rrggbb (got {bg})");
    let a = u8::from_str_radix(&f[6..8], 16).unwrap_or(0) as f64 / 255.0;
    let ch = |i: usize| {
        let fc = u8::from_str_radix(&f[i..i + 2], 16).unwrap_or(0) as f64;
        let bc = u8::from_str_radix(&b[i..i + 2], 16).unwrap_or(0) as f64;
        (fc * a + bc * (1.0 - a)).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", ch(0), ch(2), ch(4))
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

    #[test]
    fn composite_alpha_extremes_and_passthrough() {
        // α=0x00 → pure bg; α=0xff → pure fg; 6-digit fg → identity.
        assert_eq!(composite_over("#58a6ff00", "#0e1116"), "#0e1116");
        assert_eq!(composite_over("#58a6ffff", "#0e1116"), "#58a6ff");
        assert_eq!(composite_over("#58a6ff", "#0e1116"), "#58a6ff");
    }

    #[test]
    fn composite_known_selection_tints() {
        // Hand-computed source-over vectors (design doc red-set section):
        // dark selection.background over dark table.background,
        // light selection.background over light table.background.
        assert_eq!(composite_over("#58a6ff4d", "#0e1116"), "#243e5c");
        assert_eq!(composite_over("#0969da33", "#ffffff"), "#cee1f8");
    }

    #[test]
    #[should_panic(expected = "use composite_over")]
    fn contrast_ratio_rejects_alpha_hex() {
        // Pre-A3 this silently sliced the first 6 digits and read the color
        // as opaque — a latent false-pass. Now it must be loud.
        contrast_ratio("#58a6ff4d", "#0e1116");
    }
}
