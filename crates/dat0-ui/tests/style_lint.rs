//! Repo-wide style gate, re-scoped from `crates/dat0-app/src/**` to
//! `crates/dat0-ui/src/**` (+ `assets/app.css`).
//!
//! The GPUI original banned inline colour construction so the token system
//! could not be bypassed. The token system survived the migration — it is
//! `dat0_core::theme::ThemeTokens` now, emitted as `--d0-*` custom properties
//! into `<style id="d0-theme">` — but the *bypass* changed shape completely,
//! and a gate that only knew the old shape would read as coverage while
//! guarding nothing:
//!
//! | bypass | GPUI | Dioxus |
//! |---|---|---|
//! | a colour value in Rust | `gpui::rgb(0x…)`, `Hsla { … }` | `style: "color: #f5a623"` — a **string**, invisible to the old scanner |
//! | a colour value in CSS | did not exist | `app.css` is 2,400 lines the old gate never read |
//!
//! So this file scans for both, and adds the surface the GPUI build did not
//! have: `assets/app.css`, where a hex literal is legal **only inside a
//! `:root{…}` block** (custom-property definitions) and banned everywhere
//! else.
//!
//! ## What this gate does NOT do
//!
//! `src/protocol.rs`'s `app_css_and_the_token_set_agree` already checks both
//! directions of the `--d0-*` correspondence: no rule may reference a token
//! `ThemeTokens` does not define, and no token may go unread. That is the
//! *naming* half of the contract; this is the *literal* half. Neither
//! duplicates the other — a stylesheet can satisfy both token directions
//! perfectly and still hard-code `#f5a623` in a rule.
//!
//! ## Why this file cannot self-match
//!
//! The pattern table below is source code in `tests/`, and the walks cover
//! `src/` and `assets/app.css` only. Do not move this file into `src/`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Colour constructors, banned regardless of argument.
///
/// Each is matched as `NAME(` with a non-word character before it, never as a
/// plain substring. That boundary is not decoration: the GPUI original learned
/// it the hard way when a substring match on `red(` fired on
/// `md_not_attached_ignored()`. Here it is what keeps `to_srgb(` out of the
/// `rgb(` match and `parse_hexdump(` out of the `parse_hex(` one.
///
/// - `rgb` / `rgba` / `hsl` / `hsla` — CSS colour functions. Reachable from
///   Rust today: every dat0 component writes CSS through `style:` and `class:`
///   strings, so `style: "background: rgba(0,0,0,.5)"` compiles fine and
///   bypasses the token set entirely.
/// - `RGBColor` / `RGBAColor` / `HSLColor` — plotters' constructors. `dat0-ui`
///   depends on plotters directly, and a chart painted in plotters' default
///   palette instead of the resolved `--d0-*` values is exactly the drift
///   step 5.4 of the migration plan exists to prevent.
/// - `white` / `black` — the GPUI-era shorthand, kept because the shape
///   (a nullary colour constructor) is toolkit-independent.
/// - `parse_hex` — decoding a colour string by hand. See [`ALLOW`]: there is
///   exactly one legitimate site, and it is on the ratchet.
const BANNED_CTORS: &[&str] = &[
    "rgb",
    "rgba",
    "hsl",
    "hsla",
    "white",
    "black",
    "parse_hex",
    "RGBColor",
    "RGBAColor",
    "HSLColor",
];

/// Per-line escape. The reason text is mandatory and must be non-empty.
///
/// Deliberately *not* prefixed with `//`: the same marker has to work in a CSS
/// `/* … */` comment, and one marker beats two spellings that can drift.
const ALLOW_MARKER: &str = "style-lint: allow(";

