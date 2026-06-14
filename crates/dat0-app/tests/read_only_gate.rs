//! Integration-test regression anchor for the P8 T8 read-only gate predicate.
//!
//! [`dat0_app::grid::edit_ops::mutation_blocked`] is the single source of
//! truth for the Inspect-mode read-only contract. All mutation entry points
//! call it as their first statement; these tests lock the contract so a future
//! refactor cannot silently flip the semantics.

use dat0_app::grid::edit_ops::mutation_blocked;

#[test]
fn blocked_when_read_only() {
    assert!(
        mutation_blocked(true),
        "read_only=true must block mutations"
    );
}

#[test]
fn not_blocked_when_editable() {
    assert!(
        !mutation_blocked(false),
        "read_only=false must allow mutations"
    );
}
