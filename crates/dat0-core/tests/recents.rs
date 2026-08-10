use dat0_core::recents::{RecentEntry, Recents};
use tempfile::tempdir;

#[test]
fn empty_recents_starts_empty() {
    let dir = tempdir().unwrap();
    let r = Recents::with_path(dir.path().join("recents.json"));
    assert!(r.list().is_empty());
}

#[test]
fn push_then_persist_then_reload() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("recents.json");

    let mut r = Recents::with_path(p.clone());
    r.push(RecentEntry::Workspace {
        path: "/home/jane/project".into(),
    })
    .unwrap();
    r.push(RecentEntry::Package {
        path: "/tmp/q.dat0".into(),
    })
    .unwrap();
    drop(r);

    let r2 = Recents::with_path(p);
    let list = r2.list();
    assert_eq!(list.len(), 2);
    // MRU order: most recent first
    assert!(
        matches!(&list[0], RecentEntry::Package { path } if path == &std::path::PathBuf::from("/tmp/q.dat0"))
    );
}

#[test]
fn duplicate_push_promotes_to_top() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("recents.json");
    let mut r = Recents::with_path(p);
    r.push(RecentEntry::Workspace { path: "/a".into() })
        .unwrap();
    r.push(RecentEntry::Workspace { path: "/b".into() })
        .unwrap();
    r.push(RecentEntry::Workspace { path: "/a".into() })
        .unwrap();
    let list = r.list();
    assert_eq!(list.len(), 2);
    assert!(
        matches!(&list[0], RecentEntry::Workspace { path } if path == &std::path::PathBuf::from("/a"))
    );
}
