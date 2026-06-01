# dat0 P4b T0 probe — Clipboard / AccessKit / `__dat0_rowid`

Author-time verification for the three P4b seams that later tasks depend on.
Each section records a call **verified against the pinned source / runtime**, not
recalled from memory. Pins: GPUI `=0.2.2`, gpui-component `v0.5.1`
(rev `0f0ab35`), duckdb-rs `=1.4.4`.

Source locations:
- GPUI: `~/.cargo/registry/src/index.crates.io-*/gpui-0.2.2/`
- gpui-component: `~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/0f0ab35/`

---

## 1. Clipboard API (for T7 — TSV copy/cut/paste)

GPUI exposes clipboard read/write **on `App`** (and on any `cx` that derefs to
it, e.g. `Context<V>` / `Window`-bearing closures). The item type is
`gpui::ClipboardItem`, constructed for plain text via `new_string`.

Verified signatures:

```rust
// gpui-0.2.2/src/platform.rs:1517
impl ClipboardItem {
    pub fn new_string(text: String) -> Self;              // :1519
    pub fn text(&self) -> Option<String>;                 // :1553  (concats all String entries)
}

// gpui-0.2.2/src/app.rs
impl App {
    pub fn write_to_clipboard(&self, item: ClipboardItem); // :1041
    pub fn read_from_clipboard(&self) -> Option<ClipboardItem>; // :1053
}
```

**Exact calls T7 will use:**

```rust
// Write the TSV blob to the system clipboard:
cx.write_to_clipboard(gpui::ClipboardItem::new_string(tsv_string));

// Read on paste (None when the clipboard holds no string entry):
let pasted: Option<String> = cx
    .read_from_clipboard()
    .and_then(|item| item.text());
```

Notes:
- `ClipboardItem::text()` returns `Option<String>` (None if no `String` entry,
  e.g. clipboard holds only an image), so paste must handle `None`.
- Metadata variants (`new_string_with_metadata`,
  `new_string_with_json_metadata<T: Serialize>`) exist but T7 does not need
  them — the gate is plain-TSV round-trip with Excel/Sheets (decision 6).
- The `TestAppContext`/test platform also implement
  `write_to_clipboard`/`read_from_clipboard` (`app/test_context.rs:290,296`;
  `platform/test/platform.rs:402,411`), so T7's clipboard logic is unit-testable
  under `gpui::test` without a real window. The Linux **headless** client is a
  no-op (`platform/linux/headless/client.rs:114`) — relevant only to headless CI.

---

## 2. AccessKit / selection a11y tree

**Finding: AccessKit is ENTIRELY ABSENT from both pins.**

- `grep -rin accesskit gpui-0.2.2/Cargo.toml` → no matches.
- `grep -rln 'accesskit|AccessibilityNode|AccessKit' gpui-0.2.2/src` → no matches.
- `grep -rln 'accesskit|AccessibilityNode|AccessKit'` over the gpui-component
  checkout → no matches.

There is no accessibility-node tree, no `AccessibilityNode`, and no AccessKit
adapter on the pinned GPUI 0.2.2 / gpui-component 0.5.1. A screen-reader
selection tree is **not exposable** on these pins without forking GPUI.

**Conclusion (per design decision 5):** SR exposure is infeasible on pinned
GPUI; P4b ships the **operability-only** a11y baseline — keyboard navigation
covers all selection variants as pure input handling, no AccessKit-tree
dependency. Screen-reader semantics are deferred to P10 hardening.

**Candidate deferral for the controller to file:** `D-NNN` — "AccessKit / SR
selection-tree exposure" — blocked on a GPUI version that ships an AccessKit
adapter; target P10. (Decision 5 already scopes the operability baseline as
sufficient for the P4b exit gate.)

---

## 3. `__dat0_rowid` deterministic surrogate (for T3 row identity)

Verified against the real duckdb-rs `=1.4.4` via a throwaway test
(`crates/dat0-engine/tests/scratch_rowid_probe.rs`, compiled + run, then
deleted). The construct yields a **gap-free `0..n-1`** key that follows
insertion/scan (`rowid`) order, not value order, and is **stable across reads**.

Verified SQL (run on a table whose value-column order is deliberately
non-monotonic to prove the surrogate tracks `rowid`, not values):

```sql
ALTER TABLE t ADD COLUMN __dat0_rowid BIGINT;
UPDATE t SET __dat0_rowid = seq.rn
  FROM (
    SELECT rowid AS rid,
           (row_number() OVER (ORDER BY rowid)) - 1 AS rn
    FROM t
  ) seq
WHERE t.rowid = seq.rid;
```

Probe assertions that passed (duckdb 1.4.4, in-memory connection):
- `SELECT __dat0_rowid ... ORDER BY __dat0_rowid` → `[0, 1, 2, 3]` for 4 rows
  (gap-free, zero-based).
- The mapping follows insertion order (`zebra, apple, mango, kiwi`), confirming
  `row_number() OVER (ORDER BY rowid)` keys off scan order, not column values.
- Re-running the SELECT yields an identical mapping (stable; no re-numbering).
- `max(__dat0_rowid) == count(*) - 1` (no off-by-one, no gaps).

This is the form **T3 should use** to inject the surrogate at import. The
`rowid` pseudo-column is DuckDB-specific; per decision 1 the column is modeled
behind a `RowKey` enum so a non-DuckDB source (SQLite scan, MotherDuck) can
supply its own surrogate without a wire break.

Test command run:
`cargo test -p dat0-engine --test scratch_rowid_probe -- --nocapture` →
`test dat0_rowid_is_gap_free_and_stable ... ok`.
```
running 1 test
test dat0_rowid_is_gap_free_and_stable ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