/// Files that hold a colour literal or colour-decode site, with their EXACT
/// current count of offending LINES (a line with two hits counts once).
///
/// SHRINK-ONLY RATCHET. The gate fails if a count is left too high *or* too
/// low. A file absent from this table has an allowance of 0.
///
/// `launch.rs` is the one entry, and it is structural rather than debt. The
/// `tao` window is created with `Config::with_background_color((u8,u8,u8,u8))`
/// so the very first frame is not a white flash on a dark theme — that colour
/// is painted by the platform *before* a webview exists, so it cannot be a CSS
/// variable, and the only honest source for it is the theme's own
/// `--d0-canvas` string. Two lines: the `parse_hex` call in
/// `background_color`, and the `fn parse_hex` definition. Both are inside the
/// only function in the crate that is allowed to turn a token into bytes.
const ALLOW: &[(&str, usize)] = &[("launch.rs", 2)];

/// Per-file allowance of colour literals in `assets/app.css` OUTSIDE a
/// `:root{…}` block.
///
/// **EMPTY**, and that is the whole design: `app.css` holds no `:root` block
/// at all today, because the custom properties are emitted at runtime by
/// `ThemeTokens::css_vars()`. So every rule in the file reads `var(--d0-…)`
/// and a hex anywhere in it is a token that escaped the theme.
const CSS_ALLOW: &[(&str, usize)] = &[];

#[derive(Debug)]
struct Violation {
    line_no: usize,
    pattern: String,
    text: String,
}

