//! WCAG contrast gate over the three builtin [`ThemeTokens`] sets, plus the
//! two hard rules the design states about amber.
//!
//! Thresholds: text pairs ≥ 4.5:1 (WCAG 1.4.3 AA) in `light` and `dark`, and
//! ≥ **7:1** (1.4.6 AAA) in `high-contrast` — that number is the theme's entire
//! reason to exist, so it is asserted rather than assumed.
//!
//! Non-text pairs are ≥ 3:1 (WCAG 1.4.11) where a boundary is required to
//! operate the control. Decorative separators are listed in [`UNGATED`] with a
//! reason and their measured value, so an exemption is a reviewed act with a
//! number attached rather than a silent hole — and the stale arm fires too, so
//! an excuse cannot outlive the thing it excused.

use std::collections::BTreeSet;

use dat0_core::theme::contrast::{composite_over, contrast_ratio};
use dat0_core::theme::tokens::{BUILTIN_IDS, CSS_NAMES, ThemeTokens, builtin};

/// Resolve a token by field name. Goes through [`CSS_NAMES`] so the test names
/// tokens exactly as the stylesheet does, and a rename breaks here rather than
/// silently retargeting a pair.
fn tok<'a>(t: &'a ThemeTokens, css_name: &str) -> &'a str {
    CSS_NAMES
        .iter()
        .find(|(n, _)| *n == css_name)
        .map(|(_, get)| get(t))
        .unwrap_or_else(|| panic!("no such token: {css_name}"))
}

/// Foreground/background pairs that carry text.
///
/// Every semantic ink is measured against every ground it is actually painted
/// on — not just the default one. A colour that is legible on `--d0-surface`
/// and illegible on `--d0-panel` is still a bug.
const TEXT_PAIRS: &[(&str, &str)] = &[
    ("--d0-ink", "--d0-surface"),
    ("--d0-ink", "--d0-canvas"),
    ("--d0-ink", "--d0-pane-head"),
    ("--d0-ink", "--d0-panel"),
    ("--d0-ink", "--d0-tab-active"),
    ("--d0-ink", "--d0-active-bg"),
    ("--d0-fg", "--d0-surface"),
    ("--d0-fg", "--d0-canvas"),
    ("--d0-fg", "--d0-pane-head"),
    ("--d0-fg", "--d0-panel"),
    ("--d0-fg", "--d0-row-hover"),
    ("--d0-muted", "--d0-surface"),
    ("--d0-muted", "--d0-pane-head"),
    ("--d0-muted", "--d0-panel"),
    ("--d0-muted", "--d0-canvas"),
    ("--d0-chrome-muted", "--d0-chrome"),
    ("--d0-chrome-muted", "--d0-chrome-raised"),
    // Tab labels sit on the hover fill as often as on the strip itself.
    ("--d0-chrome-muted", "--d0-tab-hover"),
    ("--d0-accent", "--d0-surface"),
    ("--d0-accent", "--d0-canvas"),
    ("--d0-accent", "--d0-field"),
    ("--d0-ok", "--d0-surface"),
    ("--d0-ok", "--d0-chrome"),
    ("--d0-error", "--d0-surface"),
    ("--d0-error", "--d0-canvas"),
    ("--d0-warn-text", "--d0-chrome-raised"),
    ("--d0-warn-text", "--d0-pane-head"),
    ("--d0-amber-text", "--d0-surface"),
    ("--d0-amber-text", "--d0-canvas"),
    // Both amber fills, because the ink never changes: checking only the rest
    // state is how an unreadable hover ships.
    ("--d0-ink-on-amber", "--d0-amber"),
    ("--d0-ink-on-amber", "--d0-amber-hover"),
    ("--d0-sql-keyword", "--d0-surface"),
    ("--d0-sql-number", "--d0-surface"),
    ("--d0-sql-string", "--d0-surface"),
    ("--d0-sql-fn", "--d0-surface"),
    ("--d0-sql-comment", "--d0-surface"),
    ("--d0-cyan", "--d0-surface"),
];

