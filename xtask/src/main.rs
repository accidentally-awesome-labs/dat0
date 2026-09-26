//! dat0 build/release mechanics. Run via `cargo xtask <subcommand>`.
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use xtask::{icon, linux, macos, manifest, perf, release, sign};

#[derive(Parser)]
#[command(bin_name = "xtask", about = "dat0 build/release tasks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate the placeholder app icon (.iconset PNGs + .icns + Linux PNG).
    GenIcon {
        #[arg(long, default_value = "target/icon")]
        out: PathBuf,
    },
    /// Build universal macOS .app (both arches, lipo, Info.plist).
    BundleMacos {
        #[arg(long)]
        version: String,
        #[arg(long, default_value = "")]
        git_sha: String,
    },
    /// Sign + notarize + staple the .app into a signed .dmg.
    SignMacos {
        #[arg(long)]
        identity: String,
    },
    /// Build the .dmg without a Developer ID: the .app signed ad hoc and not
    /// notarized. A dry run's.
    DmgMacos,
    /// Build the Linux .AppImage (+ .desktop, MIME, GPG sign).
    BundleLinux {
        #[arg(long)]
        version: String,
        /// Leave the GPG signature out: a dry run without the key.
        #[arg(long)]
        unsigned: bool,
    },
    /// Verify signed artifacts (Gatekeeper / GPG).
    Verify {
        #[arg(long)]
        macos: bool,
        #[arg(long)]
        linux: bool,
    },
    /// Generate latest.json manifest for auto-update.
    GenManifest {
        #[arg(long)]
        version: String,
        #[arg(long)]
        macos_sha: String,
        #[arg(long)]
        macos_size: u64,
        #[arg(long)]
        linux_sha: String,
        #[arg(long)]
        linux_size: u64,
    },
    /// Check what a release must carry: the production update key, a real
    /// crash-report DSN, the sample's hash, the signing secrets, and a tag
    /// naming the workspace version. With `--tag`, a finding fails the run;
    /// without, a dry run, each is a warning.
    ReleaseCheck {
        #[arg(long)]
        tag: Option<String>,
    },
    /// MX2: run the perf scenarios, optionally gating on the committed budgets.
    Perf {
        /// Scenario to run; repeatable. Omitted runs all six.
        #[arg(long = "scenario")]
        scenario: Vec<String>,
        /// Compare each measurement against the budget and this host's
        /// recorded baseline; exit 1 on a breach.
        #[arg(long)]
        check: bool,
        /// Record this run as this host's baseline.
        #[arg(long)]
        update_baseline: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::GenIcon { out } => icon::generate(&out).map(|_| ()),
        Cmd::BundleMacos { version, git_sha } => macos::bundle(&version, &git_sha).map(|_| ()),
        Cmd::SignMacos { identity } => sign::sign_and_notarize(&identity).map(|_| ()),
        Cmd::DmgMacos => sign::unsigned_dmg().map(|_| ()),
        Cmd::BundleLinux { version, unsigned } => linux::bundle(&version, !unsigned).map(|_| ()),
        Cmd::Verify { macos, linux } => sign::verify(macos, linux),
        Cmd::GenManifest {
            version,
            macos_sha,
            macos_size,
            linux_sha,
            linux_size,
        } => {
            let json =
                manifest::build_manifest(&version, &macos_sha, macos_size, &linux_sha, linux_size);
            std::fs::write("target/latest.json", json)?;
            Ok(())
        }
        Cmd::ReleaseCheck { tag } => {
            let findings =
                release::check(Path::new("."), tag.as_deref(), |k| std::env::var(k).ok())?;
            // GitHub's annotation syntax, so each finding is listed on the run.
            let level = if tag.is_some() { "error" } else { "warning" };
            for f in &findings {
                println!("::{level}::{} — {}", f.what, f.fix);
            }
            match (findings.len(), &tag) {
                (0, _) => println!("release-check: nothing is missing"),
                (n, Some(tag)) => {
                    println!("release-check: {tag} is missing {n} thing(s); nothing was built");
                    std::process::exit(1);
                }
                (n, None) => println!("release-check: a tag would stop on {n} finding(s)"),
            }
            Ok(())
        }
        // The only subcommand with a meaningful non-zero exit that is not an
        // error: a budget breach is a RESULT, so it must not surface as an
        // anyhow chain that CI logs as a crash.
        Cmd::Perf {
            scenario,
            check,
            update_baseline,
        } => {
            let code = perf::run(perf::Options {
                scenarios: scenario,
                check,
                update_baseline,
            })?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
    }
}
