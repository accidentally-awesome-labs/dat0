# dat0 P7a — Workspace core (design)

> **P7 split (set here):** P7 (Workspace mode, spec §P7) is sliced 3 ways, mirroring
> the P5/P6 cadence:
> - **P7a** (this doc) = **workspace core**: folder picker, `.dat0/` layout, scratch→workspace
>   promotion, recent-workspaces menu, single-machine `flock` lock + focus-existing.
> - **P7b** = concurrency & sync-drive safety: hybrid JSON manifest-lock, sync-drive heuristic +
>   "Treat as networked" override, rich `WorkspaceInUseModal` (cross-machine), Settings → Workspace.
> - **P7c** = live data: file-watcher on workspace source files, replay-on-change, refresh UX,
>   crash-recovery polish.
>
> Brainstormed 2026-06-10 via `superpowers:brainstorming`. Slicing + all four foundation
> decisions confirmed with the user.

## Goal

Give the **already-durable, already-file-backed** scratch session a **located, named home** — a
user-chosen folder containing a git-style `.dat0/` metadata subdir — plus a **lossless
scratch→workspace promotion** path, a **recent-workspaces** menu, and a **single-machine
advisory lock** so two windows can't open the same workspace DB. This is the spine the rest of
P7 builds on (P7b adds cross-machine/sync-drive safety; P7c keeps sources live).

**Framing fact that shrinks the phase:** scratch is *not* in-memory. Each window already lives at
`state_root/scratch/{uuid}/` holding a real `scratch.duckdb` (`session/mod.rs:501` —
`DuckDBEngine::new` → `duckdb::Connection::open`) plus a `session.json` (schema v8, robust v1→v8
migration chain). The app's file-open path materializes dropped files into the DB
(`file_drop.rs:1` → `register_file_as_table` → CTAS base table). So **the data is already on
disk and self-contained**; promotion is fundamentally *move the DB + session.json into `.dat0/`*,
not *serialize from memory*.

## Decisions (locked in brainstorm)

| # | Decision | Over | Why |
|---|----------|------|-----|
| D1 | **Folder + `.dat0/` subdir** (git-style); workspace == the chosen folder | app-managed library / named bundle anywhere | Matches spec text "Open folder → `.dat0/` created"; P7c watcher's "workspace source files" sit alongside `.dat0/`. Distinct from P8b's portable `.dat0` *package file*. |
| D2 | **Promotion = move `scratch.duckdb` + `session.json` into `.dat0/`; external refs stay external** | copy sources in + relativize / materialize all to base tables | Materialized tables (app drop path = CTAS) come along self-contained. ATTACH refs (sqlite/MotherDuck) re-attach from `session.json`. Source-copy/relativize is P7c-flavored and mostly moot (tables already materialized). |
| D3 | **`flock` + focus-existing** (single-machine) | no lock in P7a / full hybrid now | Safe same-machine guarantee with the existing `fs4` pattern (`app_lock.rs`). Cross-machine/sync-drive/`WorkspaceInUseModal` → P7b. |
| D4 | **Separate `manifest.json`** for workspace identity | fold into `session.json` (v8→v9) | Splits durable workspace identity from volatile window/view state; keeps the v1→v8 migration chain untouched; this manifest is what P8a will ratify. |
| D5 | **`Home` enum on `Session`** | `Workspace` wraps `Session` / parallel type + trait | Smallest diff: `recover()`/`persist()` already centralize on one dir; we generalize that dir's *meaning* and add lock/manifest siblings. Promotion mutates `home` in place. |
| D6 | **No session-schema bump** (`session.json` stays v8) | v8→v9 | All new state lives in the separate manifest. Promotion reuses `session.json` verbatim → lossless by construction, zero migration risk. |

## Grounded facts (verified live, 2026-06-10)

- **Scratch is file-backed + durable.** `Session { window_id, scratch_dir, engine, tabs, … }`;
  `build_engine` opens `scratch_dir/scratch.duckdb` (`session/mod.rs:501`). `Session::new`
  (UUID-v7 dir) + `Session::recover` already branch on a single dir and centralize persistence.
- **App import path materializes.** `file_drop.rs:1` uses `register_file_as_table` → CTAS base
  table (`duckdb_engine.rs:230`). The view-over-file path (`register_file` →
  `CREATE OR REPLACE VIEW … read_csv(path)`, `duckdb_engine.rs:159`) exists but is not the drop
  path → dropped data lives **in** the DB, not as an external-path view.
