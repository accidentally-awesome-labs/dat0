//! Step 7: the release gate stops a tag that would ship the test update key,
//! the stub crash-report DSN, the sample's placeholder hash, a version other
//! than the tag's, or no signing secrets, and says so on a dry run.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use xtask::release::{self, REQUIRED_SECRETS};

const PRODUCTION_KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
const TEST_KEY: &str = "RWR/aKYzRk3oeZJDzLCZ/nooGJs2wLOVhTKMMaqJvOEWyFKpf53Ir9RW";
const STUB: &str = "https://stub@glitchtip.invalid/1";
const DSN: &str = "https://0123abcd@crash.example.org/7";
const HASH: &str = "4f3c2b1a0e9d8c7b6a5f4e3d2c1b0a9f8e7d6c5b4a3f2e1d0c9b8a7f6e5d4c3b";

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A workspace laid out as dat0's is, holding everything a release needs.
fn complete_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/dat0-ui\", \"crates/dat0-core\", \"xtask\"]\n\n\
         [workspace.package]\nversion = \"0.2.0\"\n",
    );
    for member in ["crates/dat0-ui", "crates/dat0-core"] {
        let name = member.trim_start_matches("crates/");
        write(
            root,
            &format!("{member}/Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion.workspace = true\n"),
        );
    }
    // xtask is not shipped, so its own version is its business.
    write(
        root,
        "xtask/Cargo.toml",
        "[package]\nname = \"xtask\"\nversion = \"0.1.0\"\n",
    );
    write(
        root,
        ".cargo/config.toml",
        &format!("[env]\nDAT0_GLITCHTIP_DSN_PUBLIC = \"{STUB}\"\n"),
    );
    write(root, release::UPDATE_KEY, &format!("{PRODUCTION_KEY}\n"));
    write(
        root,
        "crates/dat0-core/tests/fixtures/update/test-minisign.pub",
        &format!("untrusted comment: minisign public key 79E84D4633A6687F\n{TEST_KEY}\n"),
    );
    write(
        root,
        release::SAMPLE_DATA,
        &format!("pub const NYC_TAXI_SHA256: &str = \"{HASH}\";\n"),
    );
    dir
}

/// The environment a release run hands the gate: the DSN and every secret.
fn complete_env() -> HashMap<String, String> {
    let mut env: HashMap<String, String> = REQUIRED_SECRETS
        .iter()
        .map(|name| (format!("HAVE_{name}"), "true".to_string()))
        .collect();
    env.insert(release::DSN_VAR.into(), DSN.into());
    env
}

fn findings(root: &Path, tag: Option<&str>, env: &HashMap<String, String>) -> Vec<String> {
    release::check(root, tag, |k| env.get(k).cloned())
        .unwrap()
        .into_iter()
        .map(|f| f.what)
        .collect()
}

#[test]
fn a_release_that_carries_everything_passes() {
    let tree = complete_tree();
    assert_eq!(
        findings(tree.path(), Some("v0.2.0"), &complete_env()),
        Vec::<String>::new()
    );
}

#[test]
fn a_tag_naming_another_version_is_stopped() {
    let tree = complete_tree();
    let found = findings(tree.path(), Some("v0.3.0"), &complete_env());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("v0.3.0") && found[0].contains("0.2.0"),
        "the finding names the tag and the workspace version: {found:?}"
    );
    // A dry run names no tag, so there is none to disagree with.
    assert!(findings(tree.path(), None, &complete_env()).is_empty());
}

#[test]
fn a_shipped_crate_with_its_own_version_is_stopped() {
    let tree = complete_tree();
    write(
        tree.path(),
        "crates/dat0-ui/Cargo.toml",
        "[package]\nname = \"dat0-ui\"\nversion = \"0.1.0\"\n",
    );
    let found = findings(tree.path(), Some("v0.2.0"), &complete_env());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("crates/dat0-ui"), "{found:?}");
}

