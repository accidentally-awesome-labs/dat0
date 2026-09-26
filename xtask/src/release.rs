//! The release gate: what a tag must carry before anything is built for it.
//!
//! A tag ships to users, and none of what follows fails a build on its own. A
//! binary carrying the test update key builds, signs and installs, and then
//! refuses every real update manifest. One built without the crash-report DSN
//! sends the reports users opt in to nowhere. A tag that names another version
//! than the workspace ships a binary that reports the old one, so its updater
//! offers the release it already is. Each fix is a human step
//! (`docs/release-prerequisites.md`), so `release.yml` runs this check before
//! it builds anything. On a tag every finding is an error; on a dry run each is
//! a warning, and the run says what a tag would still stop on.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// One thing a release is missing, and where its fix is written down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub what: String,
    pub fix: &'static str,
}

/// The secrets `release.yml` signs and publishes with. The workflow cannot
/// hand a secret to a check without exposing it, so the gate step is given
/// `HAVE_<NAME>`, `true` or `false`, for each. `GPG_PASSPHRASE` is left out:
/// the recommended CI subkey has none (`docs/security-runbook.md`). The
/// crash-report DSN is checked through its value instead, as
/// `DAT0_GLITCHTIP_DSN_PUBLIC`.
pub const REQUIRED_SECRETS: [&str; 9] = [
    "APPLE_DEV_ID_CERT_P12",
    "CERT_PASSWORD",
    "KEYCHAIN_PASSWORD",
    "AC_API_KEY_P8",
    "AC_KEY_ID",
    "AC_ISSUER_ID",
    "DAT0_SIGN_IDENTITY",
    "GPG_PRIVATE_KEY",
    "MINISIGN_SECRET_KEY",
];

/// The key the updater trusts; `update/manifest.rs` compiles it in.
pub const UPDATE_KEY: &str = "crates/dat0-core/assets/minisign-public-key.txt";
/// Where the test keys live: none of them may be the key a release trusts.
pub const TEST_KEYS: &str = "crates/dat0-core/tests/fixtures";
/// The source that names the NYC taxi sample's hash.
pub const SAMPLE_DATA: &str = "crates/dat0-core/src/sample_data.rs";
/// The environment variable the crash-report DSN is compiled from.
pub const DSN_VAR: &str = "DAT0_GLITCHTIP_DSN_PUBLIC";

/// Check the tree at `root` for release `tag` (a dry run passes `None`), with
/// the environment read through `env`.
pub fn check(
    root: &Path,
    tag: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Vec<Finding>> {
    let mut found = Vec::new();
    version(root, tag, &mut found)?;
    update_key(root, &mut found)?;
    dsn(root, &env, &mut found)?;
    sample(root, &mut found)?;
    let missing: Vec<&str> = REQUIRED_SECRETS
        .into_iter()
        .filter(|name| env(&format!("HAVE_{name}")).as_deref() != Some("true"))
        .collect();
    if !missing.is_empty() {
        found.push(Finding {
            what: format!("secrets not set: {}", missing.join(", ")),
            fix: "docs/release-prerequisites.md says where each comes from",
        });
    }
    Ok(found)
}

fn read_toml(path: &Path) -> Result<toml::Table> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    text.parse()
        .with_context(|| format!("parse {}", path.display()))
}

/// The version every shipped crate reports is the workspace's, and a tag names
/// that version.
fn version(root: &Path, tag: Option<&str>, found: &mut Vec<Finding>) -> Result<()> {
    let manifest = read_toml(&root.join("Cargo.toml"))?;
    let workspace = manifest
        .get("workspace")
        .and_then(|w| w.as_table())
        .context("Cargo.toml has no [workspace]")?;
    let version = workspace
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .context("Cargo.toml has no [workspace.package] version")?;
    let members = workspace
        .get("members")
        .and_then(|m| m.as_array())
        .context("Cargo.toml has no workspace members")?;
    for member in members.iter().filter_map(|m| m.as_str()) {
        // xtask builds the release; it is not in it.
        if member == "xtask" {
            continue;
        }
        let crate_toml = read_toml(&root.join(member).join("Cargo.toml"))?;
        let own = crate_toml
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str());
        if let Some(own) = own {
            found.push(Finding {
                what: format!(
                    "{member} sets its own version {own}, so it reports {own} whatever \
                     the release is"
                ),
                fix: "give it `version.workspace = true`",
            });
        }
    }
    if let Some(tag) = tag
        && tag.strip_prefix('v') != Some(version)
    {
        found.push(Finding {
            what: format!(
                "tag {tag} does not name the workspace version {version}: the binary \
                 would report {version}, and its updater would offer this release to itself"
            ),
            fix: "set [workspace.package] version in Cargo.toml to the tag's, commit, and \
                  tag that commit (docs/release-runbook.md)",
        });
    }
    Ok(())
}

