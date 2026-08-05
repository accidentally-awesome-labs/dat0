# Slice B11 — `window.rs` extraction (implementation plan)

> **Execution:** INLINE by the controller, no subagents (per the B11 kickoff block and
> [[dat0-kbdnav-backlog-nextup]]). Steps use checkbox syntax for tracking.

**Goal:** turn `crates/dat0-app/src/window.rs` (8,672 lines / 229 fns) into
`window/mod.rs` (~845) plus 14 child modules, changing no behaviour, no pixel, and no public path.

**Architecture:** a file→directory conversion. `WorkspaceShell`'s struct and constructor stay in
`mod.rs`; each child module carries one topic and re-opens `impl WorkspaceShell` to hold its
methods. Rust's downward visibility means children touch the parent's private fields with no
annotation; methods crossing back the other way get `pub(super)`, and the compiler enumerates
exactly which via `E0624`. The 13 externally-referenced free functions are restored by explicit
`pub use` in `mod.rs`, so no file outside `src/window/` changes at all.

**Tech stack:** Rust 1.97.0 (pinned), gpui 0.2.2, gpui-component pinned rev `0f0ab35`. No new
dependency. Python 3 for the two one-off gate scripts (§B, §C) — not shipped, not committed as code.

Design doc: `docs/plans/2026-08-05-dat0-ui-redesign-b11-window-extraction-design.md`.

---

## Global constraints

Every task's requirements implicitly include all of these.

- **Behaviour-neutral.** No function body may change. Allowed edits to a moved item are exactly:
  its visibility keyword, its indentation, and the `use` lines that now sit at the top of its new
  file. Nothing else. §B enforces this mechanically.
- **No call-site edits outside `src/window/`.** If a task finds itself editing a test file or
  another `src/` module, stop — the `pub use` re-export list in `mod.rs` is wrong.
- **`cargo fmt --all` before every commit.** DCO: every commit uses `git commit -s`.
- **Never write the literal skip-CI marker in a commit message**, even quoted in prose
  ([[dat0-dev-workflow]]; it silently suppressed two main runs during A1).
- **Commit messages contain no backticks** when passed via `-m` — zsh command-substitutes them and
  they vanish. Use `-F -` with a heredoc (this plan does throughout).
- **`clippy::items-after-test-module`** is an error under `-D warnings`: in every file, any
  `#[cfg(test)] mod` must be the last item.
- **Preserve `DOCS_URL` / `DISCORD_URL`** verbatim (interim consts; P11b/P11c swaps them).
- **Line ranges in this plan are valid at base `68f01c3` only.** They shift the moment a task
  removes lines. Every task regenerates current ranges with §A. Never trust a stale number —
  locate items by name.

## Standing local gate

⚠ **Amended during execution (owner-approved).** The original plan ran the full 118-binary suite
after every task. Measured at T1, that is ~10+ hours of compute across the slice — these are gpui
integration tests that each open a window, and they run at roughly 12 binaries per several minutes.

**Per task:** `fmt` + `clippy --workspace --all-targets -D warnings` + the body digest.
`--all-targets` **compiles all 118 test binaries**, which catches every resolution, visibility and
import failure — the entire failure mode of a move — and the digest catches body edits. Together
they cover what a move can break.

**Full suite RUNS at five checkpoints:** T1 (baseline), T2 (first real move — proves the
`use super::*` mechanism end to end), T5 (the feature-gated block), T14 (dock, the highest-risk
move), T15 (`render`), and T16 (final, all three feature combinations).

Every commit still compiles cleanly, so bisectability is preserved.

Run at the end of every task. All must pass before committing.

```bash
cd /Users/salar/Projects/dat0
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings          # must exit 0
cargo test -p dat0-app 2>&1 | tee /tmp/b11-gate.log | tail -5
grep -c "^test result: ok" /tmp/b11-gate.log                    # expect 118
```

Feature-matrix sweep (T0, T16, and any task touching `test_support.rs` or a `cfg(test)` mod):

```bash
cargo test -p dat0-app --features a11y-capture > /tmp/b11-a11y.log 2>&1
cargo test -p dat0-app --features a11y-capture,gallery > /tmp/b11-gal.log 2>&1
grep -c "^test result: ok" /tmp/b11-a11y.log /tmp/b11-gal.log   # expect 118 each
```

⚠ Count test binaries by redirecting to a file, never through `| head` — `head` SIGPIPEs cargo
mid-write and truncates the count (A6 miscounted 51 instead of 109 this way).

⚠ `cargo test --workspace` and `cargo bench` are **unrunnable on this machine** (macOS 27 /
Xcode 26.6 vs vendored DuckDB Thrift). CI is the only place they run. This is pre-existing and
reproduces on `main` — verify with `git checkout main && cargo test --workspace` before blaming
this branch.

---

## A. Manifest generator (used by every move task)

Save to `/tmp/b11_manifest.py`. Prints every top-level and `impl`-level item with
doc-comment-and-attribute-aware boundaries, so a move never severs a `///` block from its function.

```python
#!/usr/bin/env python3
"""Item manifest for a Rust file: start, end, size, scope, kind, name."""
import re, sys

FN = re.compile(
    r"^(?P<i>\s*)(?:pub(?:\([a-z()]+\))?\s+)?(?:async\s+)?"
    r"(?P<kind>fn|struct|enum|const|static|type|mod|impl)\s+(?P<name>[a-zA-Z0-9_]+)"
)

def main(path):
    L = open(path).read().split("\n")
    starts = []
    for i, line in enumerate(L):
        m = FN.match(line)
        if m and len(m.group("i")) in (0, 4):
            starts.append((i, len(m.group("i")), m.group("kind"), m.group("name")))

    def real_start(idx):
        j = idx - 1
        while j >= 0:
            s = L[j].strip()
            if s.startswith(("///", "//!", "#[", "#![")):
                j -= 1
            elif s.startswith("//") and j > 0 and (
                L[j - 1].strip().startswith("//") or L[j + 1].strip().startswith(("///", "#["))
            ):
                j -= 1
            else:
                break
        return j + 1

    for k, (i, ind, kind, name) in enumerate(starts):
        a = real_start(i)
        b = real_start(starts[k + 1][0]) - 1 if k + 1 < len(starts) else len(L) - 1
        print(f"{a+1}\t{b+1}\t{b-a+1}\t{'M' if ind else 'T'}\t{kind}\t{name}")

main(sys.argv[1])
```

Usage: `python3 /tmp/b11_manifest.py crates/dat0-app/src/window.rs | grep -w <name>`

## B. Body-digest gate (T0 and T16)

Save to `/tmp/b11_digest.py`. Compares every function body in `window.rs` at a git ref against
every function body under `window/` on disk, normalising away only what a legitimate move may
change: leading whitespace, blank lines, `use` lines, and visibility keywords.

⚠ **T0 found and fixed TWO real defects in the authoring-time draft of this script.** The listing
below is the corrected version; see design §10 for the full account.

1. **Paths resolved against the caller's cwd**, so running it from anywhere but the repo root took
   a wrong branch. Now resolves the repo root via `git rev-parse --show-toplevel`, and raises
   rather than falling through when neither `window.rs` nor `window/` exists.
2. **Bodies were bounded by "until the next `fn`"**, which makes the last function before any
   non-`fn` item swallow that item. `bounding_rect` had absorbed `impl Render for WorkspaceShell {`,
   so moving `render` out reported *both* as `CHANGED` — two false positives on a perfectly correct
   move. A gate that cries wolf at every task is worse than none. Bodies are now bounded by **brace
   matching**, so a body depends only on itself.