#[derive(Default, Debug)]
struct ScanResult {
    violations: Vec<Violation>,
    /// Lines carrying an allow marker but NO banned pattern — the escape
    /// outlived the code it excused.
    stale_allows: Vec<usize>,
    /// Lines whose allow marker has an empty reason.
    empty_reasons: Vec<usize>,
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// Alphanumeric or `_` — the characters that make a run part of one identifier.
fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Length of the hex-digit run starting at `at`.
fn hex_run(bytes: &[u8], at: usize) -> usize {
    bytes[at..].iter().take_while(|b| is_hex(**b)).count()
}

/// Bare `0x` + exactly 6 or 8 hex digits, boundary-guarded on both sides.
///
/// The 6-or-8 anchor is what makes this rule affordable: 2-digit (`0xff`
/// alpha, `0x89` PNG magic) and 4-digit (`0xfc00` IPv6 prefixes) hex stays
/// legal, and both appear in this crate for reasons that have nothing to do
/// with colour.
fn bare_hex_hit(line: &str) -> Option<String> {
    let b = line.as_bytes();
    for i in 0..b.len().saturating_sub(1) {
        if b[i] != b'0' || (b[i + 1] != b'x' && b[i + 1] != b'X') {
            continue;
        }
        if i > 0 && is_word(b[i - 1]) {
            continue;
        }
        let n = hex_run(b, i + 2);
        if n == 6 || n == 8 {
            return Some(line[i..i + 2 + n].to_string());
        }
    }
    None
}

/// A CSS hex colour — `#` + 3, 4, 6 or 8 hex digits — boundary-guarded.
///
/// The rule the GPUI gate could not have: in a WebView build a colour is a
/// *string*, so `style: "border-color:#d0d7de"` is the natural way to bypass
/// the token set and the compiler has nothing to say about it.
///
/// The right-hand guard rejects `-` as well as word characters, so an id
/// selector whose name happens to start with hex letters (`#abc-panel`) is not
/// a colour. `#[derive(…)]`, `#!`, `r#"…"#` and `r#type` all fail the
/// hex-run test and never reach it.
fn css_hex_hit(line: &str) -> Option<String> {
    let b = line.as_bytes();
    for i in 0..b.len() {
        if b[i] != b'#' {
            continue;
        }
        if i > 0 && (is_word(b[i - 1]) || b[i - 1] == b'#') {
            continue;
        }
        let n = hex_run(b, i + 1);
        if !matches!(n, 3 | 4 | 6 | 8) {
            continue;
        }
        let after = b.get(i + 1 + n).copied();
        if after.is_some_and(|c| is_word(c) || c == b'-') {
            continue;
        }
        return Some(line[i..i + 1 + n].to_string());
    }
    None
}

/// One of [`BANNED_CTORS`] used as a call: a non-word character, the exact
/// name, then `(`.
fn ctor_hit(line: &str) -> Option<String> {
    let b = line.as_bytes();
    for name in BANNED_CTORS {
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(name) {
            let i = from + rel;
            from = i + 1;
            if i > 0 && is_word(b[i - 1]) {
                continue;
            }
            if b.get(i + name.len()) == Some(&b'(') {
                return Some(format!("{name}("));
            }
        }
    }
    None
}

/// The first banned pattern on `line`, if any.
fn banned_hit(line: &str) -> Option<String> {
    ctor_hit(line)
        .or_else(|| bare_hex_hit(line))
        .or_else(|| css_hex_hit(line))
}

/// The reason text inside `style-lint: allow(<reason>)`, or `None` when the
/// line carries no well-formed marker (a marker with no closing paren does not
/// excuse anything).
fn allow_reason(line: &str) -> Option<&str> {
    let rest = line.split_once(ALLOW_MARKER)?.1;
    let end = rest.find(')')?;
    Some(rest[..end].trim())
}

/// Scan `lines` (1-based numbers already resolved) with `hit`.
fn scan_lines(lines: &[(usize, &str)], hit: impl Fn(&str) -> Option<String>) -> ScanResult {
    let mut out = ScanResult::default();
    for (line_no, line) in lines {
        match (hit(line), allow_reason(line)) {
            (_, Some("")) => out.empty_reasons.push(*line_no),
            (Some(_), Some(_)) => {} // excused, with a reason
            (Some(pattern), None) => out.violations.push(Violation {
                line_no: *line_no,
                pattern,
                text: line.trim().to_string(),
            }),
            (None, Some(_)) => out.stale_allows.push(*line_no),
            (None, None) => {}
        }
    }
    out
}

/// Convenience for the unit tests: scan a whole source string.
fn scan(src: &str) -> ScanResult {
    let numbered: Vec<(usize, &str)> = src.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
    scan_lines(&numbered, banned_hit)
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir must succeed") {
        let path = entry.expect("dir entry must resolve").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_scanner_flags_colour_constructors_css_hex_and_bare_hex_only() {
    // (source line, expected violation count)
    let cases: &[(&str, usize)] = &[
        // The Dioxus-era bypass: a colour inside a style string.
        (r##"            style: "background: #f5a623","##, 1),
        (r##"            style: "color: rgba(0, 0, 0, 0.5)","##, 1),
        (r##"    let c = "#fff";"##, 1),
        (r##"    let c = "#ffff";"##, 1),
        (r##"    let c = "#12345678";"##, 1),
        // The GPUI-era shapes, still banned.
        ("pub const FOCUS_RING: u32 = 0x3b82f6;", 1),
        ("        .bg(gpui::rgba(0x80808022))", 1),
        ("                .text_color(white())", 1),
        ("    let c = plotters::style::RGBColor(1, 2, 3);", 1),
        ("    let c = parse_hex(&tokens.canvas);", 1),
        // Not colours.
        ("    area.fill(&WHITE)?;", 0), // plotters CONST, not a call
        (
            "        assert_eq!(&bytes[0..4], &[0x89, b'P', b'N', b'G']);",
            0,
        ),
        (
            "            IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1)),",
            0,
        ),
        ("        Some((r, g, b, 0xff))", 0),
        ("        let x = self.ring.opacity(0.13);", 0),
        // Boundary guards. Each of these would fire on a plain substring
        // match, and each is real Rust that appears in this crate's idiom.
        ("    let v = to_srgb(value);", 0),
        ("fn parse_hexdump(bytes: &[u8]) {", 0),
        ("#[derive(Clone, Copy, PartialEq, Debug)]", 0),
        ("    let s = r#\"a raw string\"#;", 0),
        ("        div { id: \"#main\", }", 0),
        ("    class: \"d0-pane\", // see #abc-panel", 0),
        ("        tracing::warn!(\"{e:#}\");", 0),
    ];
    for (line, expected) in cases {
        let res = scan(line);
        assert_eq!(
            res.violations.len(),
            *expected,
            "wrong verdict for: {line}\n{res:?}"
        );
    }
}

#[test]
fn an_escape_comment_suppresses_and_is_itself_ratcheted() {
    // A reasoned escape suppresses the violation.
    let excused = r##"    let c = "#112233"; // style-lint: allow(plotters needs a raw RGB)"##;
    let res = scan(excused);
    assert!(res.violations.is_empty(), "reasoned escape must suppress");
    assert!(res.stale_allows.is_empty());
    assert!(res.empty_reasons.is_empty());

    // The same marker inside a CSS block comment, because `app.css` has no
    // `//` comments and one marker must serve both files.
    let css = "  color: #f5a623; /* style-lint: allow(the logomark knockout) */";
    assert!(
        scan(css).violations.is_empty(),
        "a CSS escape must suppress"
    );

    // An escape with no banned pattern is stale and must fail.
    let stale = "    let c = theme.ring; // style-lint: allow(leftover)";
    assert_eq!(
        scan(stale).stale_allows,
        vec![1],
        "stale escape must be reported"
    );

    // An escape with no reason must fail.
    let no_reason = r##"    let c = "#112233"; // style-lint: allow()"##;
    assert_eq!(
        scan(no_reason).empty_reasons,
        vec![1],
        "empty reason must be reported"
    );

    // A marker with no closing paren excuses nothing.
    let malformed = r##"    let c = "#112233"; // style-lint: allow(unterminated"##;
    assert_eq!(
        scan(malformed).violations.len(),
        1,
        "malformed marker must not excuse"
    );
}

/// The ratchet's over/under-budget arithmetic, pulled out of the test so both
/// halves can be exercised directly against synthetic maps rather than only
/// ever running in their passing (i.e. silent) state against the real tree.
///
/// `counts` holds the observed violation count per file that has at least one
/// (files with zero are simply absent, matching how the caller builds it);
/// `allow` is the ratchet table; `detail` holds the pre-formatted per-line
/// breakdown for files present in `counts`.
fn ratchet_report(
    counts: &BTreeMap<String, usize>,
    allow: &BTreeMap<&str, usize>,
    detail: &BTreeMap<String, String>,
) -> String {
    let mut errors = String::new();

    // Over budget → a new literal entered the tree.
    for (rel, found) in counts {
        let budget = allow.get(rel.as_str()).copied().unwrap_or(0);
        if *found > budget {
            errors.push_str(&format!(
                "\n{rel}: {found} colour-literal lines, allowance {budget} — {} new.\n\
                 Read the value from a `--d0-` custom property instead: a class in\n\
                 `assets/app.css`, or `var(--d0-…)` in the `style:` string. Add\n\
                 `style-lint: allow(reason)` only if the colour is genuinely not a\n\
                 theme colour.\n{}",
                found - budget,
                detail.get(rel).map(String::as_str).unwrap_or("")
            ));
        }
    }

    // Under budget → the ratchet was not tightened after a migration.
    for (rel, budget) in allow {
        let found = counts.get(*rel).copied().unwrap_or(0);
        if found < *budget {
            errors.push_str(&format!(
                "\n{rel}: down to {found} colour-literal lines but the table says {budget}.\n\
                 Lower it to {found} (or remove the entry if it is 0).\n"
            ));
        }
    }

    errors
}

#[test]
fn the_ratchet_report_covers_over_under_and_at_budget() {
    // observed > allowance → non-empty, names the file and the new count.
    let counts = BTreeMap::from([("over.rs".to_string(), 3usize)]);
    let allow = BTreeMap::from([("over.rs", 1usize)]);
    let detail = BTreeMap::from([(
        "over.rs".to_string(),
        "    src/over.rs:1: banned `rgb(` — ...\n".to_string(),
    )]);
    let report = ratchet_report(&counts, &allow, &detail);
    assert!(!report.is_empty(), "over-budget must produce a report");
    assert!(report.contains("over.rs"), "report must name the file");
    assert!(
        report.contains("2 new"),
        "report must state how many are new: {report}"
    );

    // observed < allowance → non-empty, contains the actionable instruction.
    let counts = BTreeMap::from([("under.rs".to_string(), 1usize)]);
    let allow = BTreeMap::from([("under.rs", 4usize)]);
    let report = ratchet_report(&counts, &allow, &BTreeMap::new());
    assert!(!report.is_empty(), "under-budget must produce a report");
    assert!(
        report.contains("Lower it to 1"),
        "report must give the actionable instruction: {report}"
    );

    // observed == allowance, plus a file absent from the table with zero
    // violations (so it is absent from `counts` too) → report is empty.
    let counts = BTreeMap::from([("exact.rs".to_string(), 2usize)]);
    let allow = BTreeMap::from([("exact.rs", 2usize)]);
    let detail = BTreeMap::from([("exact.rs".to_string(), String::new())]);
    assert!(
        ratchet_report(&counts, &allow, &detail).is_empty(),
        "on-budget plus an unlisted clean file must report nothing"
    );
}

#[test]
fn the_ui_source_holds_no_unallowed_colour_literals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    files.sort();
    assert!(
        files.len() > 40,
        "walk found only {} files under {} — the walk is broken",
        files.len(),
        root.display()
    );

    let allow: BTreeMap<&str, usize> = ALLOW.iter().copied().collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut detail: BTreeMap<String, String> = BTreeMap::new();
    let mut errors = String::new();

    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .expect("path is under src/")
            .to_string_lossy()
            .replace('\\', "/");
        let src = fs::read_to_string(path).expect("source must be readable");
        // `#[cfg(test)]` regions are excluded, unlike the GPUI original. A unit
        // test asserting that the editor palette carries `#bc8cff`
        // (`sql_console/editor.rs`) is the token system working, not a bypass
        // of it — and the same exclusion is what the panic rule below needs
        // anyway, so there is one notion of "shipped code" rather than two.
        let res = scan_lines(&non_test_lines(&src), banned_hit);

        for line_no in &res.stale_allows {
            errors.push_str(&format!(
                "src/{rel}:{line_no}: STALE `style-lint: allow` — no banned pattern on this line; delete the comment\n"
            ));
        }
        for line_no in &res.empty_reasons {
            errors.push_str(&format!(
                "src/{rel}:{line_no}: `style-lint: allow()` needs a non-empty reason\n"
            ));
        }
        if !res.violations.is_empty() {
            let mut lines = String::new();
            for v in &res.violations {
                lines.push_str(&format!(
                    "    src/{rel}:{}: banned `{}` — {}\n",
                    v.line_no, v.pattern, v.text
                ));
            }
            detail.insert(rel.clone(), lines);
            counts.insert(rel, res.violations.len());
        }
    }

    errors.push_str(&ratchet_report(&counts, &allow, &detail));

    assert!(errors.is_empty(), "\nstyle-lint failures:\n{errors}");
}

// ===========================================================================
// The stylesheet
// ===========================================================================

/// Inclusive 1-based line ranges of every `:root{…}` block in `css`.
///
/// A hex literal inside one is a custom-property *definition* — the only place
/// a colour is allowed to be written down. Everywhere else it is a rule that
/// walked around the theme.
///
/// Brace-depth tracked from the block's opening `{`, so a nested `@media`
/// wrapper or a rule that follows on the same line cannot confuse it.
fn root_ranges(css: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut open: Option<(usize, i32)> = None;
    for (i, line) in css.lines().enumerate() {
        let line_no = i + 1;
        match open {
            None => {
                if let Some(at) = line.find(":root") {
                    let tail = &line[at..];
                    let depth = tail.matches('{').count() as i32 - tail.matches('}').count() as i32;
                    if tail.contains('{') {
                        if depth <= 0 {
                            out.push((line_no, line_no));
                        } else {
                            open = Some((line_no, depth));
                        }
                    }
                }
            }
            Some((start, depth)) => {
                let d = depth + line.matches('{').count() as i32 - line.matches('}').count() as i32;
                if d <= 0 {
                    out.push((start, line_no));
                    open = None;
                } else {
                    open = Some((start, d));
                }
            }
        }
    }
    if let Some((start, _)) = open {
        out.push((start, css.lines().count()));
    }
    out
}

fn in_any_range(ranges: &[(usize, usize)], line_no: usize) -> bool {
    ranges.iter().any(|(a, b)| line_no >= *a && line_no <= *b)
}

/// A colour literal in CSS: a hex, or one of the colour functions.
fn css_colour_hit(line: &str) -> Option<String> {
    css_hex_hit(line).or_else(|| ctor_hit(line))
}

#[test]
fn the_css_scanner_exempts_root_declarations_and_nothing_else() {
    let css = "\
:root {
  --d0-accent: #03459b;
  --d0-shadow-pane: 0 8px 24px rgba(31, 35, 40, 0.10);
}

.d0-tab.is-active {
  box-shadow: inset 0 2px 0 var(--d0-accent);
}

#main { overflow: hidden; }

.d0-cheat {
  color: #f5a623;
}
";
    let ranges = root_ranges(css);
    assert_eq!(
        ranges,
        vec![(1, 4)],
        "the :root block is lines 1-4: {ranges:?}"
    );

    let offenders: Vec<usize> = css
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(n, l)| css_colour_hit(l).is_some() && !in_any_range(&ranges, *n))
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        offenders,
        vec![13],
        "only the rule outside :root is a violation; #main and var() are not"
    );

    // A one-line `:root` block is still a block.
    let inline = ":root { --d0-accent: #03459b; }\n.x { color: #f5a623; }\n";
    assert_eq!(root_ranges(inline), vec![(1, 1)]);
}

#[test]
fn the_stylesheet_holds_no_colour_literal_outside_root() {
    let css_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/app.css");
    let css = fs::read_to_string(&css_path).expect("app.css must be readable");
    assert!(
        css.lines().count() > 500,
        "app.css is only {} lines — the read is broken, not the stylesheet",
        css.lines().count()
    );

    let ranges = root_ranges(&css);
    let scanned: Vec<(usize, &str)> = css
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(n, _)| !in_any_range(&ranges, *n))
        .collect();
    let res = scan_lines(&scanned, css_colour_hit);

    let mut errors = String::new();
    for line_no in &res.stale_allows {
        errors.push_str(&format!(
            "assets/app.css:{line_no}: STALE `style-lint: allow` — no colour literal on this line\n"
        ));
    }
    for line_no in &res.empty_reasons {
        errors.push_str(&format!(
            "assets/app.css:{line_no}: `style-lint: allow()` needs a non-empty reason\n"
        ));
    }

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut detail: BTreeMap<String, String> = BTreeMap::new();
    if !res.violations.is_empty() {
        let mut lines = String::new();
        for v in &res.violations {
            lines.push_str(&format!(
                "    assets/app.css:{}: banned `{}` — {}\n",
                v.line_no, v.pattern, v.text
            ));
        }
        detail.insert("app.css".to_string(), lines);
        counts.insert("app.css".to_string(), res.violations.len());
    }
    let allow: BTreeMap<&str, usize> = CSS_ALLOW.iter().copied().collect();
    errors.push_str(&ratchet_report(&counts, &allow, &detail));

    assert!(
        errors.is_empty(),
        "\napp.css style-lint failures:\n{errors}"
    );
}

// ===========================================================================
// No `.expect(` / `.unwrap(` on the render or page-fetch paths
// ===========================================================================
//
// The repo-wide rule is "no panics in non-test code", enforced by review. This
// makes it mechanical for the two trees where a panic is worst, re-pointed
// from `src/grid` + `src/window/render.rs` at their Dioxus successors:
// `src/components/grid` (every rendered cell, every page fetch, every edit)
// and `src/components/shell.rs` (the window root every other surface hangs
// off). A panic in either does not merely fail an operation — in a webview
// build it poisons the VirtualDom for the life of the window, and the user's
// only recovery is to quit.

/// Per-file allowance of `.expect(` / `.unwrap(` LINES in non-test code.
///
/// SHRINK-ONLY, and **empty**: both trees are at zero. Adding an entry means a
/// slice introduced a panic it could not turn into a `Result` or a graceful
/// `None` — a design question, not bookkeeping. The under-budget arm fires
/// too, so a later removal must lower the number in the same commit.
const PANIC_ALLOW: &[(&str, usize)] = &[];

/// Trees scanned by [`the_render_paths_hold_no_panic_constructors`], relative
/// to the crate root.
const PANIC_SCAN_ROOTS: &[&str] = &["src/components/grid", "src/components/shell.rs"];

/// Line-level panic constructors. Both are method calls, so the `(` is part of
/// the pattern — that keeps `unwrap_or`, `unwrap_or_else`, `unwrap_or_default`
/// and `expect_err` (all non-panicking or test-only) out of the match.
const PANIC_SUBSTRINGS: &[&str] = &[".expect(", ".unwrap("];

/// `true` when `line` is entirely a comment — `//`, `///`, `//!`, or a `*`
/// continuation inside a block comment.
///
/// Required, not cosmetic: the doc comments in these modules explain what each
/// site used to be, and a scanner that cannot tell prose from code would
/// forbid documenting the very thing it enforces. A trailing comment on a CODE
/// line is still scanned — that is what the `allow` escape is for.
fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with('*')
}

