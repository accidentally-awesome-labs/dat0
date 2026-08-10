//! `Reader::open` against deliberately malformed packages.
//!
//! A `.dat0` is a file users receive from other people. Every one of the cases
//! below is therefore reachable by an ordinary user, and the contract for all
//! of them is identical and narrow: return a **typed** [`FormatError`], never
//! panic, never write anything.
//!
//! The existing suite (`tests/reader.rs:103`, `:170`) covered exactly two
//! hand-built archives — a missing `charts.json` and a future major version.
//! These five cover the rest of the surface: truncation, a missing manifest,
//! a tampered payload, an unreadable version, and path traversal.
//!
//! Every assertion is on the `FormatError` VARIANT, not on message text — the
//! message is prose and may be reworded; the variant is the contract.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use dat0_format::*;

// ── Construction helpers ────────────────────────────────────────────────────
//
// Modelled on the hand-built zip at `tests/reader.rs:138-161`, generalized so
// each case below states only what it wants to be WRONG.

/// Write a zip containing exactly `entries`, in order, with no validation.
///
/// `ZipWriter::start_file` stores the name verbatim — unlike
/// `start_file_from_path`, which normalizes `..` (zip-8.6.0
/// `src/write.rs:1586-1588`). That is what makes the traversal case below
/// constructible at all, and it is also why the reader cannot trust a name.
fn build_zip(path: &Path, entries: &[(String, Vec<u8>)]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file(name.as_str(), opts).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

fn sha(bytes: &[u8]) -> String {
    format!("sha256:{:x}", <sha2::Sha256 as sha2::Digest>::digest(bytes))
}

fn entry(name: &str, bytes: Vec<u8>) -> (String, Vec<u8>) {
    (name.to_string(), bytes)
}

/// The five JSON sidecars of an empty-but-well-formed package, in the order
/// `Reader::open` reads them.
fn sidecars() -> Vec<(String, Vec<u8>)> {
    vec![
        entry(
            "recipe.json",
            serde_json::to_vec_pretty(&Recipe { tables: vec![] }).unwrap(),
        ),
        entry(
            "sources.json",
            serde_json::to_vec_pretty(&Sources { sources: vec![] }).unwrap(),
        ),
        entry(
            "views.json",
            serde_json::to_vec_pretty(&Views { views: vec![] }).unwrap(),
        ),
        entry(
            "queries.json",
            serde_json::to_vec_pretty(&Queries { queries: vec![] }).unwrap(),
        ),
        entry(
            "charts.json",
            serde_json::to_vec_pretty(&Charts { charts: vec![] }).unwrap(),
        ),
    ]
}

fn manifest_bytes(format_version: u32, checksums: BTreeMap<String, String>) -> Vec<u8> {
    let m = PackageManifest {
        format_version,
        kind: PACKAGE_KIND.into(),
        dat0_version: "0.0.0".into(),
        package_id: uuid::Uuid::now_v7(),
        workspace_id: uuid::Uuid::now_v7(),
        created_at: "2026-08-08T00:00:00Z".into(),
        table_count: 0,
        checksums,
    };
    serde_json::to_vec_pretty(&m).unwrap()
}

/// A package that opens cleanly. Each test below derives its malformed case
/// from this one so the ONLY difference is the defect under test.
fn well_formed(path: &Path) {
    let side = sidecars();
    let mut checksums = BTreeMap::new();
    for (name, bytes) in &side {
        checksums.insert(name.clone(), sha(bytes));
    }
    let mut entries = vec![entry(
        "manifest.json",
        manifest_bytes(FORMAT_VERSION, checksums),
    )];
    entries.extend(side);
    build_zip(path, &entries);
}

// ── The control ─────────────────────────────────────────────────────────────

#[test]
fn control_the_hand_built_package_opens() {
    // Without this, a test below could pass because the BUILDER is broken
    // rather than because the reader rejected the defect.
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("ok.dat0");
    well_formed(&p);
    Reader::open(&p).expect("the hand-built control package must open");
}

// ── 1. Truncated zip ────────────────────────────────────────────────────────

#[test]
fn truncated_zip_is_a_typed_error() {
    let tmp = tempfile::tempdir().unwrap();
    let good = tmp.path().join("good.dat0");
    well_formed(&good);

    // Halving the file destroys the end-of-central-directory record, which is
    // the realistic shape of an interrupted download or a partial copy.
    let bytes = std::fs::read(&good).unwrap();
    assert!(bytes.len() > 64, "control package should be non-trivial");
    let truncated = tmp.path().join("truncated.dat0");
    std::fs::write(&truncated, &bytes[..bytes.len() / 2]).unwrap();

    let err = Reader::open(&truncated).expect_err("a truncated zip must not open");
    assert!(
        matches!(err, FormatError::Zip(_)),
        "expected FormatError::Zip, got: {err:?}"
    );
}

// ── 2. manifest.json absent ─────────────────────────────────────────────────

#[test]
fn missing_manifest_is_file_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("no-manifest.dat0");
    // Sidecars only — a structurally valid zip that is not a package.
    build_zip(&p, &sidecars());

    let err = Reader::open(&p).expect_err("a package with no manifest must not open");
    assert!(
        matches!(err, FormatError::Zip(zip::result::ZipError::FileNotFound)),
        "expected FormatError::Zip(FileNotFound), got: {err:?}"
    );
}