★ The second defect could only be found by running the digest across a **real move**. The in-place
perturbation in T0 step 6 passed happily against the broken version — testing a movement-tolerance
gate with a non-movement probe proves nothing. T0 step 6 below is written accordingly.

```python
#!/usr/bin/env python3
"""B11 no-drift gate.  Usage: b11_digest.py <base-ref>   Exit 0 = identical."""
import re, subprocess, sys
from pathlib import Path

ROOT = Path(subprocess.run(
    ["git", "rev-parse", "--show-toplevel"],
    capture_output=True, text=True, check=True,
).stdout.strip())
SRC = Path("crates/dat0-app/src")   # repo-relative, for `git show`
ABS = ROOT / SRC                    # absolute, for disk reads

FN_RE = re.compile(
    r"^(?P<indent>\s*)(?:pub(?:\([a-z()]+\))?\s+)?(?:async\s+)?fn\s+(?P<name>[a-zA-Z0-9_]+)"
)
STRIP = re.compile(r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'|//.*$')

def normalize(lines):
    out = []
    for raw in lines:
        s = raw.strip()
        if not s or s.startswith("use ") or s.startswith("pub use "):
            continue
        s = re.sub(r"\bpub\([a-z()]+\)\s+", "", s)
        s = re.sub(r"^pub\s+", "", s)
        out.append(s)
    return "\n".join(out)

def body_end(lines, start):
    """Index one past the fn's closing brace, found by brace matching.

    Bounding a body by "until the next fn" makes the last fn before any non-fn
    item swallow that item, so moving a neighbouring item reports the untouched
    fn as CHANGED. Brace matching makes a body depend only on itself.
    """
    depth, seen = 0, False
    for i in range(start, len(lines)):
        code = STRIP.sub("", lines[i])
        for ch in code:
            if ch == "{":
                depth += 1
                seen = True
            elif ch == "}":
                depth -= 1
                if seen and depth == 0:
                    return i + 1
    return len(lines)

def extract(text, origin):
    lines = text.split("\n")
    fns = {}
    for i, line in enumerate(lines):
        m = FN_RE.match(line)
        if not (m and len(m.group("indent")) in (0, 4, 8)):
            continue
        name = m.group("name")
        key, n = name, 2
        while key in fns:
            key = f"{name}#{n}"
            n += 1
        fns[key] = (normalize(lines[i:body_end(lines, i)]), origin)
    return fns

def main():
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    base = sys.argv[1]
    before = extract(subprocess.run(
        ["git", "show", f"{base}:{SRC}/window.rs"],
        capture_output=True, text=True, check=True).stdout, "window.rs")
    after = {}
    win, flat = ABS / "window", ABS / "window.rs"
    if win.is_dir():
        files = sorted(win.rglob("*.rs"))
    elif flat.is_file():
        files = [flat]
    else:
        raise SystemExit(f"neither {win} nor {flat} exists — unexpected tree state")
    for f in files:
        for key, val in extract(f.read_text(), f.name).items():
            k, n = key, 2
            while k in after:
                k = f"{key}#{n}"
                n += 1
            after[k] = val
    missing = sorted(set(before) - set(after))
    added = sorted(set(after) - set(before))
    changed = sorted(k for k in set(before) & set(after) if before[k][0] != after[k][0])
    print(f"before: {len(before)} fns in window.rs @ {base}")
    print(f"after:  {len(after)} fns across {len(files)} file(s)")
    for k in missing:
        print(f"  MISSING  {k}")
    for k in added:
        print(f"  ADDED    {k}  (in {after[k][1]})")
    for k in changed:
        print(f"  CHANGED  {k}  (now in {after[k][1]})")
        b, a = before[k][0].split("\n"), after[k][0].split("\n")
        for i in range(max(len(a), len(b))):
            lb = b[i] if i < len(b) else "<eof>"
            la = a[i] if i < len(a) else "<eof>"
            if lb != la:
                print(f"      - {lb}\n      + {la}")
                break
    bad = len(missing) + len(added) + len(changed)
    print("DIGEST OK — every body moved verbatim" if not bad
          else f"DIGEST FAILED — {bad} discrepancies")
    return 1 if bad else 0

sys.exit(main())
```

Usage from the repo root: `python3 /tmp/b11_digest.py 68f01c3`

**Expected `ADDED` entries at T16 (the only legitimate ones):** none. Every function that exists at
T16 existed at base. If the digest reports `ADDED`, a task wrote new code — which this slice
forbids. The only new code in the whole branch is `tests/window_module_ratchet.rs` (T16), which
lives outside `src/window/` and is therefore invisible to the digest.

## C′. Two tasks are BLOCK moves, not name-based moves

T2–T4 and T6–T14 move a *named set of items*. **T5 and T15 do not** — each moves one contiguous
`impl` block whole, and treating them as name lists produces wrong code:

- **T15 `render`** is a **trait** method. A name-based move would wrap it in
  `impl WorkspaceShell`, but it must stay inside `impl Render for WorkspaceShell`. Move the whole
  block, header and closing brace included.
- **T5** is the single `#[cfg(feature = "a11y-capture")] impl WorkspaceShell` block holding all 44
  accessors.

Both shapes were already exercised by T0 probes 2 and 3, which moved exactly these two blocks and
compiled — so both are pre-validated.

## C″. Empty test-module wrappers must be deleted, not left behind

Two `#[cfg(test)] mod` wrappers in `mod.rs` are emptied by tasks that take their contents. An empty
`mod tests { }` compiles and would survive every gate silently:

- `mod tests` — T7 takes 5 chart items, T12 takes the last one. **T12 deletes the wrapper.**
- `mod live_refresh_tests` — T8 takes all 5 of its items. **T8 deletes the wrapper.**

Each task writes its tests into a fresh `#[cfg(test)] mod tests` at the bottom of its own new file.
Verify after T12: `grep -n "mod tests\|live_refresh_tests" crates/dat0-app/src/window/mod.rs`
returns nothing.

## C. The move recipe

T2–T15 are the same procedure with different inputs. It is written out in full here once; each task
below supplies only its own item list, its own hazards, and its own expected `pub(super)` surface.
No task says "same as Task N" for anything an implementer must *do* — the procedure is here, in
full, and each task states its own data.

1. **Regenerate ranges.** `python3 /tmp/b11_manifest.py crates/dat0-app/src/window.rs > /tmp/m.txt`,
   then locate this task's items **by name** in `/tmp/m.txt`. The base-relative ranges quoted in the
   task are for orientation and for cross-checking the item count — not for cutting.
