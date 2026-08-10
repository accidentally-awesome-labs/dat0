//! The dependencies the Dioxus UI is supposed to have *shed*.
//!
//! Each of these left for a specific reason, and each could creep back in
//! through a transitive edge without anyone noticing:
//!
//! | crate | why it is gone |
//! |---|---|
//! | `gpui`, `gpui-component` | the whole point |
//! | `lsp-types`, `ropey` | dat0 owned SQL completion ranking only because `gpui-component`'s `CompletionProvider` demanded an LSP shape; `@codemirror/lang-sql` does it properly |
//! | `tree-sitter-sequel` | syntax highlighting is the editor's job now |
//! | `image`, `smallvec` | plumbing for `RenderImage`; charts render to SVG |
//! | `accesskit`, `kittest` | a11y was test-only under GPUI; in a WebView it is plain DOM, in every build |
//!
//! A `cargo tree` grep in CI would catch the same thing, but only for the
//! configuration CI happens to build. This runs wherever the tests do.

use std::process::Command;

/// Crates that must not appear in `dat0-ui`'s normal dependency tree.
const BANNED: &[&str] = &[
    "gpui",
    "gpui-component",
    "gpui-component-assets",
    "lsp-types",
    "ropey",
    "tree-sitter-sequel",
    "accesskit",
    "accesskit_consumer",
    "kittest",
];

#[test]
fn the_ui_carries_none_of_the_shed_dependencies() {
    let out = Command::new(env!("CARGO"))
        .args(["tree", "-e", "normal", "--prefix", "none"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree runs");
    assert!(
        out.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let tree = String::from_utf8_lossy(&out.stdout);

    // Match the crate name at the start of a line so a *mention* — a repo URL,
    // a similarly-named crate — cannot trip the gate.
    let mut found: Vec<&str> = Vec::new();
    for banned in BANNED {
        if tree
            .lines()
            .any(|l| l.split_whitespace().next() == Some(banned))
        {
            found.push(banned);
        }
    }
    assert!(
        found.is_empty(),
        "dat0-ui has picked up dependencies the migration removed: {found:?}"
    );
}

#[test]
fn the_scan_can_actually_find_something() {
    // Teeth: if `cargo tree`'s output shape changed, the assertion above would
    // pass against an empty string and guard nothing.
    let out = Command::new(env!("CARGO"))
        .args(["tree", "-e", "normal", "--prefix", "none"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree runs");
    let tree = String::from_utf8_lossy(&out.stdout);
    for expected in ["dioxus", "dat0-core", "duckdb"] {
        assert!(
            tree.lines()
                .any(|l| l.split_whitespace().next() == Some(expected)),
            "the tree scan found no {expected}; the parse is broken, not the tree"
        );
    }
}
