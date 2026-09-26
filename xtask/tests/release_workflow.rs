//! Step 7: `release.yml` installs what the bundles build. Its first dry run
//! built the aarch64 half of the macOS binary and failed on the x86_64 half:
//! the workflow listed both targets inside a `{ … }` mapping, where the comma
//! ends the entry, so only the first was installed.

use std::path::Path;
use xtask::macos::TRIPLES;

fn workflow() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/release.yml"),
    )
    .unwrap()
}

/// The targets the macOS job hands the toolchain, as the runner reads them.
fn macos_targets(workflow: &str) -> Result<Vec<String>, String> {
    let job = workflow
        .split("\n  macos:\n")
        .nth(1)
        .ok_or("release.yml has no macos job")?
        .split("\n  linux:\n")
        .next()
        .unwrap_or_default();
    let line = job
        .lines()
        .find(|l| l.contains("targets:"))
        .ok_or("the macos job installs no targets")?;
    if line.contains('{') {
        return Err(format!(
            "the targets are in a {{ … }} mapping, where the comma ends the entry: {}",
            line.trim()
        ));
    }
    let list = line.split_once("targets:").map_or("", |(_, list)| list);
    Ok(list
        .split(',')
        .map(|t| t.trim().trim_matches(['"', '\'']).to_string())
        .collect())
}

#[test]
fn the_macos_job_installs_every_target_the_bundle_builds() {
    let installed = macos_targets(&workflow()).unwrap();
    for triple in TRIPLES {
        assert!(
            installed.iter().any(|t| t == triple),
            "release.yml's macos job does not install {triple}: {installed:?}"
        );
    }
}

#[test]
fn a_target_list_in_a_flow_mapping_is_refused() {
    let first_dry_run = "\
jobs:
  macos:
    steps:
      - uses: dtolnay/rust-toolchain@1.97.0
        with: { targets: aarch64-apple-darwin,x86_64-apple-darwin }
  linux:
";
    assert!(macos_targets(first_dry_run).is_err());
}