/// Non-text pairs whose boundary is required to perceive or operate a control.
const NON_TEXT_PAIRS: &[(&str, &str)] = &[
    // The focus ring. If this fails, keyboard navigation is invisible.
    ("--d0-accent", "--d0-surface"),
    ("--d0-accent", "--d0-canvas"),
    ("--d0-accent", "--d0-chrome"),
    // The engine / connection status dot, which carries meaning by colour.
    ("--d0-ok", "--d0-chrome"),
    ("--d0-error", "--d0-chrome"),
];

/// Boundaries deliberately outside the matrix, each with the reason it carries
/// no threshold. Shrink-only: an entry naming a pair that now passes, or a pair
/// that no longer exists, fails the gate.
const UNGATED: &[(&str, &str, &str)] = &[
    (
        "--d0-rule",
        "--d0-surface",
        "pane and grid-header separators: decorative rules, WCAG 1.4.11 carve-out. \
         Structure is conveyed by layout and by the pane header, not by the line.",
    ),
    (
        "--d0-rule-dim",
        "--d0-surface",
        "between-row rule in the grid: a reading aid at 26px row height, not a \
         boundary anything is operated by.",
    ),
    (
        "--d0-tab-divider",
        "--d0-tabstrip",
        "tab separators: the active tab is identified by its fill and its 2px \
         accent underline, both of which are gated.",
    ),
    (
        "--d0-chrome-border",
        "--d0-chrome",
        "titlebar / status-bar edges: decorative seams between two chrome \
         surfaces, no control boundary.",
    ),
    (
        "--d0-input-border",
        "--d0-surface",
        "input and chip outlines: matches the pre-migration gate's standing \
         `border`/`input.border` carve-out. Focus state is carried by \
         --d0-accent, which IS gated at 3:1 on every ground.",
    ),
    (
        "--d0-chrome-border-2",
        "--d0-chrome-raised",
        "chrome button outlines: same carve-out as --d0-input-border.",
    ),
    (
        "--d0-amber",
        "--d0-canvas",
        "the brand fill (logomark, primary CTA, package chip). Design-owned and \
         pinned by the marketing surface. What makes the CTA legible is \
         --d0-ink-on-amber on --d0-amber, which is gated at text thresholds in \
         every theme and measures 9.18:1.",
    ),
];

fn text_threshold(id: &str) -> f64 {
    // High contrast exists to clear AAA. Holding it to AA would make the theme
    // a label rather than a promise.
    if id == "high-contrast" { 7.0 } else { 4.5 }
}

fn check(id: &str, t: &ThemeTokens, pairs: &[(&str, &str)], min: f64, out: &mut Vec<String>) {
    for (fg, bg) in pairs {
        let r = contrast_ratio(tok(t, fg), tok(t, bg));
        if r < min {
            out.push(format!(
                "  {id}: {fg} on {bg} = {r:.2}:1 (need {min}:1) — {} on {}",
                tok(t, fg),
                tok(t, bg)
            ));
        }
    }
}

