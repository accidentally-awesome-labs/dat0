//! Single-instance enforcement: PID file + advisory flock + UDS server.
//!
//! Second `dat0` launch detects the lock, connects to the UDS, sends
//! `{open_window, paths?}`, and exits. The running instance spawns a
//! new Window on the main thread.
//!
//! State files:
//!   $STATE/dat0.pid   — text PID + flock guard
//!   $STATE/dat0.sock  — Unix domain socket (JSON-lines protocol)

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenWindowMessage {
    pub paths: Vec<PathBuf>,
}

pub struct AppLock {
    /// RAII guard for the advisory flock. The file handle is kept alive for
    /// the lifetime of the `AppLock`; closing it (in `Drop`) releases the
    /// flock automatically.
    // Held for its flock RAII lifetime; the OS releases the advisory lock when
    // this File handle is closed on Drop.
    #[expect(dead_code, reason = "held for RAII flock lifetime, not read directly")]
    pid_file: File,
    pid_path: PathBuf,
    sock_path: PathBuf,
}

impl AppLock {
    /// Try to acquire the singleton lock. Returns `Ok(Some(AppLock))` if
    /// this process is now the singleton; `Ok(None)` if another process
    /// already holds it (caller should forward via UDS and exit).
    pub fn try_acquire(state_dir: &Path) -> Result<Option<Self>> {
        use fs4::fs_std::FileExt;
        std::fs::create_dir_all(state_dir)
            .with_context(|| format!("create state dir {}", state_dir.display()))?;
        let pid_path = state_dir.join("dat0.pid");
        let sock_path = state_dir.join("dat0.sock");

        let mut pid_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&pid_path)
            .with_context(|| format!("open pid file {}", pid_path.display()))?;

        // Non-blocking flock. Held flock = another live instance.
        // Stale PID file with no holder = flock succeeds.
        match pid_file.try_lock_exclusive() {
            Ok(()) => {
                // We're the singleton. Rewrite PID + return.
                pid_file.set_len(0).context("truncate pid file")?;
                write!(pid_file, "{}", std::process::id()).context("write pid")?;
                pid_file.sync_all().context("fsync pid file")?;
                Ok(Some(Self {
                    pid_file,
                    pid_path,
                    sock_path,
                }))
            }
            Err(_) => Ok(None), // contention
        }
    }

    /// Forward an `OpenWindowMessage` to the running instance over UDS.
    /// Used by the second-launch path before `exit(0)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the UDS connection cannot be established or the
    /// message cannot be written. The caller may distinguish two cases by
    /// downcasting to [`std::io::Error`] and inspecting `kind()`:
    ///
    /// - [`std::io::ErrorKind::NotFound`] — the running instance has not yet
    ///   bound `dat0.sock`. Caller treats as a race; brief retry may succeed.
    /// - [`std::io::ErrorKind::ConnectionRefused`] — the socket exists but no
    ///   listener is accepting. Caller treats as an unresponsive running
    ///   instance and surfaces the spec §6 "already running but unresponsive"
    ///   path (stderr + exit 1).
    pub fn forward_open_window(state_dir: &Path, msg: OpenWindowMessage) -> Result<()> {
        use interprocess::local_socket::{GenericFilePath, Stream, prelude::*};
        let sock_path = state_dir.join("dat0.sock");
        let name = sock_path
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .context("build UDS name")?;
        let mut stream = Stream::connect(name).context("connect to running dat0 instance")?;
        let line = serde_json::to_string(&msg).context("serialize message")?;
        std::io::Write::write_all(&mut stream, line.as_bytes()).context("write to UDS")?;
        std::io::Write::write_all(&mut stream, b"\n").context("write newline to UDS")?;
        Ok(())
    }

    /// Block-and-serve UDS messages. Each line is one `OpenWindowMessage`.
    /// Calls `handler` for every received message. Runs on a dedicated
    /// tokio task (caller spawns it).
    ///
    /// # Borrow contract
    ///
    /// The `&self` borrow ties the spawned task's lifetime to the original
    /// `AppLock` binding. The caller (T12) must own an `AppLock` value
    /// outside of any `Arc`, move it into the spawned task via
    /// `tokio::spawn(async move { lock.serve(...).await })`, and keep no
    /// other handle to the lock. If shared ownership is needed later,
    /// change this signature to `serve(self, ...)` and re-evaluate the
    /// `Drop` order (file cleanup runs when the moved `AppLock` is dropped
    /// inside the task, which is fine).
    pub async fn serve(
        &self,
        handler: impl Fn(OpenWindowMessage) + Send + Sync + 'static,
    ) -> Result<()> {
        use interprocess::local_socket::tokio::prelude::*;
        use interprocess::local_socket::{GenericFilePath, ListenerOptions, ToFsName};
        use std::sync::Arc;
        use tokio::io::{AsyncBufReadExt, BufReader};

        // Stale socket file from a previous (crashed) instance.
        let _ = std::fs::remove_file(&self.sock_path);
        let name = self
            .sock_path
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .context("build UDS name")?;
        let listener = ListenerOptions::new()
            .name(name)
            .create_tokio()
            .context("bind UDS")?;
        let handler = Arc::new(handler);

        loop {
            let stream = match listener.accept().await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "UDS accept failed");
                    continue;
                }
            };
            let h = Arc::clone(&handler);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    match serde_json::from_str::<OpenWindowMessage>(&line) {
                        Ok(msg) => h(msg),
                        Err(e) => tracing::warn!(error = %e, "UDS bad message"),
                    }
                }
            });
        }
    }
}

