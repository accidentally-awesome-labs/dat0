//! GPUI file-drop handler: detect format → register_file → tab append.
//!
//! Unsupported extension (and `.sqlite`) → Banner + drop. Engine error
//! → Banner with err message + drop. Success → Tab + active.

use dat0_engine::{FileFormat, QueryEngine, RegisterOpts};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error_ux::banner;
use crate::session::{Session, Tab};

/// Handle a batch of dropped paths. Returns one [`DropOutcome`] per path, in order.
pub async fn handle_drop(paths: Vec<PathBuf>, session: Arc<Mutex<Session>>) -> Vec<DropOutcome> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        out.push(handle_one(path, &session).await);
    }
    out
}

/// Outcome for a single dropped path.
#[derive(Debug)]
pub enum DropOutcome {
    /// File was registered and a tab was appended.
    Registered {
        table_name: String,
        source_path: PathBuf,
    },
    /// Extension was not recognised (or is explicitly unsupported, e.g. `.sqlite`).
    Unsupported {
        path: PathBuf,
        extension: Option<String>,
    },
    /// The engine returned an error during `register_file`.
    EngineError { path: PathBuf, error: String },
}

async fn handle_one(path: PathBuf, session: &Mutex<Session>) -> DropOutcome {
    let fmt = match FileFormat::from_extension(&path) {
        Some(f) => f,
        None => {
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string());
            let label = ext.clone().unwrap_or_else(|| "(no extension)".to_string());
            banner::push_warning(format!("Unsupported file type: {label}"));
            return DropOutcome::Unsupported {
                path,
                extension: ext,
            };
        }
    };

    // Clone the engine Arc and immediately release the session lock before
    // the async `register_file` call. Holding a parking_lot mutex lock across
    // an `.await` point is undefined behaviour because the lock guard is not
    // Send-aware with respect to async runtimes.
    let engine = session.lock().engine.clone();

    let opts = RegisterOpts {
        format: Some(fmt),
        ..Default::default()
    };

    match engine.register_file(&path, opts).await {
        Ok(info) => {
            let mut s = session.lock();
            s.add_tab(Tab {
                table_name: info.name.clone(),
                source_path: Some(path.clone()),
            })
            .expect("session::add_tab: persist tab state");
            DropOutcome::Registered {
                table_name: info.name,
                source_path: path,
            }
        }
        Err(e) => {
            let msg = format!("{}: {e}", path.display());
            banner::push_warning(msg.clone());
            DropOutcome::EngineError { path, error: msg }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error_ux::banner::drain_pending;
    use serial_test::serial;
    use tempfile::TempDir;

    const BUDGET: u64 = 128 * 1024 * 1024;

    /// All three tests touch the process-global banner queue; `#[serial]`
    /// prevents concurrent tests from leaking banners into each other's drain.
    #[tokio::test]
    #[serial]
    async fn unsupported_ext_emits_banner_no_tab() {
        let _ = drain_pending(); // clear any banners from prior tests
        let tmp = TempDir::new().unwrap();
        let sess = Session::new(tmp.path(), BUDGET).await.unwrap();
        let arc = Arc::new(Mutex::new(sess));

        let weird = tmp.path().join("file.bin");
        std::fs::write(&weird, b"\x00\x01\x02").unwrap();

        let outcomes = handle_drop(vec![weird.clone()], Arc::clone(&arc)).await;
        assert!(
            matches!(
                outcomes[0],
                DropOutcome::Unsupported { ref path, .. } if path == &weird
            ),
            "expected Unsupported outcome"
        );
        assert!(arc.lock().tabs().is_empty(), "no tab should be added");

        let banners = drain_pending();
        assert_eq!(banners.len(), 1);
        assert!(
            banners[0].title.contains("Unsupported"),
            "banner title should mention 'Unsupported'"
        );
    }

    #[tokio::test]
    #[serial]
    async fn sqlite_ext_emits_banner_no_tab() {
        let _ = drain_pending();
        let tmp = TempDir::new().unwrap();
        let sess = Session::new(tmp.path(), BUDGET).await.unwrap();
        let arc = Arc::new(Mutex::new(sess));

        let sqlite = tmp.path().join("db.sqlite");
        std::fs::write(&sqlite, b"sqlite-stub").unwrap();

        let outcomes = handle_drop(vec![sqlite.clone()], Arc::clone(&arc)).await;
        assert!(
            matches!(outcomes[0], DropOutcome::Unsupported { .. }),
            "sqlite should produce Unsupported outcome"
        );
        assert!(arc.lock().tabs().is_empty(), "no tab should be added");
    }

    #[tokio::test]
    #[serial]
    async fn csv_drop_appends_tab() {
        let _ = drain_pending();
        let tmp = TempDir::new().unwrap();
        let sess = Session::new(tmp.path(), BUDGET).await.unwrap();
        let arc = Arc::new(Mutex::new(sess));

        let csv = tmp.path().join("a.csv");
        std::fs::write(&csv, "a,b\n1,x\n2,y\n").unwrap();

        let outcomes = handle_drop(vec![csv.clone()], Arc::clone(&arc)).await;
        assert!(
            matches!(outcomes[0], DropOutcome::Registered { .. }),
            "csv should produce Registered outcome"
        );
        let s = arc.lock();
        assert_eq!(s.tabs().len(), 1, "one tab should be added");
        assert!(s.active_tab().is_some(), "active_tab should be set");
    }
}