2. **Create the file** `crates/dat0-app/src/window/<name>.rs` with a `//!` module doc naming what
   the module owns and why those items belong together (one short paragraph — this is the reader's
   orientation and the ratchet's justification).
3. **Cut and paste** each item verbatim, in its original relative order, including its `///` doc
   comment and any attributes. For methods, wrap them in `impl WorkspaceShell { … }` in the new
   file (a second `impl` block for the same type in the same crate is legal).
4. **Declare it** in `mod.rs`: add `mod <name>;` in the alphabetically-sorted `mod` block. Add
   `pub use <name>::{…};` for any item that was `pub`/`pub(crate)` at base and is named from
   outside `src/window/` (see the re-export list in T1).
5. **Open the file with `use super::*;`** — measured at T0, not assumed. A `use` declaration is an
   item with visibility, and it defaults to private, so by the *same* downward-visibility rule that
   exposes the parent's fields, a child inherits every one of the parent's private import bindings
   through a glob. `cargo clippy --all-targets -D warnings` accepts it (`wildcard_imports` is
   pedantic-only and not enabled here). Add `use gpui::prelude::*;` where trait methods are needed,
   plus any import the parent does not already carry — the compiler names those exactly.

   Do **not** hand-curate a 15-file import matrix; that was the original plan text and T0 showed it
   to be unnecessary work. If `mod.rs` later reports `unused_imports` because its last local
   consumer moved out, move that specific `use` down to the module that still needs it.
6. **Compile and let `E0624` enumerate.** `cargo check -p dat0-app` and mark each reported method
   `pub(super)` — nothing wider unless an external path needs it. Do **not** pre-emptively widen
   visibility: an unreported method must stay private, and the error list is the inventory.
7. **Standing local gate** (top of this document).
8. **Digest spot-check:** `python3 /tmp/b11_digest.py 68f01c3` — expect only `MISSING` entries for
   items not yet moved to be *absent*; there must be **zero `CHANGED`** at every task, not just at
   the end. A `CHANGED` here means the paste altered a body.
9. **Commit** with the message given in the task.

---

## Task T0: hard gate

**Files:** none committed except the probe log appended to the design doc §10.

**Produces:** a go/no-go on the split strategy, and the two gate scripts on disk.

- [ ] **Step 1: Save both scripts**

Write §A to `/tmp/b11_manifest.py` and §B to `/tmp/b11_digest.py`.

- [ ] **Step 2: Probe 1 — visibility, in miniature**

```bash
cat > /tmp/vis.rs <<'EOF'
mod window {
    pub struct S { x: u32 }
    mod child {
        impl super::S {
            fn hidden(&self) -> u32 { self.x }
        }
    }
    impl S {
        pub fn new() -> Self { S { x: 7 } }
        pub fn call(&self) -> u32 { self.hidden() }
    }
}
fn main() { println!("{}", window::S::new().call()); }
EOF
rustc --edition 2021 -o /tmp/vis /tmp/vis.rs
```

Expected: **fails** with `error[E0624]: method 'hidden' is private`, and **no error on `self.x`** —
proving the child reads the parent's private field freely. Then change `fn hidden` to
`pub(super) fn hidden`, recompile, and expect it to build and print `7`.

*(Pre-verified at plan-authoring time; re-run to confirm the pinned 1.97.0 toolchain agrees.)*

- [ ] **Step 3: Probe 1b — visibility, in dat0**

Prove the same in the real crate, where gpui traits and a 361-field struct are in scope. Create
`crates/dat0-app/src/window/` is premature here, so use a throwaway inline module instead: append
to the bottom of `window.rs`

```rust
mod b11_probe {
    impl super::WorkspaceShell {
        fn probe_reads_private_field(&self) -> bool {
            self.sql_console_visible_flag_probe()
        }
    }
}
```

Replace `sql_console_visible_flag_probe()` with a real **private** field read chosen from the struct
(e.g. `self.left_panel_visible`), and add a call to `probe_reads_private_field` from an existing
`mod.rs`-level method. Run `cargo check -p dat0-app`.

Expected: field read compiles; the call from the parent fails `E0624`. **Then revert the probe
entirely and `touch crates/dat0-app/src/window.rs`** (A6's stale-binary trap: a correctly-reverted
source reported RED until the mtime was bumped).

- [ ] **Step 4: Probe 2 — `impl Render` from a child module**

Create `crates/dat0-app/src/window/` as a directory with `mod.rs` = the current `window.rs`, plus a
`render.rs` holding **only** the `impl Render for WorkspaceShell` block moved verbatim. Add
`mod render;` to `mod.rs`. Run `cargo check -p dat0-app`.

Expected: compiles. `Render` is a foreign trait on a local type; coherence is per-crate, so module
placement is irrelevant. **B7's design also had a "settled" central choice that gpui's entity-leasing
rules made unbuildable — this probe is why we check rather than assume.**

Revert to a single `window.rs` afterwards (T1 does this move properly).

- [ ] **Step 5: Probe 3 — `cfg(feature)` impl from a child module**

Same shape: `window/test_support.rs` holding the `#[cfg(feature = "a11y-capture")] impl
WorkspaceShell` block. Then:

```bash
cargo check -p dat0-app --features a11y-capture --all-targets
```

Expected: compiles, and the integration tests still resolve `shell.dock_mounted_for_test()` etc.
unchanged (they are `pub`, and the path `crate::window::WorkspaceShell` is unmoved). Revert.

- [ ] **Step 6: Probe 4 — digest correctness**

A movement-tolerance gate must be exercised **by a movement**. Four sub-probes, in order; 4b and 4c
are the ones that matter and the ones that found the defects.

**4a — identity.** Against the unchanged tree:

```bash
python3 /tmp/b11_digest.py 68f01c3          # expect: 229 fns both sides, DIGEST OK, exit 0
```

**4a′ — in-place edit.** Change `configured_memory_budget`'s last expression to
`crate::settings::budget::memory_budget_bytes(&store) + 1`, re-run, expect
`CHANGED configured_memory_budget` with the ± line pair and exit 1. Then
`git checkout` + **`touch`** and confirm green again.

⚠ **Do not stop here.** This sub-probe passes against a digest that is badly broken — it never
moves anything, and movement is the whole point.

**4b — legitimate move must stay GREEN.** Reuse the probe-2 tree state (`window/mod.rs` +
`window/render.rs` with `impl Render` moved verbatim) and run the digest. Expect
`229 fns across 2 file(s)`, `DIGEST OK`, exit 0.

Any `CHANGED` here is a **gate defect, not a code defect** — nothing was edited. Fix the gate before
proceeding; a digest that reports noise on correct moves will be ignored by T15 and the slice loses
its main evidence.

**4c — edit inside a moved file must go RED.** In that same two-file state, change one line in
`render.rs`'s body (e.g. `render_dialog_layer` → `render_sheet_layer`), re-run, and expect
`CHANGED render (now in render.rs)` naming the exact ± pair, exit 1.

**4d — cwd independence.** Run the digest from `crates/dat0-app/src` as well as the repo root; both
must behave identically. Then revert to a single `window.rs` and confirm 4a green once more.

- [ ] **Step 7: STOP clause**

If probe 1b or probe 2 fails, **stop and report before writing any further task.** The whole split
shape is downstream of both. Probe 3 failing means `test_support.rs` merges back into `mod.rs`
(one module fewer, no other change). Probe 4 failing means the digest is worthless and §5.2's
guarantee is void — report it rather than proceeding on a gate that cannot fail.

- [ ] **Step 8: Record and commit**

Append a T0 as-built block to the design doc §10: what each probe printed, verbatim.

```bash
git add docs/plans/2026-08-05-dat0-ui-redesign-b11-window-extraction-design.md
git commit -s -F - <<'EOF'
docs(plan): B11 T0 — hard gate results

Four probes before any bulk move:
1. A private method in a child module is invisible to the parent (E0624)
   while the child reads the parent's private fields freely. pub(super)
   resolves it. Verified in miniature and inside dat0-app.
2. impl Render for WorkspaceShell compiles from a child module.
3. A cfg(feature = "a11y-capture") impl block compiles from a child module
   and the integration tests still resolve the accessors unchanged.
4. The body-digest gate is non-vacuous: perturbing one body reports the
   exact changed line and exits 1.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T1: `window.rs` → `window/mod.rs` (rename only)

**Files:**
- Rename: `crates/dat0-app/src/window.rs` → `crates/dat0-app/src/window/mod.rs`

**Interfaces — produces:** the `mod` declaration block and the `pub use` re-export block that every
later task appends to.

Deliberately a standalone commit with **zero content change**, so git records a clean rename and
every later commit reads as a move rather than a delete-plus-add.

- [ ] **Step 1: Rename via git**

```bash
cd /Users/salar/Projects/dat0/crates/dat0-app/src
mkdir window
git mv window.rs window/mod.rs
```

- [ ] **Step 2: Verify nothing else changed**

```bash
cd /Users/salar/Projects/dat0
git status --short          # expect exactly one R (rename) line
cargo check -p dat0-app     # expect success, no warnings
```

`lib.rs:51`'s `pub mod window;` needs no edit — a module resolves to either `window.rs` or
`window/mod.rs`, and the path `crate::window::X` is identical either way.

- [ ] **Step 3: Replace the module doc with a directory map**

The existing 67-line `//!` header is almost entirely about boot (`Application::new`, single-instance
UDS, `WindowRegistry`, Cmd-N). It moves to `boot.rs` in T2. **In this commit, leave it in place** —
T2 moves it with its subject matter. This step is a no-op placeholder in T1 by design; the map doc
is written in T16 once every module exists and can be described accurately.