- **Recents foundation exists.** `recents/mod.rs` — `Recents` (MRU, max 25, JSON at
  `cfg_dir/recents.json`) already types `RecentEntry::Workspace { path }` / `Package { path }`.
- **Menu actions stubbed.** `OpenWorkspace` / `OpenPackage` are declared GPUI actions
  (`menu_macos.rs:100-101`) wired to menu items (`:28-29`) — **no handlers yet**.
- **Native folder picker available, no new dep.** `cx.prompt_for_new_path` is already used for the
  export save panel (`window.rs:2097`); `cx.prompt_for_paths` is the open-side sibling.
- **`flock` available + patterned.** `fs4` is already a workspace dep (`Cargo.toml:36`, in
  `dat0-app`); `app_lock.rs:39-55` already holds an advisory PID lock via
  `fs4::fs_std::FileExt::try_lock_exclusive`. Workspace lock mirrors it.
- **Engine close exists.** `DuckDBEngine::close()` (async, `duckdb_engine.rs:133`) sets
  `EngineStatus::Closed` — supports the close→move→reopen handoff.
- **Recovery scaffolding exists.** `recovery_panel.rs` has `load_for_open` / `discard` (UI stub);
  `window_registry.rs` (process-shared) maps live windows; `scan_orphans` (`session/mod.rs`)
  finds orphaned scratch dirs.

## On-disk layout

The workspace **is** the user-picked folder; dat0 owns a hidden `.dat0/` subdir inside it:

```
<user-folder>/                 # workspace == this folder
├── sales.csv                  # user's own files live alongside (optional)
└── .dat0/                     # dat0-managed (hidden, git-style)
    ├── manifest.json          # workspace identity (NEW, P7a)
    ├── workspace.duckdb        # the DB, moved from scratch.duckdb
    ├── workspace.duckdb.wal    # DuckDB-managed sibling
    ├── session.json            # window/view state — schema v8, UNCHANGED
    ├── lock                    # advisory flock target, held for session lifetime
    └── lineage/                # reserved empty dir (spec layout contract; P6 lineage is runtime-derived)
```

`lineage/` is **reserved-but-empty**: the spec's layout contract promises it, but P6 lineage is
derived from SQL ASTs at query time (`referenced_tables` / `json_serialize_sql`), so there is
nothing to persist yet. Creating it honors the contract and keeps P8a's format spec stable.

This is **distinct from P8b's portable `.dat0` package file** — different artifact, different
phase. The working format (P7) is an open directory; the package format (P8b) is a zip bundle.

## `manifest.json` (new, separate from `session.json`)

Pure workspace identity:

```json
{
  "format_version": 1,
  "dat0_version": "0.1.0",
  "workspace_id": "<uuid-v7>",
  "created_at": "<rfc3339>",
  "modified_at": "<rfc3339>"
}
```

- **Attachments stay in `session.json`** for P7a — already persisted there + re-attached on
  recover (P5c). Zero churn. P8a may absorb them into the manifest when it ratifies the schema.
- `modified_at` is bumped on `persist()` for a workspace home (cheap); not load-bearing in P7a.
- New module `workspace/manifest.rs`: `Manifest` struct + `read` / `write` (atomic-rename, same
  pattern as `session::persist`).

## `Home` enum (generalizes `Session`)

`Session.scratch_dir: PathBuf` → `Session.home: Home`:

```rust
pub enum Home {
    Scratch   { dir: PathBuf },                  // state_root/scratch/{uuid}/
    Workspace { root: PathBuf, dat0: PathBuf },  // root = user folder, dat0 = root/.dat0
}

impl Home {
    fn db_path(&self) -> PathBuf;        // scratch.duckdb | workspace.duckdb
    fn session_json(&self) -> PathBuf;   // <home>/session.json
    fn lock_path(&self) -> Option<PathBuf>;     // None | Some(dat0/lock)
    fn manifest_path(&self) -> Option<PathBuf>; // None | Some(dat0/manifest.json)
    fn is_workspace(&self) -> bool;
}
```

- `build_engine`, `Session::new`, `Session::recover`, `persist` branch through `home`.
- A workspace-backed `Session` also holds a live **flock guard** (open `File` on `dat0/lock` with
  `try_lock_exclusive`, kept for the session lifetime; auto-released on drop / process exit).