/// Line numbers (1-based) of `src` that are NOT inside a `#[cfg(test)]` item.
///
/// Tracks brace depth from the attribute's opening `{` so it handles a
/// `#[cfg(test)] mod tests { … }` at any position, not just at EOF. Only
/// `cfg(test)` is stripped: any other `cfg` can ship in a real binary, so its
/// panics count.
fn non_test_lines(src: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut skipping = false;
    let mut awaiting_open = false;
    let mut depth: i32 = 0;

    for (i, line) in src.lines().enumerate() {
        if !skipping && !awaiting_open && line.trim_start().starts_with("#[cfg(test)]") {
            awaiting_open = true;
            continue;
        }
        if awaiting_open {
            if line.contains('{') {
                depth = line.matches('{').count() as i32 - line.matches('}').count() as i32;
                awaiting_open = false;
                skipping = depth > 0;
            }
            continue;
        }
        if skipping {
            depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
            if depth <= 0 {
                skipping = false;
            }
            continue;
        }
        out.push((i + 1, line));
    }
    out
}

/// The first panic constructor on `line`, if any.
fn panic_hit(line: &str) -> Option<&'static str> {
    if is_comment_line(line) {
        return None;
    }
    PANIC_SUBSTRINGS
        .iter()
        .copied()
        .find(|pat| line.contains(pat))
}