- [ ] **Step 4: Standing local gate, then commit**

```bash
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app > /tmp/b11-gate.log 2>&1; grep -c "^test result: ok" /tmp/b11-gate.log
git add -A
git commit -s -F - <<'EOF'
refactor(window): window.rs becomes window/mod.rs

Pure rename, zero content change, so git records a rename and every
later commit in this slice reads as a move rather than a delete-plus-add.
The module path crate::window::X is unchanged: a module resolves to
either window.rs or window/mod.rs identically, so lib.rs needs no edit
and no call site anywhere in the tree changes.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T2: `boot.rs`

**Files:**
- Create: `crates/dat0-app/src/window/boot.rs` (~918 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges at `68f01c3`, 10 items):** `focused_session_arc` 887–898 ·
`paths_from_open_urls` 911–928 · `open_window_view` + `spawn_window` 1115–1202 · `DOCS_URL`,
`DISCORD_URL`, `register_menu_action_handlers`, `flush_focused_workspace_sql`, `run_app` 1332–2101 ·
`open_urls_decode_to_local_paths` (unit test) 8467–8496.

Also move: **the 67-line `//!` module doc at lines 1–63** — it describes boot and nothing else.
Convert it to `boot.rs`'s module doc verbatim, only changing the first line to name the module.

**Produces (re-exports `mod.rs` must add):**

```rust
pub use boot::{register_menu_action_handlers, run_app, spawn_window};
```

`focused_session_arc` is private and used by several later modules → mark `pub(super)` when T8/T12
report `E0624`, not before.

**Hazards specific to this task:**
- `DOCS_URL` / `DISCORD_URL` are the interim consts from the 2026-07-21 menu hotfix. They must
  survive verbatim; P11b/P11c swaps them before release. After this task they live at
  `window/boot.rs` — **update the pre-release ops note that currently says `window.rs`.**
- `open_urls_decode_to_local_paths` is a `#[cfg(test)]` test currently inside `mod tests`. It moves
  into a **new** `#[cfg(test)] mod tests` at the **bottom** of `boot.rs` (clippy
  `items-after-test-module`).
- `run_app` is 457 lines and captures the `WindowRegistry` `Arc` into the `Application::run`
  closure. It moves whole; do not attempt to decompose it in this slice.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Confirm the two URL consts are byte-identical to base:

```bash
git show 68f01c3:crates/dat0-app/src/window.rs | grep -A1 'DOCS_URL\|DISCORD_URL'
grep -A1 'DOCS_URL\|DISCORD_URL' crates/dat0-app/src/window/boot.rs
```

- [ ] **Step 3:** Standing local gate + digest spot-check (zero `CHANGED`).
- [ ] **Step 4: Commit**

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract boot.rs

Moves run_app, register_menu_action_handlers, flush_focused_workspace_sql,
open_window_view, spawn_window, paths_from_open_urls and its unit test, and
focused_session_arc out of mod.rs, together with the 67-line module doc that
describes boot and nothing else.

The interim DOCS_URL and DISCORD_URL consts move with their only consumer,
register_menu_action_handlers; P11b/P11c still swaps them before release and
they now live at window/boot.rs.

Bodies are unchanged; run_app keeps its WindowRegistry capture intact.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T3: `workspace_ops.rs`

**Files:**
- Create: `crates/dat0-app/src/window/workspace_ops.rs` (~422 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 14 items):** 64–406 (`recents_push_workspace`, `open_workspace_flow`,
`load_workspace_settings`, `configured_memory_budget`, `open_workspace_at`,
`open_workspace_proceed`, `focus_existing_workspace`, `bring_workspace_to_front`,
`spawn_workspace_window`, `save_workspace_flow`, `promote_focused_into`) · 1068–1114
(`open_recent_n`, `now_epoch_secs`) · 2692–2723 (`maybe_prompt_save_workspace`, a method).

**Produces:**

```rust
pub use workspace_ops::{open_workspace_flow, now_epoch_secs, save_workspace_flow,
                        spawn_workspace_window};
```

**Hazards:** this task moves both free functions *and* one method (`maybe_prompt_save_workspace`),
so `workspace_ops.rs` needs both bare `fn`s and an `impl WorkspaceShell { … }` block.
`promote_focused_into` is 142 lines and the largest single item here.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Standing local gate + digest spot-check.
- [ ] **Step 3: Commit**

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract workspace_ops.rs

Workspace lifecycle: open, save, promote-by-move, recents, and the
settings/memory-budget loaders. Includes maybe_prompt_save_workspace,
the one WorkspaceShell method in this topic, so the module carries both
free functions and an impl block.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T4: `package_ops.rs`

**Files:**
- Create: `crates/dat0-app/src/window/package_ops.rs` (~558 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 12 items):** 407–414 (`PACKAGE_BUDGET`) · 466–886 (`export_package_flow`,
`open_package_flow`, `open_package_at`, `unpack_package_flow`, `unpack_package_into`,
`open_demo_workspace`, `replay_package_flow`) · 1203–1331 (`spawn_recovered_scratch`,
`orphan_scan_emit`, `recovery_scan_emit`, `count_orphan_scratch`).

**Produces:**

```rust
pub use package_ops::{count_orphan_scratch, open_demo_workspace, orphan_scan_emit,
                      recovery_scan_emit, spawn_recovered_scratch};
```

**Hazards:** `PACKAGE_BUDGET` (base line 413) sits between `LeftPanel` and the dock-width consts in
the current const block — take only it, and leave the four dock-width consts for T14. All items
here are free functions; no `impl` block needed.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Standing local gate + digest spot-check.
- [ ] **Step 3: Commit**

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract package_ops.rs

The .dat0 package surface from P8: export, open, unpack, replay, the demo
workspace, and the orphan/recovery scratch scans, plus PACKAGE_BUDGET.
All free functions; four of them are named from outside src/window and are
re-exported from mod.rs so no call site changes.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T5: `test_support.rs`

**Files:**
- Create: `crates/dat0-app/src/window/test_support.rs` (~392 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base range, 44 items in 1 contiguous run):** 8070–8461 — the entire
`#[cfg(feature = "a11y-capture")] impl WorkspaceShell` block, including the comment above it
explaining the `items-after-test-module` ordering.

**Produces:** nothing new. Every accessor is already `pub` and reached as
`shell.<name>_for_test()` through the unmoved `crate::window::WorkspaceShell` path.

**Hazards:**
- The whole block is `#[cfg(feature = "a11y-capture")]`. In the new file, put the attribute on the
  `impl` block exactly as it is now — **do not** hoist it to a `#![cfg(...)]` inner attribute on the
  file, and **do** gate the `mod test_support;` declaration in `mod.rs`:
  ```rust
  #[cfg(feature = "a11y-capture")]
  mod test_support;
  ```
  Without the gate, the module compiles in release builds as an empty file — harmless but it makes
  the feature boundary invisible to a reader.
