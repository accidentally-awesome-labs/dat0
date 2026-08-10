//! `src/components/` cannot regrow into another 8,672-line file.
//!
//! The GPUI build's `window.rs` reached 8,672 lines because nothing ever
//! objected. This is the objection, carried across the migration and re-pointed
//! at the surface that inherited the risk: one component file per surface, each
//! with an explicit ceiling.
//!
//! The table fails in both directions, and a file on disk with no entry fails
//! too, so a fresh module cannot quietly become the next dumping ground.
//!
//! Modelled on `tests/style_lint.rs`'s colour ratchet, including the lesson
//! that ratchet arithmetic needs its own test — otherwise the only version that
//! ever runs is the silent passing one.

use std::collections::BTreeMap;
use std::path::Path;

/// Per-file line ceilings for `src/components/`, keyed by path relative to that
/// directory (nested, because unlike `window/` this tree has subdirectories).
///
/// Measured 2026-08-10, at the end of the GPUI to Dioxus migration, and set to
/// `(lines + 50) / 100 * 100 + 100` — at least 51 lines of headroom on day one,
/// enough that ordinary edits never redden the ratchet, which is what gets a
/// ratchet deleted.
///
/// Raising an entry is a deliberate act and belongs in the same commit as the
/// code that needs it, with the reason in the message. If a module is pushing
/// its ceiling the answer is usually a new module rather than a bigger number.
const MAX_LINES: &[(&str, usize)] = &[
    ("about.rs", 300),
    ("ai.rs", 900),
    ("banner.rs", 300),
    ("charts.rs", 700),
    ("command_palette.rs", 600),
    ("connections.rs", 600),
    ("crash_report.rs", 300),
    ("dock.rs", 300),
    ("empty_state.rs", 500),
    ("export_dialog.rs", 500),
    ("filter_popover.rs", 600),
    ("grid/cell_editor.rs", 300),
    ("grid/context_menu.rs", 300),
    ("grid/header.rs", 300),
    ("grid/mod.rs", 700),
    ("import_progress.rs", 400),
    ("import_wizard.rs", 900),
    ("inspector.rs", 900),
    ("live_refresh.rs", 200),
    ("mod.rs", 300),
    ("modals.rs", 900),
    ("name_prompt.rs", 300),
    ("onboarding.rs", 300),
    ("pane.rs", 200),
    ("pipeline_bar.rs", 300),
    ("query_library.rs", 300),
    ("recovery.rs", 500),
    ("saved_queries.rs", 200),
    ("settings_ui.rs", 900),
    ("shell.rs", 1100),
    ("sidebar.rs", 600),
    ("sql_console/editor.rs", 400),
    ("sql_console/mod.rs", 700),
    ("sql_console/tabs.rs", 400),
    ("update_ui.rs", 400),
    ("workspace_in_use.rs", 300),
];

/// `shell.rs` is the one that inherited `window.rs`'s job, so it carries the
/// promise directly: asserting it here means the cap survives even if someone
/// later raises its `MAX_LINES` entry without thinking about the target.
const SHELL_HARD_CAP: usize = 1_500;

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

/// Collect every `.rs` under `dir`, keyed by path relative to `base`.
fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, usize>) {
    for entry in std::fs::read_dir(dir).expect("components dir exists") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            walk(base, &path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let rel = path
                .strip_prefix(base)
                .expect("under base")
                .to_string_lossy()
                .into_owned();
            out.insert(rel, count_lines(&path));
        }
    }
}

fn count_lines(p: &Path) -> usize {
    std::fs::read_to_string(p)
        .unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
        .lines()
        .count()
}

