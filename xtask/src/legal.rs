//! The notices a bundle carries beside the binary: dat0's licence, the
//! third-party NOTICE with each licence's text, and the licences of the fonts
//! and icons compiled into the binary, which cargo-about cannot see. The
//! licences dat0's dependencies use ask for their text to travel with the
//! binary, and OFL 1.1 §2 asks it of the fonts.

use anyhow::{Context, Result};
use std::path::Path;

/// `(source in the repository, name in the bundle)`.
pub const FILES: [(&str, &str); 4] = [
    ("LICENSE", "LICENSE"),
    ("NOTICE.md", "NOTICE.md"),
    ("crates/dat0-ui/assets/fonts/LICENSE-geist", "LICENSE-geist"),
    (
        "crates/dat0-ui/assets/icons/LICENSE-lucide",
        "LICENSE-lucide",
    ),
];

/// Copy [`FILES`] from the repository at `root` into `dir`.
pub fn copy_into(root: &Path, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    for (source, name) in FILES {
        std::fs::copy(root.join(source), dir.join(name))
            .with_context(|| format!("copy {source} into {}", dir.display()))?;
    }
    Ok(())
}