- The ordering comment moves with the block, but **rewrite its last sentence**: after this task the
  constraint is satisfied structurally (the accessors are alone in their own file), not by manual
  ordering. Leaving the old wording would be a stale claim. This is the one place in the slice
  where prose is edited rather than moved; call it out in the commit message.
- This task changes what compiles under a feature flag → run the **full feature matrix**, not just
  the plain gate.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Full feature-matrix sweep (three combinations, 118 each).
- [ ] **Step 3:** Confirm the accessor count is unchanged:

```bash
grep -c "_for_test" crates/dat0-app/src/window/test_support.rs   # expect 44
```

- [ ] **Step 4:** Digest spot-check, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract test_support.rs

The 44 cfg(a11y-capture) accessors move to their own file as one block.
They stay a single auditable surface because 118 test binaries depend on
them, and the module declaration is feature-gated so the boundary stays
visible to a reader.

The comment explaining clippy's items-after-test-module ordering moves
with the block, with its last sentence rewritten: the constraint is now
satisfied structurally rather than by hand-maintained ordering, since the
accessors are alone in their own file.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T6: `modals.rs` — first `impl` split

**Files:**
- Create: `crates/dat0-app/src/window/modals.rs` (~456 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 16 items):** 2102–2178 (`NamePromptIntent`, `MountedModal`, `push_modal`) ·
5130–5508 (`open_name_prompt`, `open_name_prompt_with`, `restore_modal_focus`, `mounted_modals`,
`open_modal_count`, `on_name_prompt_event`, `show_saved_picker`, `request_command_palette`,
`mount_command_palette`, `on_command_palette_event`, `run_palette_action`, `on_saved_picker_event`,
`dismiss_saved_picker`).

**Produces:** expect `E0624` on the methods `render` and the dock/SQL/chart modules call —
likely `open_name_prompt_with`, `restore_modal_focus`, `mounted_modals`, `open_modal_count`,
`request_command_palette`. Mark exactly those `pub(super)`.

**Hazards — this is the first task that splits `impl WorkspaceShell`:**
- `push_modal` is a generic free function (`fn push_modal<T: ModalContent + Render>`), not a method.
  It sits immediately before the `WorkspaceShell` struct at base; take the function and the two
  types, and **leave the struct**.
- The B1/B2 single-modal invariant lives in these methods. It is behaviour, so it moves untouched —
  but `tests/modal_trap_nav.rs` (10 tests) and the palette tests are the ones that will notice if
  the move is wrong. They run in the standing gate; read their result lines specifically.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Confirm the modal suites ran and passed by name:

```bash
grep -E "modal_trap_nav|command_palette" /tmp/b11-gate.log
```

- [ ] **Step 3:** Standing local gate + digest spot-check, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract modals.rs

The B1/B2 ModalHost surface and B4's command palette: push_modal, the
NamePromptIntent and MountedModal types, name-prompt open/route, the saved
picker, and the four palette methods. First task in the slice to split
impl WorkspaceShell across files; the methods the shell still calls are
marked pub(super) as enumerated by E0624.

The single-modal invariant is behaviour and moves untouched.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T7: `charts.rs`

**Files:**
- Create: `crates/dat0-app/src/window/charts.rs` (~481 lines + 5 unit tests)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 13 items + tests):** 929–1007 (`axis_field`, `set_axis_field`,
`axis_role_key`, `axis_required`, `cycle_axis`) · 3952–4235 (`toggle_chart_panel`, `run_plot_query`,
`render_chart_toolbar`, `open_chart_save_prompt`, `export_chart`) · 4831–4948 (`save_named_chart`,
`open_saved_chart`, `show_chart_with_spec`).

**Plus 5 unit tests** currently inside `mod tests` at base 8513–8573: `spec`,
`required_axis_cycles_over_options_only`, `optional_axis_passes_through_none`,
`value_axis_maps_to_the_field_each_type_reads`, `required_axes_classification`. These move into a
new `#[cfg(test)] mod tests` at the **bottom** of `charts.rs`.

**Hazards:**
- `spec` is a **test helper**, not a test — it has no `#[test]` attribute. Move it with the five
  tests; the digest keys on name, and a helper left behind would show as `MISSING`.
- The base `mod tests` block also holds `bare_table_name_strips_quotes_and_schema`, which belongs to
  T12 (`sql.rs`). **Take only the five chart items**; the `mod tests` block itself stays in `mod.rs`
  until T12 empties it.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Confirm the moved tests actually ran:

```bash
grep -E "required_axis_cycles|required_axes_classification" /tmp/b11-gate.log
```

- [ ] **Step 3:** Standing local gate + digest spot-check, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract charts.rs

The P9a chart surface: the five free axis helpers with their five unit
tests, plot query, toolbar render, save/open, and show_chart_with_spec.
The tests move with their subject into a cfg(test) mod at the bottom of
the new file; the shared mod tests block in mod.rs keeps the one SQL test
until that module is extracted.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T8: `live_refresh.rs`

**Files:**
- Create: `crates/dat0-app/src/window/live_refresh.rs` (~306 lines + 5 test items)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 10 items):** 1008–1067 (`partition_replay_on_drift`,
`dispatch_live_refresh`) · 3525–3759 (`on_table_mutated_structural`, `active_source_path`,
`retarget_source_watch`, `on_source_changed`, `run_refresh`, `perform_reimport`,
`apply_refresh_replay`) · 8574–8584 (the `mod live_refresh_tests` declaration).

**Plus its 5 inner items** at base 8585–8672: `one_cell_edit`, `sort_on` (helpers, no `#[test]`),
`refresh_needs_confirm_only_when_rowid_ops_present`,
`schema_drift_lands_on_bare_base_when_column_missing`,
`projection_ops_referencing_missing_columns_are_not_drift`.

**Produces:**

```rust
pub use live_refresh::dispatch_live_refresh;
```

**Hazards:** `live_refresh_tests` is currently the **last** item in `window.rs`. Moving it makes it
the last item in `live_refresh.rs` — which satisfies `items-after-test-module` in the new file, but
only if nothing is appended after it later. Nothing is: T9–T15 create their own files.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Confirm the three drift tests ran:

```bash
grep -E "refresh_needs_confirm|schema_drift_lands|projection_ops_referencing" /tmp/b11-gate.log
```

- [ ] **Step 3:** Standing local gate + digest spot-check, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract live_refresh.rs

The P7c live-data surface: source watch and retarget, run_refresh,
perform_reimport, apply_refresh_replay, partition_replay_on_drift, and
dispatch_live_refresh. The live_refresh_tests module moves with it and
becomes the last item in the new file.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T9: `catalog_inspector.rs`

**Files:**
- Create: `crates/dat0-app/src/window/catalog_inspector.rs` (~483 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 13 items):** 2848–2911 (`refresh_column_view`, `inspector_projection`,
`column_name`) · 3319–3524 (`open_table_tab`, `refresh_catalog`, `catalog_nav_key`,
`toggle_catalog_parent`, `set_inspector_target`, `recompute_lineage`) · 3760–3951
(`load_inspector_profile`, `load_column_extras`, `dispatch_extra`) · 4236–4256
(`toggle_inspector_mode`).

**Hazards:** `load_column_extras` is 94 lines and the largest item here. `catalog_nav_key` is the
P6a keyboard-nav routing that `render` calls — expect it in the `E0624` list.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Confirm the catalog nav suite passed:

```bash
grep -E "catalog_nav|inspector" /tmp/b11-gate.log
```

- [ ] **Step 3:** Standing local gate + digest spot-check, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract catalog_inspector.rs

