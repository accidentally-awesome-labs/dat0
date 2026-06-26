//! Pure panel data + pager helpers for the first-run tour carousel.
//!
//! `PANELS` is the ordered 7-panel script (design D3 / onboarding-v1.md §5):
//! each panel bundles its illustration bytes (via `include_bytes!`) plus the
//! i18n keys for its headline and one-line body. The pager helpers (`next` /
//! `back` / `is_last`) are the carousel's navigation arithmetic, kept pure and
//! unit-tested here; the modal render in `mod.rs` is UAT-gated (house pattern).

/// One tour panel: a bundled illustration plus the i18n keys for its copy.
pub struct Panel {
    /// PNG bytes bundled at compile time (placeholder solid art until T11).
    pub image: &'static [u8],
    /// i18n key for the panel headline.
    pub title_key: &'static str,
    /// i18n key for the one-line panel body.
    pub body_key: &'static str,
}

/// The 7-panel tour script, in order (onboarding-v1.md §5 table).
pub const PANELS: [Panel; 7] = [
    Panel {
        image: include_bytes!("../../assets/onboarding/p1.png"),
        title_key: "onboarding.tour.p1.title",
        body_key: "onboarding.tour.p1.body",
    },
    Panel {
        image: include_bytes!("../../assets/onboarding/p2.png"),
        title_key: "onboarding.tour.p2.title",
        body_key: "onboarding.tour.p2.body",
    },
    Panel {
        image: include_bytes!("../../assets/onboarding/p3.png"),
        title_key: "onboarding.tour.p3.title",
        body_key: "onboarding.tour.p3.body",
    },
    Panel {
        image: include_bytes!("../../assets/onboarding/p4.png"),
        title_key: "onboarding.tour.p4.title",
        body_key: "onboarding.tour.p4.body",
    },
    Panel {
        image: include_bytes!("../../assets/onboarding/p5.png"),
        title_key: "onboarding.tour.p5.title",
        body_key: "onboarding.tour.p5.body",
    },
    Panel {
        image: include_bytes!("../../assets/onboarding/p6.png"),
        title_key: "onboarding.tour.p6.title",
        body_key: "onboarding.tour.p6.body",
    },
    Panel {
        image: include_bytes!("../../assets/onboarding/p7.png"),
        title_key: "onboarding.tour.p7.title",
        body_key: "onboarding.tour.p7.body",
    },
];

/// Advance to the next panel, clamped at the last (index 6).
pub fn next(i: usize) -> usize {
    (i + 1).min(PANELS.len() - 1)
}

/// Step back to the previous panel, clamped at the first (index 0).
pub fn back(i: usize) -> usize {
    i.saturating_sub(1)
}

/// True when `i` is the last panel (index 6) — Next becomes "Get started".
pub fn is_last(i: usize) -> bool {
    i == PANELS.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_panels() {
        assert_eq!(PANELS.len(), 7);
    }

    #[test]
    fn pager_bounds() {
        assert_eq!(next(0), 1);
        assert_eq!(next(6), 6, "next clamps at last");
        assert_eq!(back(0), 0, "back clamps at first");
        assert_eq!(back(3), 2);
        assert!(is_last(6) && !is_last(0));
    }

    #[test]
    fn copy_invariants_present() {
        let p4 = dat0_i18n::t(PANELS[3].body_key).to_lowercase();
        assert!(
            p4.contains("review") && (p4.contains("run") || p4.contains("runs")),
            "panel 4 must state AI SQL is reviewed before it runs"
        );
        let p7 = dat0_i18n::t(PANELS[6].body_key).to_lowercase();
        assert!(
            p7.contains("key") && p7.contains("local"),
            "panel 7 must state AI off until key + data local"
        );
    }
}
