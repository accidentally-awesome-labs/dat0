//! B11: `src/window/` cannot regrow into another 8,672-line file.
//!
//! `window.rs` reached 8,672 lines because nothing ever objected. This is the
//! objection. Each module carries an explicit ceiling; the table fails in both
//! directions, and a module on disk with no entry fails too, so a fresh file
//! cannot quietly become the next dumping ground.
//!
//! Modelled on `tests/style_lint.rs`'s colour ratchet, including the lesson A4
//! recorded about it: the ratchet arithmetic needs its own test, because
//! otherwise the only version that ever runs is the silent passing one.

use std::collections::BTreeMap;
use std::path::Path;

/// Per-file line ceilings for `src/window/`. Set at B11 to
/// `(lines + 50) / 100 * 100 + 100` in integer arithmetic, which leaves every
/// module at least 51 lines of headroom on day one — enough that ordinary
/// edits never redden the ratchet, which is what gets a ratchet deleted.
///
/// Raising an entry is a deliberate act and belongs in the same commit as the
/// code that needs it, with the reason in the commit message. If a module is
/// pushing its ceiling, the answer is usually a new module rather than a
/// bigger number — that is the whole point of the slice that created this file.
const MAX_LINES: &[(&str, usize)] = &[
    ("ai.rs", 700),
    ("boot.rs", 1100),
    ("catalog_inspector.rs", 600),
    ("charts.rs", 700),
    ("connections.rs", 500),
    ("data_io.rs", 500),
    ("dock.rs", 900),
    ("live_refresh.rs", 500),
    ("mod.rs", 1000),
    ("modals.rs", 600),
    ("package_ops.rs", 700),
    ("render.rs", 900),
    ("sql.rs", 800),
    ("test_support.rs", 500),
    ("workspace_ops.rs", 500),
];

/// The master plan's B11 row asks for `window.rs` under 5k. `mod.rs` landed at
/// 928. Asserting the promise directly means it survives even if someone later
/// raises `MAX_LINES["mod.rs"]` without thinking about the target.
const MOD_RS_HARD_CAP: usize = 5_000;

/// Slack on the under-arm.
///
/// A line ceiling is not a target to converge on, so a file sitting a little
/// under its number is normal and must not fail. A file sitting *far* under
/// means the ceiling is stale and is silently holding open budget nobody is
/// using — which is how a ratchet quietly stops ratcheting.
const UNDER_SLACK: usize = 300;

/// Pure ratchet arithmetic, extracted so it can be tested against constructed
/// inputs rather than only ever running in its silent passing state.
fn ratchet_report(counts: &BTreeMap<String, usize>, allow: &BTreeMap<&str, usize>) -> String {
    let mut errors = String::new();

    for (rel, found) in counts {
        match allow.get(rel.as_str()) {
            None => errors.push_str(&format!(
                "\n{rel}: {found} lines but no MAX_LINES entry.\n\
                 Add one at {}. A new module with no ceiling is how the old\n\
                 window.rs happened.\n",
                (found + 50) / 100 * 100 + 100
            )),
            Some(budget) if found > budget => errors.push_str(&format!(
                "\n{rel}: {found} lines, ceiling {budget} — {} over.\n\
                 Extract a module, or raise the ceiling in this same commit and\n\
                 say why in the message.\n",
                found - budget
            )),
            Some(_) => {}
        }
    }

    for (rel, budget) in allow {
        let found = counts.get(*rel).copied().unwrap_or(0);
        if found == 0 {
            errors.push_str(&format!(
                "\n{rel}: in MAX_LINES but not on disk. Remove the entry.\n"
            ));
        } else if budget.saturating_sub(found) > UNDER_SLACK {
            errors.push_str(&format!(
                "\n{rel}: down to {found} lines but ceiling says {budget}.\n\
                 Lower MAX_LINES[\"{rel}\"] to {} — a stale ceiling holds open\n\
                 budget nobody is using.\n",
                (found + 50) / 100 * 100 + 100
            ));
        }
    }

    errors
}

fn count_lines(p: &Path) -> usize {
    std::fs::read_to_string(p)
        .unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
        .lines()
        .count()
}

#[test]
fn window_modules_stay_within_their_ceilings() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/window");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("src/window exists") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            let name = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            counts.insert(name, count_lines(&path));
        }
    }
    assert!(
        counts.len() >= 15,
        "walk found only {} modules under {} — the walk is broken",
        counts.len(),
        dir.display()
    );

    let mod_rs = counts.get("mod.rs").copied().expect("mod.rs exists");
    assert!(
        mod_rs <= MOD_RS_HARD_CAP,
        "window/mod.rs is {mod_rs} lines, over the {MOD_RS_HARD_CAP} cap B11 committed to"
    );

    let allow: BTreeMap<&str, usize> = MAX_LINES.iter().copied().collect();
    let report = ratchet_report(&counts, &allow);
    assert!(report.is_empty(), "{report}");
}

#[test]
fn ratchet_report_covers_over_under_missing_and_untabled() {
    // Over ceiling — a module grew.
    let counts = BTreeMap::from([("over.rs".to_string(), 450usize)]);
    let allow = BTreeMap::from([("over.rs", 400usize)]);
    let r = ratchet_report(&counts, &allow);
    assert!(r.contains("450 lines, ceiling 400 — 50 over"), "{r}");

    // Far under ceiling — the number was left high after a shrink.
    let counts = BTreeMap::from([("under.rs".to_string(), 50usize)]);
    let allow = BTreeMap::from([("under.rs", 400usize)]);
    let r = ratchet_report(&counts, &allow);
    assert!(r.contains("Lower MAX_LINES[\"under.rs\"] to 200"), "{r}");

    // Within slack — silent. This arm is why UNDER_SLACK exists: without it
    // every ordinary edit would redden the ratchet and it would get deleted.
    let counts = BTreeMap::from([("ok.rs".to_string(), 250usize)]);
    let allow = BTreeMap::from([("ok.rs", 400usize)]);
    assert!(ratchet_report(&counts, &allow).is_empty());

    // On disk, absent from the table.
    let counts = BTreeMap::from([("new.rs".to_string(), 120usize)]);
    let allow = BTreeMap::new();
    let r = ratchet_report(&counts, &allow);
    assert!(r.contains("no MAX_LINES entry"), "{r}");

    // In the table, gone from disk.
    let counts = BTreeMap::new();
    let allow = BTreeMap::from([("gone.rs", 400usize)]);
    let r = ratchet_report(&counts, &allow);
    assert!(r.contains("not on disk"), "{r}");
}
