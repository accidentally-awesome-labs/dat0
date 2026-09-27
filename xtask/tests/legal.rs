//! Step 7: both bundles carry the notices their licences ask to travel with
//! the binary, and the NOTICE carries each licence's text, not only its name.

use std::path::Path;
use xtask::legal::{FILES, copy_into};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

#[test]
fn every_notice_a_bundle_carries_is_in_the_repository() {
    for (source, _) in FILES {
        assert!(repo_root().join(source).is_file(), "{source} is missing");
    }
}

/// What a bundle receives, read back: dat0's licence, the NOTICE with each
/// licence's text, and the fonts' OFL.
#[test]
fn a_bundle_carries_the_licence_texts() {
    let dir = tempfile::tempdir().unwrap();
    copy_into(repo_root(), dir.path()).unwrap();
    let read = |name: &str| {
        std::fs::read_to_string(dir.path().join(name))
            .unwrap_or_else(|e| panic!("the bundle has no {name}: {e}"))
    };
    let notice = read("NOTICE.md");
    for text in [
        // MIT, Apache 2.0 and ISC, the licences most of the tree uses.
        "Permission is hereby granted, free of charge",
        "TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION",
        "Permission to use, copy, modify, and/or distribute this software",
    ] {
        assert!(notice.contains(text), "NOTICE.md lacks {text:?}");
    }
    assert!(read("LICENSE").contains("Apache License"));
    assert!(read("LICENSE-geist").contains("SIL OPEN FONT LICENSE"));
    assert!(read("LICENSE-lucide").contains("ISC License"));
}

#[test]
fn the_notices_are_copied_whole() {
    let dir = tempfile::tempdir().unwrap();
    copy_into(repo_root(), dir.path()).unwrap();
    for (source, name) in FILES {
        assert_eq!(
            std::fs::read(dir.path().join(name)).unwrap(),
            std::fs::read(repo_root().join(source)).unwrap(),
            "{name}"
        );
    }
}