#[test]
fn the_test_update_key_is_stopped_wherever_the_fixtures_keep_it() {
    let tree = complete_tree();
    write(tree.path(), release::UPDATE_KEY, &format!("{TEST_KEY}\n"));
    let found = findings(tree.path(), Some("v0.2.0"), &complete_env());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("test-minisign.pub"), "{found:?}");

    // A second test key, deeper in the fixtures, is found as well.
    write(
        tree.path(),
        release::UPDATE_KEY,
        &format!("{PRODUCTION_KEY}\n"),
    );
    write(
        tree.path(),
        "crates/dat0-core/tests/fixtures/update/roundtrip/throwaway.pub",
        &format!("untrusted comment: throwaway\n{PRODUCTION_KEY}\n"),
    );
    let found = findings(tree.path(), Some("v0.2.0"), &complete_env());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("throwaway.pub"), "{found:?}");

    // An empty key file, or one holding the whole .pub file, is not a key.
    for text in ["", "untrusted comment: minisign public key\nRWQ...\n"] {
        write(tree.path(), release::UPDATE_KEY, text);
        let found = findings(tree.path(), Some("v0.2.0"), &complete_env());
        assert_eq!(found.len(), 1, "{text:?}: {found:?}");
        assert!(found[0].contains("one line"), "{found:?}");
    }
}

#[test]
fn a_missing_or_stub_dsn_is_stopped() {
    let tree = complete_tree();
    for (dsn, expect) in [
        (None, "no crash-report DSN"),
        (Some(""), "no crash-report DSN"),
        (Some(STUB), "is the stub"),
        (Some("glitchtip.example.org"), "not an https://"),
    ] {
        let mut env = complete_env();
        match dsn {
            Some(dsn) => env.insert(release::DSN_VAR.into(), dsn.into()),
            None => env.remove(release::DSN_VAR),
        };
        let found = findings(tree.path(), Some("v0.2.0"), &env);
        assert_eq!(found.len(), 1, "{dsn:?}: {found:?}");
        assert!(found[0].contains(expect), "{dsn:?}: {found:?}");
    }
}

#[test]
fn the_samples_placeholder_hash_is_stopped() {
    let tree = complete_tree();
    write(
        tree.path(),
        release::SAMPLE_DATA,
        "pub const NYC_TAXI_SHA256: &str = \"FILL_AT_T8\";\n",
    );
    let found = findings(tree.path(), Some("v0.2.0"), &complete_env());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("FILL_AT_T8"), "{found:?}");
}

#[test]
fn missing_secrets_are_named_in_one_finding() {
    let tree = complete_tree();
    let mut env = complete_env();
    env.insert("HAVE_GPG_PRIVATE_KEY".into(), "false".into());
    env.remove("HAVE_AC_KEY_ID");
    let found = findings(tree.path(), Some("v0.2.0"), &env);
    assert_eq!(found, ["secrets not set: AC_KEY_ID, GPG_PRIVATE_KEY"]);
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Every crate the release ships reports the workspace's version, so a tag
/// only has to agree with one number.
#[test]
fn every_shipped_crate_reports_the_workspace_version() {
    let found = findings(&repo_root(), None, &complete_env());
    let own: Vec<&String> = found
        .iter()
        .filter(|f| f.contains("its own version"))
        .collect();
    assert!(own.is_empty(), "{own:?}");
}

/// The gate can only check the secrets the workflow tells it about. Every
/// secret `release.yml` reads is either one the gate requires, and handed to
/// it as `HAVE_<NAME>`, or one it has a reason to leave out.
#[test]
fn the_gate_is_told_about_every_secret_the_release_reads() {
    // GPG_PASSPHRASE: optional by design. GLITCHTIP_DSN_PUBLIC: checked as
    // the DSN itself, which the gate step is handed.
    const NOT_REQUIRED: [&str; 2] = ["GPG_PASSPHRASE", "GLITCHTIP_DSN_PUBLIC"];
    let workflow =
        std::fs::read_to_string(repo_root().join(".github/workflows/release.yml")).unwrap();
    let mut read: Vec<&str> = workflow
        .split("secrets.")
        .skip(1)
        .map(|rest| {
            rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap()
        })
        .collect();
    read.sort_unstable();
    read.dedup();
    for name in &read {
        assert!(
            REQUIRED_SECRETS.contains(name) || NOT_REQUIRED.contains(name),
            "release.yml reads secrets.{name}, which the gate does not check"
        );
    }
    for name in REQUIRED_SECRETS {
        let line = format!("HAVE_{name}: ${{{{ secrets.{name} != '' }}}}");
        assert!(
            workflow.contains(&line),
            "release.yml's gate is not handed `{line}`"
        );
    }
    assert!(
        workflow.contains("DAT0_GLITCHTIP_DSN_PUBLIC: ${{ secrets.GLITCHTIP_DSN_PUBLIC }}"),
        "release.yml's gate is not handed the DSN"
    );
}
