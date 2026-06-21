//! dat0 build/release mechanics. Run via `cargo xtask <subcommand>`.
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use xtask::{icon, linux, macos, sign};

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::GenIcon { out } => icon::generate(&out).map(|_| ()),
        Cmd::BundleMacos { version, git_sha } => macos::bundle(&version, &git_sha).map(|_| ()),
        Cmd::SignMacos { identity } => sign::sign_and_notarize(&identity).map(|_| ()),
        Cmd::BundleLinux { version } => linux::bundle(&version).map(|_| ()),
        Cmd::Verify { macos, linux } => sign::verify(macos, linux),
    }
}