/// Same over/under arithmetic as [`ratchet_report`], with the message this
/// rule needs. Kept separate rather than parameterised: the actionable advice
/// is what makes a gate failure fixable, and "use a theme token" is not it
/// here.
fn panic_ratchet_report(
    counts: &BTreeMap<String, usize>,
    allow: &BTreeMap<&str, usize>,
    detail: &BTreeMap<String, String>,
) -> String {
    let mut errors = String::new();

    for (rel, found) in counts {
        let budget = allow.get(rel.as_str()).copied().unwrap_or(0);
        if *found > budget {
            errors.push_str(&format!(
                "\n{rel}: {found} panic-constructor lines, allowance {budget} — {} new.\n\
                 A render or page-fetch path must degrade, not panic: return `Result`,\n\
                 use `let Some(x) = … else {{ return … }}`, or `debug_assert!(false, …)`\n\
                 plus a neutral value. Add `style-lint: allow(reason)` only if the\n\
                 panic is genuinely unreachable AND cheaper to prove than to remove.\n{}",
                found - budget,
                detail.get(rel).map(String::as_str).unwrap_or("")
            ));
        }
    }

    for (rel, budget) in allow {
        let found = counts.get(*rel).copied().unwrap_or(0);
        if found < *budget {
            errors.push_str(&format!(
                "\n{rel}: down to {found} panic-constructor lines but PANIC_ALLOW says {budget}.\n\
                 Lower PANIC_ALLOW[\"{rel}\"] to {found} (or remove the entry if it is 0).\n"
            ));
        }
    }

    errors
}

