//! Repo-wide style gate (UI redesign A4, master plan §5 row A4).
//!
//! Bans inline color construction in `crates/dat0-app/src/**/*.rs` so the A1-A3
//! token system cannot be bypassed, and ratchets the pre-A6 backlog DOWN: each
//! A6 sub-slice that migrates a file must lower that file's number here, and the
//! gate fails if a number is left too high.
//!
//! ## Why constructors are banned regardless of argument
//! The master plan's original sketch banned only the literal form (`rgb(0x`).
//! That misses `gpui::rgb(crate::a11y::FOCUS_RING)` — five real call sites where
//! the literal hides one `const` indirection away. Banning the constructor plus
//! bare 6/8-digit hex catches both halves.
//!
//! ## Why this file cannot self-match
//! The pattern table below is source code in `tests/`, and the walk covers `src/`
//! only. No concat-splitting needed (unlike the A2 in-module self-lint this
//! supersedes). Do not move this file into `src/`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Banned as plain substrings, regardless of argument.
/// `hsl(` is not a prefix of `hsla(` (the next char differs), so both are listed.
const BANNED_SUBSTRINGS: &[&str] = &[
    "rgb(",
    "rgba(",
    "hsla(",
    "hsl(",
    "white()",
    "black()",
    "parse_hex",
];

/// Per-line escape. The reason text is mandatory and must be non-empty.
const ALLOW_MARKER: &str = "// style-lint: allow(";

/// Files that still hold pre-A6 inline colors, with their EXACT current count of
/// offending LINES (a line with two constructors counts once).
///
/// SHRINK-ONLY RATCHET. Each A6 sub-slice that migrates a file lowers its number
/// in the same PR; the gate fails if a count is left too high *or* too low.
/// A file absent from this table has an allowance of 0.
const ALLOW: &[(&str, usize)] = &[
    ("a11y/mod.rs", 2),
    ("catalog/panel.rs", 4),
    ("charts/mod.rs", 2),
    ("charts/panel.rs", 2),
    ("empty_state.rs", 1),
    ("error_ux/banner.rs", 4),
    ("grid/mod.rs", 7),
    ("onboarding/mod.rs", 2),
    ("settings_ui/panel.rs", 1),
    ("view/pipeline_bar.rs", 9),
    ("view/query_library.rs", 1),
    ("window.rs", 1),
];

/// Bare `0x` + exactly 6 or 8 hex digits, boundary-guarded on both sides.
///
/// The 6-or-8 anchor is what makes this rule affordable: 2-digit (`0x89` PNG
/// magic, `0xE2` UTF-8 continuation bytes) and 4-digit (`0xfc00` IPv6 prefixes)
/// hex stays legal, so the only thing it catches in `src/` today is the one real
/// color const, `a11y/mod.rs` `FOCUS_RING`.
fn bare_hex_re() -> Regex {
    Regex::new(r"(?i)(^|[^0-9a-z_])0x[0-9a-f]{6}([0-9a-f]{2})?([^0-9a-f]|$)")
        .expect("bare-hex pattern must compile")
}

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

/// The first banned pattern on `line`, if any.
fn banned_hit(line: &str, hex: &Regex) -> Option<String> {
    for pat in BANNED_SUBSTRINGS {
        if line.contains(pat) {
            return Some((*pat).to_string());
        }
    }
    hex.find(line).map(|m| m.as_str().trim().to_string())
}

/// The reason text inside `// style-lint: allow(<reason>)`, or `None` when the
/// line carries no well-formed marker (a marker with no closing paren does not
/// excuse anything).
fn allow_reason(line: &str) -> Option<&str> {
    let rest = line.split_once(ALLOW_MARKER)?.1;
    let end = rest.find(')')?;
    Some(rest[..end].trim())
}