- `window_id` (UUID) is retained for both variants: scratch parses it from the dir name; workspace
  reads it from `manifest.workspace_id`.

## Promotion (scratch → workspace)

Triggered by "Save Workspace". **Failure-safe ordering** — scratch stays intact until the last
step, so any crash degrades to "scratch still there" (orphan-recovery handles it):

```
promote(session, target_folder):
  1. dat0 = target_folder/.dat0
  2. GUARD: if dat0 exists → refuse, banner "Folder already a workspace — open it instead". Abort.
  3. create dat0/ + dat0/lineage/
  4. acquire flock(dat0/lock)              — fail → banner, abort (scratch untouched)
  5. session.persist()                      — flush latest session.json to scratch
  6. engine.close()                          — clean DuckDB shutdown (WAL flush) BEFORE moving file
  7. move scratch.duckdb → dat0/workspace.duckdb   — fs::rename; EXDEV → copy+remove
  8. move session.json → dat0/session.json   — verbatim, v8
  9. write dat0/manifest.json                — new workspace_id (uuid-v7), timestamps
 10. session.home = Workspace{root,dat0}; rebuild engine on workspace.duckdb; re-attach attachments
 11. recents.push(Workspace{path: target_folder}); rebuild File→Open Recent menu
 12. remove old scratch dir                  — LAST + idempotent
 13. toast "Saved workspace to <folder>"
```

**Delicate mechanic (steps 6–10):** moving an open DuckDB file is unsafe, so close (WAL flushes on
connection drop) → fs-move → reopen bound to the new path. **EXDEV** (cross-volume rename, e.g.
scratch on the app-data disk + target on an external drive) must fall back to copy-file + remove.

Promotion is **lossless by construction**: `session.json` (tabs, transform stacks, SQL tabs,
history, saved queries, attachments, dock state) is *moved*, not re-serialized.

## Open workspace + locking

`File → Open Workspace…` (action already declared at `menu_macos.rs:100`) → folder picker
(`cx.prompt_for_paths { directories: true, multiple: false }`):

```
open_workspace(folder):
  1. dat0 = folder/.dat0; missing → banner "Not a dat0 workspace". Abort.
  2. window_registry: already open in THIS process? → focus that window. Done.
  3. try flock(dat0/lock):
       success → Session::recover_workspace(dat0) → new window;
                 register window↔workspace in window_registry; recents.push
       held    → P7a (single-machine): banner "This workspace is open in another dat0 window."
                 (Rich WorkspaceInUseModal + cross-machine + sync-drive override = P7b.)
```

- `flock` is advisory + **auto-released on process death** → a lock left by a crashed process
  self-heals; the next open reacquires cleanly. **No force-unlock** (force-unlock stays v1.x per
  spec).
- `Session::recover_workspace(dat0)` mirrors `Session::recover` but reads `home` as a workspace,
  reads `manifest.workspace_id` for `window_id`, and holds the flock guard.

## Recent-workspaces menu

`Recents` already exists; P7a is wiring only:

- Push `RecentEntry::Workspace { path }` on promote + open.
- Populate a `File → Open Recent` submenu from `recents.list()`.
- Rebuild the macOS menu (`set_menus`) when the list changes (menu is static today).
- Clicking an entry runs `open_workspace` (which focus-existing's if already open in-process).

## Promotion trigger UX

- **Explicit:** "Save Workspace" — File menu + command palette. Available while the window is
  scratch-backed; no-op/hidden once it's a workspace.
- **Gentle prompt:** once per scratch session, after **≥3 transforms applied** (across tabs) **OR
  ≥1 saved query**, surface a dismissible `error_ux::Banner`: *"Save this as a workspace to keep
  it? [Save Workspace] [Not now]"*. Non-blocking. "Already prompted" is in-memory only (shows at
  most once per session, never nags).

## Error / edge handling

- **Target already a workspace** (`.dat0/` exists) → refuse promote; offer "Open it instead."
- **Folder not writable** → flock/create fails early → banner; scratch untouched.
- **Cross-volume move** → `fs::rename` → `EXDEV` → explicit copy-file + remove fallback.
- **Crash mid-promotion** → partial `.dat0/` may exist. On open, validate `manifest.json` +
  `workspace.duckdb` both present; incomplete → banner "Workspace looks incomplete (interrupted
  save)" + offer discard. Scratch dir still present (removed last) → recover from it. No data loss.
