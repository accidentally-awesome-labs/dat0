//! dat0 build/release mechanics. Run via `cargo xtask <subcommand>`.
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use xtask::{icon, linux, macos, manifest, perf, sign};

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
    /// Build the Linux .AppImage (+ .desktop, MIME, GPG sign).
    BundleLinux {
        #[arg(long)]
        version: String,
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
        Cmd::BundleLinux { version } => linux::bundle(&version).map(|_| ()),
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