/// The second line of a minisign `.pub` file is its key; the first is a comment.
fn key_line(pub_file: &str) -> Option<&str> {
    pub_file.lines().nth(1).map(str::trim)
}

fn pub_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            pub_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "pub") {
            out.push(path);
        }
    }
    Ok(())
}

/// The updater trusts one key, and it is none of the test keys.
fn update_key(root: &Path, found: &mut Vec<Finding>) -> Result<()> {
    const FIX: &str = "generate the production key (docs/release-prerequisites.md §1)";
    let text = std::fs::read_to_string(root.join(UPDATE_KEY))
        .with_context(|| format!("read {UPDATE_KEY}"))?;
    let key = text.trim();
    if key.is_empty() || key.lines().count() != 1 {
        found.push(Finding {
            what: format!("{UPDATE_KEY} must hold one line, the public key"),
            fix: FIX,
        });
        return Ok(());
    }
    let mut tests = Vec::new();
    pub_files(&root.join(TEST_KEYS), &mut tests)?;
    tests.sort();
    for test in tests {
        let text = std::fs::read_to_string(&test)?;
        if key_line(&text) == Some(key) {
            let name = test
                .strip_prefix(root)
                .unwrap_or(&test)
                .display()
                .to_string();
            found.push(Finding {
                what: format!(
                    "the updater trusts the test key in {name}, so it would refuse every \
                     real release manifest"
                ),
                fix: FIX,
            });
        }
    }
    Ok(())
}

/// The crash-report DSN is set, and is not the development stub.
fn dsn(root: &Path, env: &impl Fn(&str) -> Option<String>, found: &mut Vec<Finding>) -> Result<()> {
    const FIX: &str = "set the GLITCHTIP_DSN_PUBLIC secret (docs/release-prerequisites.md §5)";
    let config = read_toml(&root.join(".cargo/config.toml"))?;
    let stub = config
        .get("env")
        .and_then(|e| e.get(DSN_VAR))
        .and_then(|v| {
            v.as_str()
                .or_else(|| v.get("value").and_then(|v| v.as_str()))
        })
        .context(".cargo/config.toml names no stub DSN")?;
    let what = match env(DSN_VAR).filter(|d| !d.trim().is_empty()) {
        None => format!(
            "no crash-report DSN: the build would carry the stub {stub}, and the reports \
             users opt in to would go nowhere"
        ),
        Some(dsn) if dsn.trim() == stub => format!("the crash-report DSN is the stub {stub}"),
        Some(dsn) if !(dsn.starts_with("https://") && dsn.contains('@')) => {
            "the crash-report DSN is not an https://key@host/project URL".to_string()
        }
        Some(_) => return Ok(()),
    };
    found.push(Finding { what, fix: FIX });
    Ok(())
}

/// The NYC taxi sample the hero offers is checked against a real hash.
fn sample(root: &Path, found: &mut Vec<Finding>) -> Result<()> {
    let text = std::fs::read_to_string(root.join(SAMPLE_DATA))
        .with_context(|| format!("read {SAMPLE_DATA}"))?;
    let hash = text
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("pub const NYC_TAXI_SHA256: &str = \"")
        })
        .and_then(|rest| rest.split('"').next())
        .with_context(|| format!("{SAMPLE_DATA} names no NYC_TAXI_SHA256"))?;
    if !(hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())) {
        found.push(Finding {
            what: format!(
                "the NYC taxi sample's hash is {hash:?}, not a SHA-256, so the sample \
                 the hero offers cannot be downloaded and checked"
            ),
            fix: "publish the asset and record its hash (docs/release-prerequisites.md §4)",
        });
    }
    Ok(())
}
