//! Step 7: `release.yml` installs what the bundles build, and signs the update
//! manifest in a way that can succeed. Its first dry run built the aarch64 half
//! of the macOS binary and failed on the x86_64 half: the workflow listed both
//! targets inside a `{ … }` mapping, where the comma ends the entry, so only
//! the first was installed. No dry run reaches the manifest signing, which
//! runs on a tag alone; run by hand, it failed as a runner would run it.

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

/// The publish job's steps, each from its `- ` line.
fn publish_steps(workflow: &str) -> Vec<&str> {
    let job = workflow
        .split("\n  publish:\n")
        .nth(1)
        .expect("release.yml has no publish job");
    job.split("\n      - ").skip(1).collect()
}

/// rsign asks for the key's password on the terminal unless `-W` says there
/// is none, and a runner has no terminal: without it the signing fails,
/// leaving an empty signature (rsign2 0.6.7, "No such device or address").
/// The signature is then checked as the app will check it, before anything
/// is uploaded, and the key reaches only the step that runs rsign.
#[test]
fn the_manifest_is_signed_without_a_terminal_and_checked_before_publishing() {
    let workflow = workflow();
    let steps = publish_steps(&workflow);
    let find = |needle: &str| {
        steps
            .iter()
            .position(|step| step.contains(needle))
            .unwrap_or_else(|| panic!("the publish job does not run `{needle}`"))
    };
    let (sign, verify, publish) = (
        find("rsign sign"),
        find("cargo xtask verify-manifest"),
        find("gh release create"),
    );
    assert!(
        sign < verify && verify < publish,
        "the manifest is signed, then checked, then published"
    );

    let line = steps[sign]
        .lines()
        .find(|l| l.trim_start().starts_with("rsign sign"))
        .unwrap();
    assert!(
        line.split_whitespace().any(|word| word == "-W"),
        "rsign sign without -W asks for a password on a terminal: {}",
        line.trim()
    );
    let install = steps[find("cargo install rsign2")]
        .lines()
        .find(|l| l.contains("cargo install rsign2"))
        .unwrap();
    assert!(
        install.contains("--version"),
        "rsign2 is installed unpinned: {}",
        install.trim()
    );

    let holding: Vec<&&str> = steps
        .iter()
        .filter(|step| step.contains("secrets.MINISIGN_SECRET_KEY"))
        .collect();
    assert_eq!(holding.len(), 1, "one step holds the manifest key");
    assert!(
        !holding[0].contains("cargo "),
        "the step holding the manifest key runs cargo, whose build scripts would see it"
    );
}