impl Drop for AppLock {
    fn drop(&mut self) {
        // Best-effort cleanup. Flock is released automatically on file close.
        // Removing PID + socket files signals "no live instance" to the next
        // launcher; if removal fails (other process moved them, etc.) we log
        // and proceed.
        if let Err(e) = std::fs::remove_file(&self.pid_path) {
            tracing::debug!(error = %e, "remove pid file");
        }
        if let Err(e) = std::fs::remove_file(&self.sock_path) {
            tracing::debug!(error = %e, "remove sock file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn first_acquire_succeeds_returns_some() {
        let tmp = TempDir::new().unwrap();
        let lock = AppLock::try_acquire(tmp.path()).unwrap();
        assert!(lock.is_some(), "first acquire must succeed");
    }

    #[test]
    fn second_acquire_in_same_process_returns_none_or_errors() {
        let tmp = TempDir::new().unwrap();
        let _first = AppLock::try_acquire(tmp.path()).unwrap().expect("first");
        let second = AppLock::try_acquire(tmp.path()).unwrap();
        assert!(second.is_none(), "second acquire must report contention");
    }

    #[test]
    fn stale_pid_file_no_live_holder_succeeds() {
        let tmp = TempDir::new().unwrap();
        // Simulate stale PID file: no flock held.
        std::fs::write(tmp.path().join("dat0.pid"), "99999").unwrap();
        let lock = AppLock::try_acquire(tmp.path()).unwrap();
        assert!(
            lock.is_some(),
            "acquire must succeed when PID file exists but no flock is held"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn uds_round_trip_delivers_message() {
        use std::sync::Arc;
        use std::sync::Mutex;

        let tmp = TempDir::new().unwrap();
        let lock = AppLock::try_acquire(tmp.path()).unwrap().expect("acquire");
        let received: Arc<Mutex<Vec<OpenWindowMessage>>> = Arc::default();

        let received_clone = Arc::clone(&received);
        let state = tmp.path().to_path_buf();
        let handle = tokio::spawn(async move {
            let _ = lock
                .serve(move |msg| received_clone.lock().unwrap().push(msg))
                .await;
        });

        // Give the listener a moment to bind.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        AppLock::forward_open_window(
            &state,
            OpenWindowMessage {
                paths: vec!["/tmp/sample.csv".into()],
            },
        )
        .expect("forward");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let got = received.lock().unwrap().clone();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].paths, vec![PathBuf::from("/tmp/sample.csv")]);

        handle.abort();
    }
}
