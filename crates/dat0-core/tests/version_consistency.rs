//! RL2: on a tag build, the crate version must equal the tag.
//!
//! Nothing else in the tree enforces this. The root workspace version
//! (`Cargo.toml`, `[workspace.package] version`) is baked into the binary via
//! `env!("CARGO_PKG_VERSION")` — it is what `dat0 --version` prints, what the
//! About box shows (`src/about/build_info.rs:11`), and what
//! `update::newer_than` compares an incoming manifest against. `release.yml`
//! derives its own `VERSION` from `github.ref_name` instead
//! (`.github/workflows/release.yml`, `VERSION:` env on the bundle steps), so
//! tagging `v1.2.0` without bumping `Cargo.toml` produces a release whose
//! artifacts, manifest URLs and `latest.json` all say `1.2.0` while the binary
//! inside reports `0.1.0` — and then refuses to see itself as out of date.
//!
//! Off a tag (local runs, PR CI) this passes trivially: there is no tag to
//! compare against, and failing would make every developer's `cargo test` red.

#[test]
fn crate_version_matches_the_release_tag() {
    // GitHub Actions sets both of these on every run; `GITHUB_REF_TYPE` is
    // "tag" only for tag pushes, which is exactly when `release.yml`'s
    // `publish` job runs (it is gated on `github.ref_type == 'tag'`).
    let Ok(ref_type) = std::env::var("GITHUB_REF_TYPE") else {
        return;
    };
    if ref_type != "tag" {
        return;
    }
    let Ok(ref_name) = std::env::var("GITHUB_REF_NAME") else {
        return;
    };

    let tag_version = ref_name.trim_start_matches('v');
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        tag_version,
        "\n\
         Tag {ref_name} does not match the workspace version {}.\n\
         Bump `[workspace.package] version` in the root Cargo.toml to {tag_version},\n\
         commit, delete and re-push the tag. See docs/release-runbook.md step 1.\n",
        env!("CARGO_PKG_VERSION"),
    );
}