// ── 3. recipe.json does not match its recorded sha256 ───────────────────────

#[test]
fn tampered_recipe_is_a_checksum_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("tampered.dat0");

    let side = sidecars();
    let mut checksums = BTreeMap::new();
    for (name, bytes) in &side {
        // Record the sha of a DIFFERENT payload for recipe.json, leaving the
        // real recipe.json in the archive. This is the shape of an edited
        // package whose manifest was not (or could not be) re-signed.
        if name == "recipe.json" {
            checksums.insert(name.clone(), sha(b"{\"tables\":[\"not the real recipe\"]}"));
        } else {
            checksums.insert(name.clone(), sha(bytes));
        }
    }
    let mut entries = vec![entry(
        "manifest.json",
        manifest_bytes(FORMAT_VERSION, checksums),
    )];
    entries.extend(side);
    build_zip(&p, &entries);

    let err = Reader::open(&p).expect_err("a recipe.json/manifest sha disagreement must not open");
    match err {
        FormatError::ChecksumMismatch { entry } => {
            assert_eq!(entry, "recipe.json", "must name the entry that failed");
        }
        other => panic!("expected FormatError::ChecksumMismatch, got: {other:?}"),
    }
}

// ── 4. format_version far in the future ─────────────────────────────────────

#[test]
fn unreadable_format_version_is_rejected_before_anything_else() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("v999.dat0");

    // Deliberately paired with EMPTY checksums and no sidecars: the version
    // check must fire before the reader tries to parse or verify anything,
    // otherwise a future package surfaces as a confusing parse error instead
    // of "this dat0 is too old" (reader.rs:23-26).
    build_zip(
        &p,
        &[entry("manifest.json", manifest_bytes(999, BTreeMap::new()))],
    );

    let err = Reader::open(&p).expect_err("format_version 999 must not open");
    assert!(
        matches!(
            err,
            FormatError::UnsupportedVersion {
                found: 999,
                // Fully qualified so this is unambiguously a CONST pattern.
                // A bare `FORMAT_VERSION` that failed to resolve would become
                // an irrefutable binding and the assertion would silently
                // stop checking anything.
                supported: dat0_format::FORMAT_VERSION
            }
        ),
        "expected FormatError::UnsupportedVersion{{found:999}}, got: {err:?}"
    );
}

// ── 5. Path traversal ───────────────────────────────────────────────────────

#[test]
fn entry_escaping_the_root_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("traversal.dat0");

    // `ParsedPackage::extract_data_to` builds its output path as
    // `dir.join("data").join(name.strip_prefix("data/"))`. With this name that
    // resolves to `<dir>/data/../../evil.parquet` — two levels ABOVE the
    // directory the user chose. The package is otherwise completely valid,
    // which is the point: nothing else about it would raise a flag.
    let side = sidecars();
    let mut checksums = BTreeMap::new();
    for (name, bytes) in &side {
        checksums.insert(name.clone(), sha(bytes));
    }
    let evil = "data/../../evil.parquet";
    let evil_bytes = b"pwned".to_vec();
    checksums.insert(evil.to_string(), sha(&evil_bytes));

    let mut entries = vec![entry(
        "manifest.json",
        manifest_bytes(FORMAT_VERSION, checksums),
    )];
    entries.extend(side);
    entries.push(entry(evil, evil_bytes));
    build_zip(&p, &entries);

    let err = Reader::open(&p).expect_err("an escaping entry name must not open");
    match err {
        FormatError::UnsafeEntryPath { entry } => {
            assert_eq!(entry, evil, "must name the offending entry");
        }
        other => panic!("expected FormatError::UnsafeEntryPath, got: {other:?}"),
    }

    // And nothing was written outside the package: the rejection happens at
    // `open`, so no `ParsedPackage` exists to extract from in the first place.
    assert!(
        !tmp.path().join("evil.parquet").exists(),
        "reader must not have materialized anything"
    );
}

#[test]
fn absolute_entry_paths_are_rejected_too() {
    // Same class, different escape: `Path::join` with an absolute component
    // DISCARDS the base entirely rather than walking up from it, so this is
    // strictly worse than `..` and must be covered by the same guard.
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("absolute.dat0");

    let side = sidecars();
    let mut checksums = BTreeMap::new();
    for (name, bytes) in &side {
        checksums.insert(name.clone(), sha(bytes));
    }
    let mut entries = vec![entry(
        "manifest.json",
        manifest_bytes(FORMAT_VERSION, checksums),
    )];
    entries.extend(side);
    entries.push(entry("/tmp/evil.parquet", b"pwned".to_vec()));
    build_zip(&p, &entries);

    let err = Reader::open(&p).expect_err("an absolute entry name must not open");
    assert!(
        matches!(err, FormatError::UnsafeEntryPath { .. }),
        "expected FormatError::UnsafeEntryPath, got: {err:?}"
    );
}