#[test]
fn the_panic_scanner_ignores_comments_tests_and_non_panicking_lookalikes() {
    // Real panics.
    assert_eq!(
        panic_hit("    let x = y.expect(\"boom\");"),
        Some(".expect(")
    );
    assert_eq!(panic_hit("    let x = y.unwrap();"), Some(".unwrap("));
    // Non-panicking lookalikes must NOT match.
    assert_eq!(panic_hit("    let x = y.unwrap_or(0);"), None);
    assert_eq!(panic_hit("    let x = y.unwrap_or_else(|| 0);"), None);
    assert_eq!(panic_hit("    let x = y.unwrap_or_default();"), None);
    // Prose describing a removed panic is prose.
    assert_eq!(panic_hit("/// used to be `.expect(\"poisoned\")`"), None);
    assert_eq!(
        panic_hit("    // .unwrap() here would brick the cache"),
        None
    );
    assert_eq!(panic_hit("     * .expect(\"x\") in a block comment"), None);

    // `#[cfg(test)]` regions are excluded wherever they sit, and code after the
    // region closes is scanned again.
    let src = "fn a() { ok() }\n\
               #[cfg(test)]\n\
               mod tests {\n\
               fn t() { z.unwrap(); }\n\
               }\n\
               fn b() { q.expect(\"live\"); }\n";
    let kept: Vec<usize> = non_test_lines(src)
        .into_iter()
        .filter(|(_, l)| panic_hit(l).is_some())
        .map(|(n, _)| n)
        .collect();
    assert_eq!(kept, vec![6], "only the post-test-module panic may be seen");
}

