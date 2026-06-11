//! Pure heuristic: is a workspace path on a sync drive (so it needs the
//! cross-machine lock manifest)? Design D2 — over-detection is harmless, so we
//! only ever err toward "networked"; the only override that matters is force-on.

use std::path::Path;

use crate::settings::Workspace as WorkspaceSettings;

/// Known sync-provider path fragments (matched against the absolute path,
/// case-sensitive — macOS paths preserve case even on a case-insensitive FS).
/// `Library/Mobile Documents` = iCloud Drive; `.var/syncthing` = Syncthing.
const SYNC_FRAGMENTS: &[&str] = &[
    "Library/Mobile Documents",
    "Dropbox",
    "OneDrive",
    "Google Drive",
    ".var/syncthing",
];

/// True when `path` should use the cross-machine lock manifest.
pub fn is_networked(path: &Path, settings: &WorkspaceSettings) -> bool {
    if settings.treat_all_as_networked {
        return true;
    }
    if settings
        .treat_paths_as_networked
        .iter()
        .any(|p| path.starts_with(p))
    {
        return true;
    }
    let s = path.to_string_lossy();
    SYNC_FRAGMENTS.iter().any(|frag| s.contains(frag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn settings(all: bool, force_on: &[&str]) -> WorkspaceSettings {
        WorkspaceSettings {
            treat_paths_as_networked: force_on.iter().map(PathBuf::from).collect(),
            treat_all_as_networked: all,
        }
    }

    #[test]
    fn detects_known_sync_prefixes() {
        let s = settings(false, &[]);
        for p in [
            "/Users/x/Library/Mobile Documents/com~apple~CloudDocs/proj",
            "/Users/x/Dropbox/proj",
            "/Users/x/OneDrive/proj",
            "/Users/x/Google Drive/proj",
            "/home/x/.var/syncthing/proj",
        ] {
            assert!(is_networked(Path::new(p), &s), "should detect {p}");
        }
    }

    #[test]
    fn local_path_is_not_networked() {
        let s = settings(false, &[]);
        assert!(!is_networked(Path::new("/Users/x/Projects/proj"), &s));
        assert!(!is_networked(Path::new("/tmp/scratch/proj"), &s));
    }

    #[test]
    fn force_on_list_marks_networked() {
        let s = settings(false, &["/Volumes/share"]);
        assert!(is_networked(Path::new("/Volumes/share/proj"), &s));
        assert!(!is_networked(Path::new("/Volumes/other/proj"), &s));
    }

    #[test]
    fn global_toggle_marks_everything_networked() {
        let s = settings(true, &[]);
        assert!(is_networked(Path::new("/tmp/anything"), &s));
    }
}