- **Stale lock from crashed process** → flock auto-released on death → next open reacquires.
- **Workspace opened with a missing external ATTACH** (sqlite path gone / MotherDuck offline) →
  existing P5c re-attach error path surfaces a non-fatal banner; workspace still opens.

## Testing

- **Unit (no GPUI):** `Home` path resolution (db/session/lock/manifest for both variants);
  `Manifest` read/write round-trip; promotion move-logic incl. simulated `EXDEV` copy path; flock
  acquire/release + contention (two handles → second `try_lock` fails); `recover_workspace` from a
  built `.dat0/` fixture; incomplete-workspace detection.
- **Integration (real engine):** promote a scratch holding a **materialized** table → reopen
  workspace → rows + tabs + transform stacks + saved queries intact (the *lossless* exit
  criterion); open same workspace twice → second blocked; ATTACH re-attach on workspace open.
- **Exit criteria covered (P7a subset of spec §P7):** `.dat0/` created with manifest + lock + DB +
  lineage dir; promotion lossless; recent-workspaces populates + click opens-new / focuses-existing;
  force-quit during workspace work → next launch recovers from `.dat0/`.

## T0 spikes (GATE before building)

1. **`cx.prompt_for_paths` directory selection** — confirm `PathPromptOptions { directories: true,
   multiple: false }` returns a folder on macOS. Low risk (`prompt_for_new_path` already used for
   export); verify the directories flag + the oneshot-receiver await pattern.
2. **DuckDB close→move→reopen** — confirm `engine.close()` fully releases the file handle so
   `fs::rename(workspace.duckdb)` succeeds on macOS, and reopen on the new path sees all data with
   the WAL flushed (incl. the `.wal` sibling). **The delicate mechanic — gate it first.**
3. **`fs4` workspace flock** (reuse `app_lock.rs` pattern) — confirm `try_lock_exclusive` on a held
   lock file fails (not blocks) and auto-releases on process exit. Dep already present; confirm
   semantics on the workspace `lock` path.

## Non-goals (explicitly out — deferred)

| Item | Target |
|------|--------|
| Hybrid JSON manifest-lock for sync drives; sync-drive heuristic + "Treat as networked" override | **P7b** |
| Rich `WorkspaceInUseModal` w/ cross-machine warning (P7a = minimal banner + same-machine focus) | **P7b** |
| Settings → Workspace section | **P7b** |
| File-watcher on source files + replay-on-change + refresh UX | **P7c** |
| Force-unlock | **v1.x** (per spec) |
| `.dat0` portable package file | **P8b** (different artifact) |
| Copy/relativize external source files into the workspace | P7c / v1.x (mostly moot — tables materialized) |

## Forward-compat note

Every deferral maps to a later slice's primitive (manifest-lock → P7b, watcher → P7c, package →
P8b). P7a's `.dat0/` layout is forward-compatible by design: P7b/P7c/P8a **add** files/fields,
never reshape what P7a writes. The separate `manifest.json` is the seam P8a will ratify.

## Deferrals touched

- **Opens D-019..? (P7b/P7c register entries)** — created at execution time per the deferral
  protocol as the non-goals above are formally scheduled. (Note: the D-register has an intentional
  gap at D-016/D-017 — see P6b retro — so the next id is whatever the register's tail is at
  execution; do not assume D-019.)
- No existing deferral is closed by P7a (D-018 workspace-DAG is a P6b lineage deferral, unrelated).

## Decisions register (for the retro)

- **CHOSE** folder + `.dat0/` subdir **OVER** app-managed library / named bundle — spec-aligned,
  P7c-watcher-aligned, P8b-distinct.
- **CHOSE** move-DB promotion w/ external refs intact **OVER** source-copy/relativize — data already
  materialized; lossless via file-move; least work.
- **CHOSE** `flock` + focus-existing **OVER** no-lock / full-hybrid — safe single-machine now, reuses
  `app_lock.rs` pattern; cross-machine → P7b.
- **CHOSE** separate `manifest.json` **OVER** session.json v8→v9 — identity vs view-state split; no
  migration; P8a-ratifiable seam.
- **CHOSE** `Home` enum on `Session` **OVER** wrapper / parallel-type+trait — smallest diff; reuses
  centralized persistence; in-place promotion.
- **CHOSE** no session bump **OVER** v8→v9 — new state is manifest-only.