#[test]
fn the_panic_ratchet_report_covers_over_and_under() {
    let counts = BTreeMap::from([("grid/mod.rs".to_string(), 2usize)]);
    let allow: BTreeMap<&str, usize> = BTreeMap::new();
    let report = panic_ratchet_report(&counts, &allow, &BTreeMap::new());
    assert!(
        report.contains("grid/mod.rs"),
        "must name the file: {report}"
    );
    assert!(
        report.contains("2 new"),
        "must count the new ones: {report}"
    );

    let allow = BTreeMap::from([("grid/mod.rs", 3usize)]);
    let report = panic_ratchet_report(&BTreeMap::new(), &allow, &BTreeMap::new());
    assert!(
        report.contains("Lower PANIC_ALLOW[\"grid/mod.rs\"] to 0"),
        "stale-under must be actionable: {report}"
    );
}

#[test]
fn the_render_paths_hold_no_panic_constructors() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let mut files = Vec::new();
    for root in PANIC_SCAN_ROOTS {
        let path = crate_root.join(root);
        if path.is_dir() {
            collect_rs(&path, &mut files);
        } else {
            assert!(
                path.is_file(),
                "scan root {} does not exist",
                path.display()
            );
            files.push(path);
        }
    }
    files.sort();
    assert!(
        files.len() >= 5,
        "walk found only {} files under {PANIC_SCAN_ROOTS:?} — the walk is broken",
        files.len()
    );

    let allow: BTreeMap<&str, usize> = PANIC_ALLOW.iter().copied().collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut detail: BTreeMap<String, String> = BTreeMap::new();

    for path in &files {
        let rel = path
            .strip_prefix(crate_root.join("src/components"))
            .expect("path is under src/components/")
            .to_string_lossy()
            .replace('\\', "/");
        let src = fs::read_to_string(path).expect("source must be readable");

        let mut lines = String::new();
        let mut hits = 0usize;
        for (line_no, line) in non_test_lines(&src) {
            let Some(pattern) = panic_hit(line) else {
                continue;
            };
            // Same escape hatch as the colour ratchet, same mandatory reason.
            if allow_reason(line).is_some_and(|r| !r.is_empty()) {
                continue;
            }
            hits += 1;
            lines.push_str(&format!(
                "    src/components/{rel}:{line_no}: banned `{pattern}` — {}\n",
                line.trim()
            ));
        }
        if hits > 0 {
            detail.insert(rel.clone(), lines);
            counts.insert(rel, hits);
        }
    }

    let report = panic_ratchet_report(&counts, &allow, &detail);
    assert!(report.is_empty(), "\npanic-path lint failures:\n{report}");
}