The P6a/P6b surface: catalog refresh and tree nav, table-tab open, the
inspector target and lineage recompute, column profiling (profile, extras,
dispatch), and the inspector mode toggle.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T10: `connections.rs`

**Files:**
- Create: `crates/dat0-app/src/window/connections.rs` (~356 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base range, 7 items in 1 contiguous run):** 5509–5864 — `handle_connections_event`,
`disconnect_md`, `detach_attachment`, `spawn_md_connect`, `spawn_md_test`,
`reconnect_persisted_md`, `open_md_token_prompt`.

**Hazards:** the cleanest task in the slice — one contiguous run, one topic. `spawn_md_connect` and
`spawn_md_test` touch the OS keychain via the token store; they move untouched and no test drives
the real keychain (the AI-config slice's safety trap: never drive the real keychain from a test).

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Standing local gate + digest spot-check, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract connections.rs

The P9b MotherDuck surface: connections event routing, connect/test spawns,
token prompt, disconnect, and attachment detach. One contiguous run at base,
one topic.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T11: `ai.rs`

**Files:**
- Create: `crates/dat0-app/src/window/ai.rs` (~609 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 13 items):** 5865–6462 (`settings_store`, `toggle_ai_panel`,
`hydrate_ai_panel`, `update_ai_settings`, `maybe_show_ai_privacy_banner`, `handle_ai_panel_event`,
`ai_ready`, `push_ai_ready_to_console`, `spawn_ai_test`, `spawn_ai_nl2sql`, `spawn_ai_explain`,
`open_ai_entry_prompt`) · 6652–6662 (`AiEntryKind`).

**Hazards:**
- `handle_ai_panel_event` is 126 lines, the largest method outside `render`/`ensure_dock_area`.
- `hydrate_ai_panel` probes the OS keychain and settings — B9's restore fix routes through
  `on_left_panel_shown` (T14, `dock.rs`), so after T14 the two modules call each other. Expect
  `pub(super)` in both directions; that is correct, not a smell.
- `settings_store` is a small helper used by AI and elsewhere — if T12/T13 report `E0624` on it,
  widen to `pub(super)` then.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Confirm the AI nav suite passed:

```bash
grep -E "ai_nav" /tmp/b11-gate.log
```

- [ ] **Step 3:** Standing local gate + digest spot-check, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract ai.rs

The P9c surface: AI panel toggle and hydration, settings update, privacy
banner, panel event routing, the three async spawns (test, nl2sql, explain),
the entry prompt, and the AiEntryKind enum.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T12: `sql.rs`

**Files:**
- Create: `crates/dat0-app/src/window/sql.rs` (~671 lines + 1 unit test)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 16 items):** 899–910 (`bare_table_name`) · 3110–3318 (`mount_sql_console`,
`toggle_sql_console`, `refresh_completion_snapshot`) · 4257–4376 (`on_sql_console_event`,
`cancel_sql_run`, `persist_sql_console`) · 4492–4676 (`spawn_sql_run`, `finish_sql_run`) ·
4810–4830 (`save_named_query`) · 4949–5022 (`save_console_as_table`) · 5121–5129
(`delete_named_query`) · 6663–6703 (`SqlRunOutcome`, `classify_run_err`, `format_exec_status`,
`now_unix_millis`).

**Plus the unit test** `bare_table_name_strips_quotes_and_schema` at base 8497–8512.

**Hazards:**
- **This task empties the shared `mod tests` block in `mod.rs`** (T7 took the other five items).
  After moving the last test out, **delete the now-empty `#[cfg(test)] mod tests { }`** rather than
  leaving an empty module. Verify: `grep -n "mod tests" crates/dat0-app/src/window/mod.rs` returns
  nothing.
- `finish_sql_run` (98) and `spawn_sql_run` (84) are the async run pair; `mount_sql_console` is the
  extraction B9 made. All move whole.
- `persist_sql_console` is called by `flush_focused_workspace_sql` in `boot.rs` (T2) — expect
  `E0624` and mark `pub(super)`.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Confirm the shared test module is gone and the SQL suites ran:

```bash
grep -n "mod tests" crates/dat0-app/src/window/mod.rs          # expect no output
grep -E "sql_console|sql_nav|bare_table_name" /tmp/b11-gate.log
```

- [ ] **Step 3:** Standing local gate + digest spot-check, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract sql.rs

The P5a/B8 SQL console surface: mount and toggle, completion snapshot,
console event routing, the run spawn/finish pair, cancel, persist, the
query library save/delete, save-console-as-table, and the SqlRunOutcome
classification helpers.

Takes bare_table_name and its unit test, which empties the shared cfg(test)
mod tests block in mod.rs; the now-empty module is removed rather than left
behind.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T13: `data_io.rs`

**Files:**
- Create: `crates/dat0-app/src/window/data_io.rs` (~440 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 9 items):** 3051–3109 (`open_export_dialog`, `route_export_event`) ·
4677–4770 (`run_export`) · 5023–5120 (`open_save_view_as_table`, `save_view_as_table`) ·
6463–6651 (`open_sample_kind`, `open_recent_entry`, `open_file_picker`, `route_drop_outcomes`).

**Hazards:** `route_drop_outcomes` (82) is the file-drop landing path. B10 recorded a **deliberate
coverage gap** here: `.drag_over`'s style closure runs only during a real platform drag and gpui
0.2.2 cannot simulate one, so there is no window-level drag-drop test in the tree. `on_drop` itself
is covered by `file_drop.rs` units. This move is therefore **less test-covered than any other in
the slice** — the digest is the primary evidence for it, so read its output for this task rather
than skimming.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Read the digest output in full for this task (not just the exit code).
- [ ] **Step 3:** Standing local gate, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract data_io.rs

Data in and out: the export dialog and its event route, run_export,
save-view-as-table, sample and recent-entry open, the file picker, and
route_drop_outcomes.

route_drop_outcomes carries B10's recorded coverage gap (gpui 0.2.2 cannot
simulate a platform drag, so no window-level drop test exists); the
body-digest gate is the primary evidence that it moved verbatim.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T14: `dock.rs`

**Files:**
- Create: `crates/dat0-app/src/window/dock.rs` (~727 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 34 items):** 415–441 (`INSPECTOR_DOCK_WIDTH`, `CHARTS_DOCK_WIDTH`,
`LEFT_DOCK_WIDTH`, `SQL_CONSOLE_DOCK_HEIGHT`) · 4377–4491 (`persist_dock_ui`,
`current_dock_layout`, `persist_dock_layout`, `persist_dock_layout_seed`, `window_disc`) ·
6705–7289 (`child_widget_type_name`, `hero_focus_handle`, `render_inspector_body`,
`ensure_dock_area`, `sync_left_dock`, `sync_right_dock`, `render_charts_body`, `chart_visible`,
`inspector_visible`, `sql_console_visible`, `render_catalog_body`, `render_connections_body`,
`render_ai_body`, `catalog_visible`, `connections_visible`, `ai_visible`, `rail_move_cursor`,
`rail_activate_cursor`, `rail_click`, `rail_cursor_for_test`, `set_left_panel_exclusive`,
`left_panel_visible`, `open_left_panel`, `on_left_panel_shown`, `activate_left_panel`).

**Hazards — the highest-risk task in the slice:**
- `ensure_dock_area` is 256 lines and holds B5–B8's entire dock construction, including the
  `DockItem::split` of three single-panel tabs that B7 shipped after `DockItem::tabs` proved
  unbuildable (every `add_panel` after the first re-enters `shell.read` while the shell is leased →
  panic). It moves whole and untouched. **Do not tidy it.**
