//! P7a T2: workspace flock acquire / contention / release.
use dat0_app::workspace::lock::WorkspaceLock;

#[test]
fn first_acquire_succeeds_second_contends_release_frees() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lock_path = tmp.path().join("lock");

    let first = WorkspaceLock::try_acquire(&lock_path).unwrap();
    assert!(first.is_some(), "first acquire must succeed");

    let second = WorkspaceLock::try_acquire(&lock_path).unwrap();
    assert!(second.is_none(), "second acquire must report contention");

    drop(first); // releases the flock
    let third = WorkspaceLock::try_acquire(&lock_path).unwrap();
    assert!(
        third.is_some(),
        "acquire must succeed after the holder drops"
    );
}