fn scan(src: &str, hex: &Regex) -> ScanResult {
    let mut out = ScanResult::default();
    for (i, line) in src.lines().enumerate() {
        let line_no = i + 1;
        match (banned_hit(line, hex), allow_reason(line)) {
            (_, Some("")) => out.empty_reasons.push(line_no),
            (Some(_), Some(_)) => {} // excused, with a reason
            (Some(pattern), None) => out.violations.push(Violation {
                line_no,
                pattern,
                text: line.trim().to_string(),
            }),
            (None, Some(_)) => out.stale_allows.push(line_no),
            (None, None) => {}
        }
    }
    out
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
fn scanner_flags_constructors_and_bare_hex_only() {
    let hex = bare_hex_re();
    // (source line, expected violation count)
    let cases: &[(&str, usize)] = &[
        ("        BannerKind::Info => gpui::rgb(0x3b82f6),", 1),
        ("pub const FOCUS_RING: u32 = 0x3b82f6;", 1),
        (
            "            .border_color(gpui::rgb(crate::a11y::FOCUS_RING));",
            1,
        ),
        ("                .text_color(gpui::white())", 1),
        ("        .bg(gpui::rgba(0x80808022))", 1),
        ("    area.fill(&WHITE)?;", 0), // plotters CONST, not a call
        (
            "        assert_eq!(&bytes[0..4], &[0x89, b'P', b'N', b'G']);",
            0,
        ),
        (
            "            IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1)),",
            0,
        ),
        ("        let x = self.ring.opacity(0.13);", 0),
    ];
    for (line, expected) in cases {
        let res = scan(line, &hex);
        assert_eq!(
            res.violations.len(),
            *expected,
            "wrong verdict for: {line}\n{res:?}"
        );
    }
}

#[test]
fn escape_comment_suppresses_and_ratchets() {
    let hex = bare_hex_re();

    // A reasoned escape suppresses the violation.
    let excused = "    let c = gpui::rgb(0x112233); // style-lint: allow(plotters needs a raw RGB)";
    let res = scan(excused, &hex);
    assert!(res.violations.is_empty(), "reasoned escape must suppress");
    assert!(res.stale_allows.is_empty());
    assert!(res.empty_reasons.is_empty());

    // An escape with no banned pattern is stale and must fail.
    let stale = "    let c = theme.ring; // style-lint: allow(leftover)";
    let res = scan(stale, &hex);
    assert_eq!(res.stale_allows, vec![1], "stale escape must be reported");

    // An escape with no reason must fail.
    let no_reason = "    let c = gpui::rgb(0x112233); // style-lint: allow()";
    let res = scan(no_reason, &hex);
    assert_eq!(res.empty_reasons, vec![1], "empty reason must be reported");

    // A marker with no closing paren excuses nothing.
    let malformed = "    let c = gpui::rgb(0x112233); // style-lint: allow(unterminated";
    let res = scan(malformed, &hex);
    assert_eq!(res.violations.len(), 1, "malformed marker must not excuse");
}

#[test]
fn src_holds_no_unallowed_color_literals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let hex = bare_hex_re();

    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    files.sort();
    assert!(
        files.len() > 50,
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
        let res = scan(&src, &hex);

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

    // Over budget → a new literal entered the tree.
    for (rel, found) in &counts {
        let budget = allow.get(rel.as_str()).copied().unwrap_or(0);
        if *found > budget {
            errors.push_str(&format!(
                "\n{rel}: {found} color-literal lines, allowance {budget} — {} new.\n\
                 Use a token from `crate::theme::tokens` (`cx.theme().d0()`), or add\n\
                 `// style-lint: allow(reason)` if the color is genuinely not a theme color.\n{}",
                found - budget,
                detail.get(rel).map(String::as_str).unwrap_or("")
            ));
        }
    }

    // Under budget → the ratchet was not tightened after a migration.
    for (rel, budget) in &allow {
        let found = counts.get(*rel).copied().unwrap_or(0);
        if found < *budget {
            errors.push_str(&format!(
                "\n{rel}: down to {found} color-literal lines but ALLOW says {budget}.\n\
                 Lower ALLOW[\"{rel}\"] to {found} (or remove the entry if it is 0).\n"
            ));
        }
    }

    assert!(errors.is_empty(), "\nstyle-lint failures:\n{errors}");
}
