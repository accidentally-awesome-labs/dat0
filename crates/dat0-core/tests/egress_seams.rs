//! SH1 privacy gate: every outbound HTTP call site is metered.
//!
//! The status bar renders `crate::telemetry::egress::total_sent()`, and the
//! marketing page turns that into a promise — `0 bytes left this machine`. A
//! promise backed by a counter is only as good as the counter's coverage, so
//! this is the coverage check: any file under `src/` that reaches for an HTTP
//! client must also carry a literal `// egress-seam` comment marking where it
//! records what it sent.
//!
//! ## Why a source scan and not a runtime assertion
//! There is no runtime moment at which "all network calls are accounted for"
//! is observable — an unmetered call is invisible precisely because it records
//! nothing. The only place the omission is visible is the source, at review
//! time, which is where this gate fires.
//!
//! ## Why the marker and not "calls `record_sent`"
//! `record_request` / `record_sent` / `note_unmetered_channel` are three
//! different right answers depending on the seam (see
//! `src/telemetry/egress.rs`), and a scanner enumerating function names would
//! have to grow every time a fourth is right. The comment is the contract: it
//! says a human decided what this seam sends. `connections/connect.rs` carries
//! one without touching an HTTP client at all — the MotherDuck extension's
//! socket is the seam this codebase can least afford to leave undocumented.
//!
//! Same shrink-only `ALLOW` shape as `tests/style_lint.rs`, same
//! over-AND-under-budget arithmetic, so an exemption cannot outlive its reason.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Substrings that mean "this file talks to the network".
///
/// Both are path-qualified (`::`) rather than bare crate names so a mention in
/// prose ("reqwest sets Content-Length itself") does not trip the gate — the
/// module doc of `telemetry/egress.rs` contains exactly such a mention.
const CLIENT_MARKERS: &[&str] = &["reqwest::", "ureq::"];

/// The marker a metered seam must carry.
const SEAM_MARKER: &str = "// egress-seam";

/// Files that use an HTTP client and are exempt from carrying a seam marker.
///
/// SHRINK-ONLY RATCHET, and **empty**. It exists so an exemption is a reviewed
/// act with a name attached rather than a silent hole, and the under-budget arm
/// fires too: an entry left behind after the file was metered fails the gate.
///
/// An entry here is a claim that a file reaches an HTTP client and sends
/// nothing — which for a request is never true. Expect to justify it.
const ALLOW: &[&str] = &[];

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

/// `true` when `src` reaches for an HTTP client.
fn uses_http_client(src: &str) -> bool {
    CLIENT_MARKERS.iter().any(|m| src.contains(m))
}

/// Over/under-budget arithmetic, pulled out of the test so both arms can be
/// exercised against synthetic input rather than only ever running in their
/// passing (i.e. silent) state against the real tree.
///
/// `unmarked` = files that use a client and carry no marker.
/// `marked` = files that use a client and DO carry one.
fn seam_report(unmarked: &[String], marked: &[String], allow: &BTreeMap<&str, ()>) -> String {
    let mut errors = String::new();

    for rel in unmarked {
        if allow.contains_key(rel.as_str()) {
            continue;
        }
        errors.push_str(&format!(
            "\nsrc/{rel}: uses an HTTP client but carries no `{SEAM_MARKER}` comment.\n\
             Record what this call sends via `crate::telemetry::egress` and mark the\n\
             call site, or — if the volume is genuinely unobservable from here —\n\
             call `note_unmetered_channel()` and say so in the comment.\n"
        ));
    }

    // Under budget: an exemption that is no longer needed. Same discipline as
    // `style_lint::ratchet_report` — a stale allowance is a hole waiting to be
    // reused by the next slice.
    for rel in allow.keys() {
        if marked.iter().any(|m| m == rel) {
            errors.push_str(&format!(
                "\nsrc/{rel}: now carries `{SEAM_MARKER}` but is still in ALLOW.\n\
                 Remove the ALLOW entry in this PR.\n"
            ));
        } else if !unmarked.iter().any(|u| u == rel) {
            errors.push_str(&format!(
                "\nsrc/{rel}: in ALLOW but no longer uses an HTTP client (or no longer exists).\n\
                 Remove the ALLOW entry in this PR.\n"
            ));
        }
    }

    errors
}

#[test]
fn scanner_matches_qualified_paths_only() {
    assert!(uses_http_client("let c = reqwest::Client::new();"));
    assert!(uses_http_client("    ureq::get(url)"));
    // Prose mentioning the crate without a path does not count — the egress
    // module doc names both clients while calling neither.
    assert!(!uses_http_client("//! reqwest sets Content-Length itself"));
    assert!(!uses_http_client("// the ureq call moved to check.rs"));
}

#[test]
fn seam_report_covers_missing_marker_and_both_stale_allow_arms() {
    let empty: BTreeMap<&str, ()> = BTreeMap::new();

    let missing = seam_report(&["net/a.rs".to_string()], &[], &empty);
    assert!(
        missing.contains("carries no `// egress-seam`"),
        "an unmarked client file must fail: {missing}"
    );

    let exempt: BTreeMap<&str, ()> = [("net/a.rs", ())].into_iter().collect();
    assert!(
        seam_report(&["net/a.rs".to_string()], &[], &exempt).is_empty(),
        "an ALLOW entry must silence its own file"
    );

    let now_marked = seam_report(&[], &["net/a.rs".to_string()], &exempt);
    assert!(
        now_marked.contains("still in ALLOW"),
        "a marked-but-allowed file must fail the under arm: {now_marked}"
    );

    let gone = seam_report(&[], &[], &exempt);
    assert!(
        gone.contains("no longer uses an HTTP client"),
        "an allowance for a file with no client must fail: {gone}"
    );
}

#[test]
fn every_network_call_site_is_metered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    files.sort();
    assert!(
        files.len() > 50,
        "walk found only {} files under {} — the walk is broken",
        files.len(),
        root.display()
    );

    let mut unmarked = Vec::new();
    let mut marked = Vec::new();
    for path in &files {
        let src = fs::read_to_string(path).expect("source must be readable");
        if !uses_http_client(&src) {
            continue;
        }
        let rel = path
            .strip_prefix(&root)
            .expect("path is under src/")
            .to_string_lossy()
            .replace('\\', "/");
        if src.contains(SEAM_MARKER) {
            marked.push(rel);
        } else {
            unmarked.push(rel);
        }
    }

    // Teeth: if the walk stops finding network files at all, the gate has
    // silently become a no-op and would pass a tree full of unmetered calls.
    assert!(
        marked.len() + unmarked.len() >= 5,
        "expected at least 5 files using an HTTP client, found {} — the scan is broken",
        marked.len() + unmarked.len()
    );

    let allow: BTreeMap<&str, ()> = ALLOW.iter().map(|f| (*f, ())).collect();
    let errors = seam_report(&unmarked, &marked, &allow);
    assert!(errors.is_empty(), "\negress-seam failures:{errors}");
}
