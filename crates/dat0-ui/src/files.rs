//! Native file dialogs and file drop.
//!
//! GPUI supplied `cx.prompt_for_paths` / `cx.prompt_for_new_path`, which
//! returned a `oneshot::Receiver` the caller awaited. `rfd::AsyncFileDialog`
//! is the same shape without the toolkit — an `async fn` returning the
//! selection — so the nine call sites port by substitution.
//!
//! Every dialog here is `async` and must be awaited from a `spawn`; none of
//! them block, and none of them touch the engine. What happens to a chosen
//! path is the caller's business, and for opening data files it is
//! [`dat0_core::file_drop::handle_drop`] — the same function the drop handler
//! calls, so "open" and "drop" cannot drift.

use std::path::{Path, PathBuf};

use dioxus::html::HasFileData as _;

/// Extensions dat0 can register as a table.
///
/// `sqlite` is deliberately absent: `handle_drop` rejects it with a banner
/// (attaching a SQLite database is a connection, not a file import), and a
/// picker that offers a file the app then refuses is a worse experience than
/// one that does not list it.
const DATA_EXTENSIONS: &[&str] = &[
    "csv", "tsv", "txt", "parquet", "pq", "json", "ndjson", "jsonl",
];

/// Pick one data file to open.
pub async fn pick_data_file() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .add_filter("data", DATA_EXTENSIONS)
        .pick_file()
        .await
        .map(|h| h.path().to_path_buf())
}

/// Pick any number of data files.
pub async fn pick_data_files() -> Vec<PathBuf> {
    rfd::AsyncFileDialog::new()
        .add_filter("data", DATA_EXTENSIONS)
        .pick_files()
        .await
        .map(|hs| hs.iter().map(|h| h.path().to_path_buf()).collect())
        .unwrap_or_default()
}

/// Pick a `.dat0` package.
pub async fn pick_package() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .add_filter("dat0 package", &["dat0"])
        .pick_file()
        .await
        .map(|h| h.path().to_path_buf())
}

/// Pick a folder — the workspace open/save-as target.
pub async fn pick_folder() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|h| h.path().to_path_buf())
}

/// Choose a path to write, seeded with a suggested file name.
///
/// The suggestion carries the extension, which is what makes the platform
/// panel default to the right file type; callers must pass a full name like
/// `workspace.dat0` or `export.csv`, not a stem.
pub async fn pick_save_path(suggested: &str) -> Option<PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new().set_file_name(suggested);
    if let Some(ext) = Path::new(suggested).extension().and_then(|e| e.to_str()) {
        dialog = dialog.add_filter(ext, &[ext]);
    }
    dialog.save_file().await.map(|h| h.path().to_path_buf())
}

/// The paths carried by a Dioxus file-drop event.
///
/// Returns real filesystem paths, which is why
/// `Config::with_disable_drag_drop_handler` stays at its default (`false`): the
/// HTML5 drop payload gives dat0 a `File`, not a path, and dat0 needs the path
/// to register a table without copying the file. The only in-page drag is the
/// grid's column reorder, which is unaffected on macOS and Linux — and Windows
/// is not a supported target, so the platform's mutual exclusion between the
/// two is moot.
pub fn dropped_paths(data: &dioxus::events::DragData) -> Vec<PathBuf> {
    data.files()
        .into_iter()
        .map(|f| f.path())
        // A webview drop can carry an entry with no filesystem path — a
        // dragged selection from another app, say. Registering one would fail
        // deep in the engine with a confusing message; dropping it here means
        // the user simply sees nothing happen for that item.
        .filter(|p| p.exists())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picker_does_not_offer_a_file_the_app_would_refuse() {
        // `handle_drop` banners `.sqlite` rather than importing it; listing it
        // here would be an invitation to that banner.
        assert!(!DATA_EXTENSIONS.contains(&"sqlite"));
        assert!(DATA_EXTENSIONS.contains(&"parquet"));
    }
}