#[test]
fn text_pairs_meet_their_threshold_in_every_builtin() {
    let mut failures = Vec::new();
    for id in BUILTIN_IDS {
        let t = builtin(id).expect("builtin parses");
        check(id, &t, TEXT_PAIRS, text_threshold(id), &mut failures);
    }
    assert!(
        failures.is_empty(),
        "text contrast failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn non_text_pairs_meet_wcag_1_4_11() {
    let mut failures = Vec::new();
    for id in BUILTIN_IDS {
        let t = builtin(id).expect("builtin parses");
        check(id, &t, NON_TEXT_PAIRS, 3.0, &mut failures);
    }
    assert!(
        failures.is_empty(),
        "non-text contrast failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn high_contrast_actually_beats_the_other_two() {
    // A regression where high-contrast is copied from light and never sharpened
    // would pass the AAA gate only if light already did. Assert the ordering
    // directly on the body-text pair.
    let ratio_of = |id: &str| {
        let t = builtin(id).unwrap();
        contrast_ratio(tok(&t, "--d0-fg"), tok(&t, "--d0-surface"))
    };
    let hc = ratio_of("high-contrast");
    assert!(hc >= 7.0, "high-contrast body text is {hc:.2}:1");
    assert!(
        hc > ratio_of("light"),
        "high-contrast ({hc:.2}) must exceed light ({:.2})",
        ratio_of("light")
    );
}

/// Hard rule 1: **amber is a fill, never body text.**
#[test]
fn only_amber_named_tokens_carry_the_amber_hex() {
    const AMBER: &str = "#f5a623";
    for id in BUILTIN_IDS {
        let t = builtin(id).unwrap();
        for (name, value) in t.pairs() {
            if value.eq_ignore_ascii_case(AMBER) {
                assert!(
                    name.starts_with("--d0-amber"),
                    "{id}: {name} resolves to the amber fill {AMBER}. Amber may only \
                     appear as a background, fill or border-color; text that must read \
                     as amber uses --d0-amber-text."
                );
            }
        }
    }
}

/// Hard rule 1, second half: text *on* amber does not change between themes,
/// because the fill it sits on does not.
#[test]
fn ink_on_amber_is_identical_in_every_builtin() {
    let values: BTreeSet<String> = BUILTIN_IDS
        .iter()
        .map(|id| builtin(id).unwrap().ink_on_amber)
        .collect();
    assert_eq!(
        values.len(),
        1,
        "ink_on_amber must be one value across all builtins, found {values:?}"
    );
}

/// Hard rule 2: **blue, not amber, is the interaction accent.**
#[test]
fn the_accent_is_never_the_brand_fill() {
    for id in BUILTIN_IDS {
        let t = builtin(id).unwrap();
        assert_ne!(
            t.accent.to_ascii_lowercase(),
            t.amber.to_ascii_lowercase(),
            "{id}: the focus ring / caret / active-tab underline must be blue, not amber"
        );
    }
}

/// Drift alarm, both directions: an `UNGATED` entry must name a real pair that
/// really is below 3:1, and must not duplicate a gated one.
#[test]
fn ungated_entries_are_real_and_still_needed() {
    let gated: BTreeSet<(&str, &str)> = TEXT_PAIRS.iter().chain(NON_TEXT_PAIRS).copied().collect();

    for (fg, bg, reason) in UNGATED {
        assert!(
            !reason.trim().is_empty(),
            "{fg}/{bg} is exempt with no reason"
        );
        assert!(
            !gated.contains(&(*fg, *bg)),
            "{fg}/{bg} is both gated and exempt"
        );

        let below_somewhere = BUILTIN_IDS.iter().any(|id| {
            let t = builtin(id).unwrap();
            contrast_ratio(tok(&t, fg), tok(&t, bg)) < 3.0
        });
        assert!(
            below_somewhere,
            "{fg}/{bg} now clears 3:1 in every builtin — delete the exemption \
             and add it to NON_TEXT_PAIRS"
        );
    }
}

/// Every token must be reachable by the name the stylesheet uses.
#[test]
fn css_names_cover_every_token_and_resolve() {
    let t = builtin("light").unwrap();
    let names: BTreeSet<&str> = CSS_NAMES.iter().map(|(n, _)| *n).collect();
    assert_eq!(names.len(), CSS_NAMES.len(), "duplicate --d0- name");
    for (name, _) in CSS_NAMES {
        assert!(!tok(&t, name).is_empty(), "{name} is empty");
    }
}

/// Tokens that carry no measurable pair, each with the reason. Same contract as
/// [`UNGATED`] — a reason, and a stale entry fails — but for a different cause:
/// these are not colours that sit on other colours. A shadow is a blur list, an
/// inset is a translucent rgba consumed only by a `box-shadow`, and the scrim is
/// an overlay with nothing painted on it (the modal panel above it is opaque
/// `--d0-surface`, which *is* gated).
const UNMEASURABLE: &[(&str, &str)] = &[
    (
        "--d0-shadow-pane",
        "a box-shadow list, not a colour: no foreground and no ground",
    ),
    ("--d0-shadow-overlay", "as --d0-shadow-pane"),
    (
        "--d0-scrim",
        "translucent overlay: nothing is painted on it, and what shows through \
         it is dimmed content rather than text. Asserted instead by \
         the_modal_scrim_actually_dims_what_is_behind_it",
    ),
    (
        "--d0-inset",
        "translucent rgba consumed only as an inner box-shadow on field edges",
    ),
];

/// Drift alarm: **no token may ship unaccounted for.**
///
/// Successor to the GPUI gate's `sibling_pairs_all_gated`, which walked the
/// `X.foreground`/`X.background` families the widget library's schema happened
/// to define. There are no families now, so the alarm widens to the whole token
/// set: every `--d0-*` name must be measured by a pair, excused by [`UNGATED`]
/// with a reason, or classified [`UNMEASURABLE`]. Adding a colour to
/// `ThemeTokens` and forgetting to measure it fails here.
#[test]
fn every_token_is_measured_excused_or_classified() {
    let mut accounted: BTreeSet<&str> = BTreeSet::new();
    for (fg, bg) in TEXT_PAIRS.iter().chain(NON_TEXT_PAIRS) {
        accounted.insert(fg);
        accounted.insert(bg);
    }
    for (fg, bg, _) in UNGATED {
        accounted.insert(fg);
        accounted.insert(bg);
    }
    for (name, reason) in UNMEASURABLE {
        assert!(
            !reason.trim().is_empty(),
            "{name} is classified with no reason"
        );
        accounted.insert(name);
    }

    let defined: BTreeSet<&str> = CSS_NAMES.iter().map(|(n, _)| *n).collect();

    let unmeasured: Vec<&&str> = defined.difference(&accounted).collect();
    assert!(
        unmeasured.is_empty(),
        "these tokens are in no pair and in no exemption list: {unmeasured:?} — \
         add a measured pair, or classify them with the reason they carry none"
    );
    let stale: Vec<&&str> = accounted.difference(&defined).collect();
    assert!(
        stale.is_empty(),
        "these names are measured or excused but no longer exist: {stale:?}"
    );
}

/// Every ground in the matrix is **opaque**.
///
/// Successor to `composited_tints_keep_text_readable`. The GPUI palette painted
/// text over 8-digit tints (`selection.background`, `drop_target.background`),
/// so the gate had to composite before it could measure. The redesign resolved
/// those to opaque tokens — `--d0-row-hover`, `--d0-active-bg` — which is why
/// the pairs above can be measured directly. That is a premise, not a fact of
/// nature: if a translucent value returns under text, `contrast_ratio` would
/// panic on it, and a panic is a worse diagnosis than this.
#[test]
fn every_measured_ground_is_opaque() {
    for id in BUILTIN_IDS {
        let t = builtin(id).unwrap();
        for (fg, bg) in TEXT_PAIRS.iter().chain(NON_TEXT_PAIRS) {
            for name in [fg, bg] {
                let v = tok(&t, name);
                assert!(
                    v.starts_with('#') && v.len() == 7,
                    "{id}: {name} = {v} is not an opaque #rrggbb. A translucent \
                     value under text must be composited with \
                     dat0_core::theme::contrast::composite_over before it is \
                     measured — add the composite here rather than dropping the pair"
                );
            }
        }
    }
}

/// The scrim is a real dim: neither invisible nor a blackout.
///
/// The one place the old composited-tint check still has a subject. A scrim
/// that composites to its own ground means a modal that does not read as modal
/// — which is exactly how the GPUI build's stacked dialogs became unreadable —
/// and an opaque one hides the workspace the dialog is about.
#[test]
fn the_modal_scrim_actually_dims_what_is_behind_it() {
    for id in BUILTIN_IDS {
        let t = builtin(id).unwrap();
        let scrim = t.scrim.trim_start_matches('#');
        assert_eq!(
            scrim.len(),
            8,
            "{id}: the scrim must carry an alpha channel, got {}",
            t.scrim
        );
        let alpha = u8::from_str_radix(&scrim[6..8], 16).expect("hex alpha");
        assert!(
            alpha > 0,
            "{id}: a fully transparent scrim leaves the modal floating on live UI"
        );
        assert!(
            alpha < 255,
            "{id}: an opaque scrim hides the workspace the dialog is about"
        );

        let dimmed = composite_over(&t.scrim, &t.canvas);
        assert_ne!(
            dimmed, t.canvas,
            "{id}: the scrim composites to the canvas it covers — no dim at all"
        );
    }
}
