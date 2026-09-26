//! Work kept off the window's thread.
//!
//! A Dioxus task runs on the thread that draws the window. The package code
//! reads, hashes and writes whole files between its awaits, so a large package
//! would freeze the window while it did. [`run`] takes such work to the
//! runtime's blocking pool, where the engine's own tasks still run.

use std::future::Future;

use anyhow::Context as _;

/// Run `work` to completion on the blocking pool and hand back its result.
pub async fn run<T: Send + 'static>(
    work: impl Future<Output = anyhow::Result<T>> + Send + 'static,
) -> anyhow::Result<T> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || runtime.block_on(work))
        .await
        .context("the work stopped before it finished")?
}

#[cfg(test)]
mod tests {
    use super::run;

    #[tokio::test(flavor = "multi_thread")]
    async fn work_runs_off_the_calling_thread_and_can_spawn_its_own() {
        let here = std::thread::current().id();
        let (there, nested) = run(async {
            let nested = tokio::task::spawn_blocking(|| 7).await?;
            anyhow::Ok((std::thread::current().id(), nested))
        })
        .await
        .unwrap();
        assert_ne!(here, there);
        assert_eq!(nested, 7);
    }

    async fn boom() -> anyhow::Result<()> {
        panic!("boom")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn work_that_panics_is_an_error() {
        assert!(run(boom()).await.is_err());
    }
}