- `rail_cursor_for_test` is a test accessor that lives in the *production* impl block, not the
  `cfg(a11y-capture)` block. It moves here with the rail methods, not to `test_support.rs`.
- `on_left_panel_shown` is B9's fix for the second-entry-point bug (restore seeded visibility bools
  and bypassed hydration). It is called from **two** places — the ctor in `mod.rs` and
  `activate_left_panel` here — so expect `E0624` from `mod.rs` and mark it `pub(super)`.
- The four dock-width consts are `f32` and are the only `px(` sites left in the file after B10.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Confirm the dock suites ran, all of them:

```bash
grep -E "dock_layout_persist|dock_skeleton|rail|left_dock|right_dock|bottom_dock" /tmp/b11-gate.log
```

- [ ] **Step 3:** Standing local gate + digest spot-check, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract dock.rs

The B5-B9 dock surface: ensure_dock_area, the left/right sync pair, the six
panel body renders, the visibility predicates, the B7 activity rail, and
B9's dock-layout capture and persist, plus the four dock-size consts.

ensure_dock_area moves whole and untouched, including the DockItem::split
of three single-panel tabs that B7 shipped after DockItem::tabs proved
unbuildable under gpui's entity leasing.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T15: `render.rs`

**Files:**
- Create: `crates/dat0-app/src/window/render.rs` (~779 lines)
- Modify: `crates/dat0-app/src/window/mod.rs`

**Items (base ranges, 4 items):** 7290–7443 (`render_grid_body`, `grid_focus_handle`,
`bounding_rect`) · 7445–8069 (the whole `impl Render for WorkspaceShell` block).

**Hazards:**
- `bounding_rect` is `pub(crate)` and free — check whether anything outside `src/window/` names it
  (`grep -rn "bounding_rect" crates/`); if so, add it to the `pub use` list.
- `render` is 632 lines and reads nearly every field on the shell. Expect the **largest `E0624`
  batch of the slice** here — that batch is the shell's real internal API surface, and worth a
  moment's reading rather than mechanical fixing.
- T0 probe 2 already proved a foreign-trait impl compiles from a child module. If this task
  contradicts that, stop.

- [ ] **Step 1:** Follow §C steps 1–6 with the item list above.
- [ ] **Step 2:** Record the `E0624` count for the design's as-built section:

```bash
cargo check -p dat0-app 2>&1 | grep -c "E0624"
```

- [ ] **Step 3:** Standing local gate + digest spot-check, then commit.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): extract render.rs

The Render impl and the grid body it draws: render, render_grid_body,
grid_focus_handle, and the bounding_rect helper. Trait impls are coherent
per crate rather than per module, so placement in a child module is legal
and was proven at T0.

mod.rs now holds the WorkspaceShell type, its constructor, and its grid and
view wiring, and nothing else.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task T16: ratchet, map doc, and final gates

**Files:**
- Create: `crates/dat0-app/tests/window_module_ratchet.rs`
- Modify: `crates/dat0-app/src/window/mod.rs` (module map doc)
- Modify: `docs/plans/2026-08-05-dat0-ui-redesign-b11-window-extraction-design.md` (§10 as-built)

**Interfaces — produces:** `MAX_LINES` ratchet table; `ratchet_report(&BTreeMap<String, usize>,
&BTreeMap<&str, usize>) -> String`.

- [ ] **Step 1: Write the ratchet test**

Mirrors `tests/style_lint.rs`'s two-sided ratchet, including A4's lesson that **the ratchet
arithmetic itself needs a unit test** — its over/under logic had none and was hand-probed.

Line counts differ from violation counts in one way: a violation count is meant to reach zero, a
line count fluctuates with every edit. So the under-arm needs slack, or ordinary work reddens it.

```rust
//! B11: `src/window/` cannot regrow into another 8,672-line file.
//!
//! `window.rs` reached 8,672 lines because nothing ever objected. This is the
//! objection. Each module carries an explicit ceiling; the table fails in both
//! directions, and a new file with no entry fails too, so a fresh module cannot
//! quietly become the next dumping ground.

use std::collections::BTreeMap;
use std::path::Path;

/// Per-file line ceilings for `src/window/`. Set at B11 to each file's size
/// rounded up to the next 100.
///
/// Raising an entry is a deliberate act and belongs in the same commit as the
/// code that needs it, with a reason in the commit message. If a module is
/// pushing its ceiling, the answer is usually a new module, not a bigger number.
const MAX_LINES: &[(&str, usize)] = &[
    // filled in at T16 from the real tree; see Step 2
];

/// `mod.rs` is the slice's headline promise: the master plan's B11 row asks for
/// under 5k, and it lands near 900. This asserts the promise directly rather
/// than leaving it implied by the table.
const MOD_RS_HARD_CAP: usize = 5_000;

/// Slack on the under-arm. A line ceiling is not a target to converge on, so a
/// file sitting a little under its number is normal. A file sitting *far* under
/// means the ceiling is stale and is silently holding open budget nobody is
/// using.
const UNDER_SLACK: usize = 300;

/// Pure ratchet arithmetic, extracted so it can be tested against constructed
/// inputs rather than only ever running in its silent passing state.
fn ratchet_report(
    counts: &BTreeMap<String, usize>,
    allow: &BTreeMap<&str, usize>,
) -> String {
    let mut errors = String::new();

    for (rel, found) in counts {
        match allow.get(rel.as_str()) {
            None => errors.push_str(&format!(
                "\n{rel}: {found} lines but no MAX_LINES entry.\n\
                 Add one at the next multiple of 100 above {found}. A new module \
                 without a ceiling is how the old window.rs happened.\n"
            )),
            Some(budget) if found > budget => errors.push_str(&format!(
                "\n{rel}: {found} lines, ceiling {budget} — {} over.\n\
                 Extract a module, or raise the ceiling in this same commit and \
                 say why in the message.\n",
                found - budget
            )),
            Some(_) => {}
        }
    }

    for (rel, budget) in allow {
        let found = counts.get(*rel).copied().unwrap_or(0);
        if found == 0 {
            errors.push_str(&format!(
                "\n{rel}: in MAX_LINES but not on disk. Remove the entry.\n"
            ));
        } else if budget.saturating_sub(found) > UNDER_SLACK {
            errors.push_str(&format!(
                "\n{rel}: down to {found} lines but ceiling says {budget}.\n\
                 Lower MAX_LINES[\"{rel}\"] to {} — a stale ceiling holds open \
                 budget nobody is using.\n",
                found.next_multiple_of(100)
            ));
        }
    }

    errors
}

fn count_lines(p: &Path) -> usize {
    std::fs::read_to_string(p).expect("read module").lines().count()
}

#[test]
fn window_modules_stay_within_their_ceilings() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/window");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("src/window exists") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            counts.insert(name, count_lines(&path));
        }
    }
    assert!(
        counts.len() >= 15,
        "walk found only {} modules under {} — the walk is broken",
        counts.len(),
        dir.display()
    );

    let mod_rs = counts.get("mod.rs").copied().expect("mod.rs exists");
    assert!(
        mod_rs <= MOD_RS_HARD_CAP,
        "window/mod.rs is {mod_rs} lines, over the {MOD_RS_HARD_CAP} cap B11 committed to"
    );

    let allow: BTreeMap<&str, usize> = MAX_LINES.iter().copied().collect();
    let report = ratchet_report(&counts, &allow);
    assert!(report.is_empty(), "{report}");
}

#[test]
fn ratchet_report_covers_over_under_missing_and_untabled() {
    // over budget
    let counts = BTreeMap::from([("over.rs".to_string(), 450usize)]);
    let allow = BTreeMap::from([("over.rs", 400usize)]);
    let r = ratchet_report(&counts, &allow);
    assert!(r.contains("450 lines, ceiling 400 — 50 over"), "{r}");

    // far under budget → stale ceiling
    let counts = BTreeMap::from([("under.rs".to_string(), 50usize)]);
    let allow = BTreeMap::from([("under.rs", 400usize)]);
    let r = ratchet_report(&counts, &allow);
    assert!(r.contains("Lower MAX_LINES[\"under.rs\"] to 100"), "{r}");

    // within slack → silent
    let counts = BTreeMap::from([("ok.rs".to_string(), 250usize)]);
    let allow = BTreeMap::from([("ok.rs", 400usize)]);
    assert!(ratchet_report(&counts, &allow).is_empty());

    // on disk, absent from the table
    let counts = BTreeMap::from([("new.rs".to_string(), 120usize)]);
    let allow = BTreeMap::new();
    let r = ratchet_report(&counts, &allow);
    assert!(r.contains("no MAX_LINES entry"), "{r}");

    // in the table, gone from disk
    let counts = BTreeMap::new();
    let allow = BTreeMap::from([("gone.rs", 400usize)]);
    let r = ratchet_report(&counts, &allow);
    assert!(r.contains("not on disk"), "{r}");
}
```