#[test]
fn component_modules_stay_within_their_ceilings() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/components");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    // Recursive, unlike the `window/` original: `components/` has
    // subdirectories, and a flat walk would leave `grid/` and `sql_console/`
    // permanently unratcheted — the exact hole the table exists to close.
    walk(&dir, &dir, &mut counts);
    assert!(
        counts.len() >= 15,
        "walk found only {} modules under {} — the walk is broken",
        counts.len(),
        dir.display()
    );

    let shell = counts.get("shell.rs").copied().expect("shell.rs exists");
    assert!(
        shell <= SHELL_HARD_CAP,
        "components/shell.rs is {shell} lines, over the {SHELL_HARD_CAP} cap"
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

/// Field count of `pub struct WorkspaceShell` (MT1).
///
/// The file-size ratchet above caps the FILE; nothing capped the STRUCT, and it
/// reached **76** fields spanning nine unrelated domains — twelve of them six
/// copy-pasted `Option<Entity<T>>`/`Option<Subscription>` modal pairs whose
/// mutual exclusion no type enforced. MT1 collapsed those into one `modal_slot`
/// and MX1 added two, landing at 65.
///
/// Raised to 66 by MX2 for `perf_run`, which is `#[cfg(feature = "perf-harness")]`
/// and therefore absent from every shipped build — but this parser reads the
/// SOURCE, not a compiled struct, so it counts regardless. Deliberate: a
/// feature-gated field is still a field the next reader has to understand.
///
/// Raised to 69 by MT2 (+2: `last_persisted_layout`, `dock_persist_task` — the
/// debounced dock-resize persistence) and SH3 (+1: `hero_state`, the cached
/// `recents.json` + `settings.toml` snapshot that replaced a per-frame read of
/// both). SH3 is deliberately ONE field and not two: the two facts are read
/// from the same `config_dir()` call and refreshed together, so
/// `empty_state::HeroState` owns them jointly rather than the shell holding two
/// loose bools.
///
/// Carried across the migration and re-pointed: the GPUI `WorkspaceShell` held
/// ~71 fields across nine domains, and `state::Workspace` is deliberately much
/// smaller because a Dioxus surface keeps its own state in its own component
/// rather than in one shared struct. The ceiling is the current count, so the
/// shrink is locked in and the old struct cannot reassemble itself here.
///
/// Measured, not estimated. Fails over AND stale-under, like `MAX_LINES` — a
/// slice that removes fields must lower this in the same commit, which is what
/// makes shrinking visible rather than silently forgotten.
const MAX_WORKSPACE_FIELDS: usize = 13;

/// Slack on the field ratchet's under-arm.
///
/// Much tighter than [`UNDER_SLACK`]: a field is a deliberate act, not an
/// incidental line, so two of drift is generous and ten would let a whole
/// subsystem's worth of state leave without anyone noticing the ceiling was
/// stale.
const WORKSPACE_FIELDS_UNDER_SLACK: usize = 2;

/// Count top-level field declarations in a `pub struct NAME { … }` block.
///
/// Brace/bracket-depth aware, so the multi-line generic on `active_popover`
/// counts once rather than once per line, and comma-separated generic
/// arguments inside a type never count at all.
fn count_struct_fields(src: &str, decl: &str) -> usize {
    let start = src.find(decl).unwrap_or_else(|| panic!("{decl} not found"));
    let open = start + src[start..].find('{').expect("struct body opens");
    let mut depth = 0usize;
    let mut end = open;
    for (i, ch) in src[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body: String = src[open + 1..end]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut depth = 0i32;
    let mut fields = 0usize;
    let mut current = String::new();
    for ch in body.chars() {
        match ch {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
        if ch == ',' && depth == 0 {
            if !current.trim().is_empty() {
                fields += 1;
            }
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        fields += 1;
    }
    fields
}

#[test]
fn workspace_field_count_stays_within_its_ceiling() {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/state.rs"))
        .expect("read state.rs");
    let found = count_struct_fields(&src, "pub struct Workspace");

    assert!(
        found <= MAX_WORKSPACE_FIELDS,
        "Workspace has {found} fields, ceiling {MAX_WORKSPACE_FIELDS} — {} over.\n\
         State that only one surface reads belongs in that surface, not here —\n\
         that is why this struct is a fifth the size of the shell it replaced.\n\
         Raise the ceiling in this same commit and say why, or move the field.",
        found - MAX_WORKSPACE_FIELDS
    );
    assert!(
        MAX_WORKSPACE_FIELDS.saturating_sub(found) <= WORKSPACE_FIELDS_UNDER_SLACK,
        "Workspace is down to {found} fields but the ceiling says \
         {MAX_WORKSPACE_FIELDS}.\n\
         Lower MAX_WORKSPACE_FIELDS to {found} — a stale ceiling holds open budget\n\
         nobody is using, which is how a ratchet quietly stops ratcheting."
    );
}

/// The parser is the whole test; a silently-passing miscount would make the
/// ratchet decorative. These are the shapes `WorkspaceShell` actually contains.
#[test]
fn count_struct_fields_handles_generics_comments_and_multiline() {
    let src = r#"
pub struct Sample {
    // A plain line comment, not a field.
    /// A doc comment, also not a field.
    a: u8,
    /// Commas inside generics must not split a field.
    b: std::collections::HashMap<String, Vec<(usize, usize)>>,
    /// A multi-line generic — the `active_popover` shape.
    c: Option<
        Box<dyn Fn(&mut u8, &mut u8) -> bool>,
    >,
    /// No trailing comma on the last field.
    d: bool
}
"#;
    assert_eq!(count_struct_fields(src, "pub struct Sample"), 4);
}
