//! Replaying a `.dat0` package against fresh sources (PD-023, step 5.4d).
//!
//! File → Replay .dat0 Package was disabled. It now asks for a package, then
//! for the file to read in place of each of its sources, then where to write
//! the new package, and rebuilds the package's tables on the new data
//! (`cli::replay_with`). The GPUI build asked for one file and bound it to the
//! first source, so a package of several could not be replayed from the app.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use dioxus::prelude::*;

use dat0_core::error_ux::Banner;
use dat0_i18n::t;

use crate::state::Workspace;

/// File → Replay .dat0 Package…: pick a package, a file for each of its
/// sources, and where to write what replay makes.
pub fn pick(ws: Workspace) {
    if !crate::launch::has_desktop() {
        tracing::debug!("replay package: no window system, nothing to show");
        return;
    }
    spawn(async move {
        let Some(package) = crate::files::pick_package().await else {
            return;
        };
        let sources = match sources(&package).await {
            Ok(sources) if sources.is_empty() => {
                ws.push_banner(Banner::warning_with_body(
                    t("package.replay.none"),
                    package.display().to_string(),
                ));
                return;
            }
            Ok(sources) => sources,
            Err(e) => return report(ws, Err(e)),
        };
        let mut bound = HashMap::new();
        for source in sources {
            let Some(file) = crate::files::pick_replay_source(&source).await else {
                return;
            };
            bound.insert(source, file);
        }
        let stem = package
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "package".into());
        let Some(out) = crate::files::pick_save_path(&format!("{stem}-replayed.dat0")).await else {
            return;
        };
        report(ws, replay(package, bound, out).await);
    });
}

/// The names of `package`'s sources, each of which replay reads anew.
pub async fn sources(package: &Path) -> anyhow::Result<Vec<String>> {
    let path = package.to_path_buf();
    let parsed = crate::background::run(async move {
        dat0_format::Reader::open(&path).with_context(|| format!("open {}", path.display()))
    })
    .await?;
    Ok(parsed
        .sources
        .sources
        .into_iter()
        .map(|s| s.logical_name)
        .collect())
}

/// Replay `package` with each source read from the file `bound` maps its name
/// to, writing the new package to `out`. Returns where it wrote.
pub async fn replay(
    package: PathBuf,
    bound: HashMap<String, PathBuf>,
    out: PathBuf,
) -> anyhow::Result<PathBuf> {
    crate::background::run(
        async move { dat0_core::cli::replay_with(&package, &bound, Some(out)).await },
    )
    .await
}

fn report(ws: Workspace, replayed: anyhow::Result<PathBuf>) {
    match replayed {
        Ok(out) => ws.push_banner(Banner {
            body: out.display().to_string(),
            ..Banner::info(t("package.replay.done.title"))
        }),
        Err(e) => ws.push_banner(Banner::error(
            t("package.replay.failed.title"),
            format!("{e:#}"),
        )),
    }
}