- [ ] **Step 2: Fill `MAX_LINES` from the real tree**

```bash
cd /Users/salar/Projects/dat0/crates/dat0-app/src/window
for f in *.rs; do
  n=$(wc -l < "$f")
  echo "    (\"$f\", $(( (n / 100 + 1) * 100 ))),   // $n"
done | sort
```

Paste the output into `MAX_LINES`. Ceilings land at the next multiple of 100 above actual, giving
every module headroom under `UNDER_SLACK` on day one.

- [ ] **Step 3: Run it, then prove it non-vacuous**

```bash
cargo test -p dat0-app --test window_module_ratchet
```

Expect 2 passing tests. Then prove the live test can fail — append 400 blank lines to
`window/connections.rs`, re-run, expect a red naming `connections.rs` and the overage; then
`git checkout` the file, **`touch` it**, and re-run to confirm green (A6's stale-binary trap).

- [ ] **Step 4: Write the `mod.rs` module map doc**

Replace the top of `mod.rs` with a short `//!` block that names every child module and one line on
what it owns. This is the orientation a reader of a 15-file directory needs and it does not exist
today. Keep it to one line per module; the detail lives in each module's own `//!`.

- [ ] **Step 5: Final digest — the slice's main evidence**

```bash
cd /Users/salar/Projects/dat0
python3 /tmp/b11_digest.py 68f01c3
```

Expected: `229 fns` before, `229 fns` after across 15 files, `DIGEST OK`, exit 0. **Zero `MISSING`,
zero `ADDED`, zero `CHANGED`.** Any output at all here is a finding, not a formality — record it
verbatim in §10 whatever it says.

- [ ] **Step 6: Full feature matrix**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
for feat in "" "--features a11y-capture" "--features a11y-capture,gallery"; do
  cargo test -p dat0-app $feat > "/tmp/b11-final${feat// /}.log" 2>&1
  grep -c "^test result: ok" "/tmp/b11-final${feat// /}.log"
done
```

Expect **119** per combination — 118 at B10 plus `window_module_ratchet`.

- [ ] **Step 7: Seeded boot check**

A fresh-session boot exercises **none** of the dock restore path (B9), and this slice moved
`ensure_dock_area`, the persist methods and `run_app`. So seed the layout or the check is vacuous.

```bash
cd /Users/salar/Projects/dat0
cargo build --bin dat0
export DAT0_CONFIG_DIR=/tmp/b11-boot
mkdir -p "$DAT0_CONFIG_DIR"
cat > "$DAT0_CONFIG_DIR/settings.toml" <<'EOF'
[ui]
left_panel = "catalog"

[ui.dock_layout]
left_size = 384
right_size = 288
bottom_size = 320
EOF
./target/debug/dat0 > /tmp/b11-boot-branch.log 2>&1 &
sleep 8; kill %1
```

Match the seed's exact shape against `src/session/dock_layout.rs` and `Settings`/`UiSettings`
(schema v3) before running — the block above is the intended shape, and any mismatch in key names
must be corrected from the source, not guessed. Then build `main` the same way into a second log and
diff, normalising timestamps, UUIDs, durations and the config path. Expect an identical log.

Run the second arm too (`left_panel = "ai"`), which is B9's riskiest: its hydration fix puts a
`tokio::spawn` in the first render that no test can clear.

- [ ] **Step 8: Confirm no call site outside `src/window/` changed**

```bash
git diff --stat 68f01c3..HEAD -- . ':(exclude)crates/dat0-app/src/window' \
                                  ':(exclude)docs' \
                                  ':(exclude)crates/dat0-app/tests/window_module_ratchet.rs'
```

Expected: **empty**. Anything listed means the `pub use` re-export set is wrong and the slice broke
its central promise.

- [ ] **Step 9: Write design §10 as-built and commit**

Record: final per-module line counts; `mod.rs`'s actual size; the total `E0624` count and which
methods needed `pub(super)` (this is the shell's real internal API surface, measured for the first
time); the digest result verbatim; the boot-log diff result; anything the plan got wrong.

```bash
git add -A
git commit -s -F - <<'EOF'
refactor(window): line ratchet, module map, and B11 as-built

Adds tests/window_module_ratchet.rs: a per-file line ceiling for every
module under src/window/, failing when a file grows past its ceiling, when
a ceiling is left stale far above its file, when a module on disk has no
entry, and when an entry names a file that is gone. window.rs reached 8,672
lines because nothing ever objected; this is the objection. The ratchet
arithmetic carries its own unit test, per the A4 lesson that the gate logic
needs testing rather than only ever running in its silent passing state.

mod.rs gains a module map naming each child and what it owns.

Design doc section 10 records the as-built: final module sizes, the E0624
inventory of methods that needed pub(super), the body-digest result, and
the seeded dock-layout boot check against a main build.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
```

---

## Self-review

**Spec coverage.** Design §3's 14 modules → T2–T15 (one each) plus T1 for `mod.rs`. §4's two
hazards → T2 (URL consts) and T5 (clippy ordering). §5.1 standing suite → every task's gate. §5.2
digest → §B, T0 step 6, every task's step, T16 step 5. §5.3 boot check → T16 step 7. §5.4 ratchet →
T16 steps 1–3. §6's four probes → T0 steps 2–6. §7's commit order → T1–T16 in that order. §9 owed
glance → nothing to do in this slice, correctly absent from the tasks.

**Placeholder scan.** T1 step 3 is explicitly a deliberate no-op with its reason stated, not a TBD.
`MAX_LINES` is empty in the source block and filled by T16 step 2, which gives the exact command
producing the content — the values cannot be known before the modules exist. Design §10 is the
house convention for an as-built section filled during execution.

**Type consistency.** `ratchet_report` takes `(&BTreeMap<String, usize>, &BTreeMap<&str, usize>)`
and returns `String` in both the implementation and its unit test. `count_lines` takes `&Path`.
`MOD_RS_HARD_CAP` and `UNDER_SLACK` are `usize`, matching the counts they compare against.
`next_multiple_of` is stable since Rust 1.73 and the toolchain is pinned at 1.97.0.

**One known gap, stated rather than hidden.** The base line ranges quoted in T2–T15 are valid only
at `68f01c3` and go stale as tasks execute. §C step 1 addresses this by regenerating the manifest
per task and locating items by name; the quoted ranges are for orientation and item-count
cross-checking. An implementer who cuts by stale line number will produce garbage, which is why
this is called out here, in §C, and in the global constraints.
