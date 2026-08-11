//! The load-bearing rules of the design system, asserted so a refactor cannot
//! silently drift off them.
//!
//! This is deliberately narrow. Most of the design contract is already gated
//! elsewhere and duplicating it here would produce two tests that fail together
//! and neither of which is the one you read:
//!
//! | Rule | Already gated by |
//! |---|---|
//! | amber is a fill, never body text (token *values*) | `dat0-core/tests/theme_tokens_contrast.rs` |
//! | `ink_on_amber` identical across all three builtins | `dat0-core/tests/theme.rs` |
//! | `app.css` names no token `ThemeTokens` lacks, and defines none nothing uses | `dat0_ui::protocol` unit tests |
//! | one titlebar / tabstrip / statusbar / pane-stack, each mounted once | `tests/a11y_content.rs` |
//! | the real pixel geometry — 44 / 38 / 30 / 238, Geist Mono at 12.5px | `examples/shell_probe.rs`, in a real window |
//!
//! What is left is the half no token test can see, because it is about the
//! *stylesheet* rather than the palette: a rule may hold the amber value and
//! still put it somewhere the design forbids.

use std::path::Path;

/// The design's single amber. Hard-coded rather than read from a builtin,
/// because this test is the thing that would catch a builtin being edited to
/// smuggle amber into a text colour.
const AMBER: &str = "#f5a623";

fn app_css() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/app.css"))
        .expect("read assets/app.css")
}

/// Strip `/* … */`. CSS has no nested comments, so a scan for the next `*/` is
/// exact rather than approximate — the same stripper `protocol.rs` uses, and
/// needed for the same reason: the file documents the amber rule in prose, and
/// a scanner that could not tell prose from code would forbid explaining what
/// it enforces.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        match rest[open + 2..].find("*/") {
            Some(close) => rest = &rest[open + 2 + close + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The design's first hard rule: **amber is a fill, never ink.**
///
/// It is the one colour in the palette with no accessible text contrast against
/// the canvas, which is why `--d0-amber-text` exists as a separate, darker
/// token for the rare "text that must read as amber" case. A `color:` that
/// resolves to the fill is the exact mistake the two tokens exist to prevent,
/// and it is invisible in review — the value looks right.
#[test]
fn amber_is_never_a_text_colour_in_the_stylesheet() {
    let css = strip_comments(&app_css());
    let mut offenders = Vec::new();

    for (i, line) in css.lines().enumerate() {
        let t = line.trim();
        // `color:` but not `background-color:`, `border-color:`, `caret-color:`
        // or a custom property — those are fills, carets and rules, all legal.
        let is_text_colour = t.starts_with("color:")
            || t.starts_with("-webkit-text-fill-color:")
            || t.starts_with("text-decoration-color:");
        if !is_text_colour {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.contains(AMBER) || lower.contains("var(--d0-amber)") {
            offenders.push(format!("{}: {t}", i + 1));
        }
    }

    assert!(
        offenders.is_empty(),
        "amber is a fill, never ink — these declarations put the amber FILL on \
         text. Use `--d0-amber-text`, which is the darker token that exists for \
         exactly this:\n  {}",
        offenders.join("\n  ")
    );
}

/// Amber appears at all, so the rule above is not vacuous.
///
/// A stylesheet with no amber in it would pass the fill rule perfectly while
/// meaning the brand colour had been deleted.
#[test]
fn the_stylesheet_still_uses_amber_somewhere() {
    let css = strip_comments(&app_css());
    assert!(
        css.contains("var(--d0-amber)") || css.contains("var(--d0-amber-hover)"),
        "no rule references the amber fill — either the brand colour is gone or \
         the token was renamed, and `amber_is_never_a_text_colour_in_the_stylesheet` \
         is now checking nothing"
    );
}

/// Every colour a theme carries is reachable from CSS.
///
/// `ThemeTokens` is the source of truth and `CSS_NAMES` is how it reaches the
/// document. A field added to the struct but not to the table is a token that
/// exists in Rust, round-trips through the builtin JSONs, and paints nothing —
/// which reads in review as "the colour is wired up".
/// Three fields are deliberately excluded, and each for a reason rather than
/// because it was inconvenient: `id` names the theme, `mode` selects the
/// browser's `color-scheme`, and `shadow` is a boolean that switches elevation
/// off for high-contrast. None of them is a colour, so none of them belongs in
/// a table of colour custom properties.
#[test]
fn every_token_field_reaches_the_stylesheet() {
    use dat0_core::theme::tokens::{CSS_NAMES, ThemeTokens};

    let light = dat0_core::theme::builtin("light").expect("the light builtin exists");
    let json = serde_json::to_value(&light).expect("ThemeTokens serializes");
    const NOT_COLOURS: [&str; 3] = ["id", "mode", "shadow"];
    let fields: Vec<String> = json
        .as_object()
        .expect("a struct serializes to an object")
        .iter()
        // A colour is a string; `shadow` is a bool and drops out here. `id` and
        // `mode` are strings that name things rather than describe them, so
        // they are named explicitly — a filter that silently skipped every
        // string it could not place would skip a real token too.
        .filter(|(k, v)| v.is_string() && !NOT_COLOURS.contains(&k.as_str()))
        .map(|(k, _)| k.clone())
        .collect();

    // The table stores accessors, not names, so compare by resolved value
    // position: every field must be produced by exactly one accessor.
    let vars = light.css_vars();
    let mut missing = Vec::new();
    for field in &fields {
        let css_name = format!("--d0-{}", field.replace('_', "-"));
        if !CSS_NAMES.iter().any(|(n, _)| *n == css_name) {
            missing.push(css_name);
        }
    }
    assert!(
        missing.is_empty(),
        "these ThemeTokens fields have no CSS_NAMES entry, so they paint \
         nothing: {missing:?}"
    );

    // And the emitted block really carries them, which is what CSS_NAMES is for.
    for (name, _) in CSS_NAMES.iter() {
        assert!(
            vars.contains(&format!("{name}:")),
            "{name} is in CSS_NAMES but absent from css_vars() output"
        );
    }

    // Non-vacuity: a struct that stopped serializing its fields would make the
    // loop above pass over an empty list.
    assert!(
        fields.len() >= 30,
        "only {} token fields found — the reflection is broken, not the theme",
        fields.len()
    );
    let _ = std::marker::PhantomData::<ThemeTokens>;
}

/// The shell declares one grid track per child it renders.
///
/// The headless half of `examples/shell_probe.rs`'s "grid sits beside the
/// sidebar". There is no layout here, so this cannot measure boxes — but the
/// bug it guards was not a measurement problem. The shell rendered THREE
/// children (sidebar, splitter, work area) into a two-column grid, so the work
/// area wrapped onto a second row and the catalog sat on top of the grid
/// instead of beside it. Every height and width was individually correct;
/// sizes are not a layout, and a probe that only measured sizes passed.
///
/// A track count is something a string can carry, so it belongs in the suite
/// that runs without a display.
#[test]
fn the_shell_declares_a_track_for_every_child_it_renders() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/components/shell.rs"),
    )
    .expect("read shell.rs");

    // Every dock, not just the sidebar. The sidebar's was fixed when the bug
    // was first found; `.d0-workarea` and `.d0-centre` shipped with the same
    // defect and were caught later, by eye, because this test only ever looked
    // at one of the three.
    for (needle, children) in [
        (
            "grid-template-columns: {sidebar_px}px",
            "sidebar, splitter, work area",
        ),
        (
            "grid-template-columns: minmax(0, 1fr) 0px {right_px}px",
            "centre, splitter, right column",
        ),
        (
            "grid-template-rows: minmax(0, 1fr) 0px {bottom_px}px",
            "pane stack, splitter, console",
        ),
    ] {
        let line = src
            .find(needle)
            .map(|i| src[i..].lines().next().unwrap_or_default())
            .unwrap_or_else(|| panic!("no dock template matching {needle:?}"));
        let tracks = track_count(line);
        assert_eq!(
            tracks, 3,
            "the shell renders three children here — {children} — so the \
             template needs three tracks. With two, the last child is \
             auto-placed into an implicit track and the splitter takes the one \
             meant for the panel. Found {tracks} in: {line}"
        );
    }
}

/// Count the tracks in a `grid-template-*` declaration, at paren depth 0 so
/// `minmax(0, 1fr)` counts once.
///
/// A count rather than a substring match on a literal width: the splitter's
/// track went from `4px` to `0px` when it stopped occupying space and started
/// straddling the boundary, and a test that pinned the old number failed for a
/// change that did not touch what it was guarding.
fn track_count(line: &str) -> usize {
    let Some(decl) = line.split_once(':').map(|(_, rest)| rest) else {
        return 0;
    };
    let decl = decl.trim().trim_end_matches(['"', ',']).trim();
    let (mut depth, mut n, mut in_token) = (0i32, 0usize, false);
    for ch in decl.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && ch.is_whitespace() {
            in_token = false;
            continue;
        }
        if !in_token {
            n += 1;
            in_token = true;
        }
    }
    n
}
