# P5c — SQL Console: MotherDuck end-to-end + Connections panel (design)

> Date: 2026-06-04
> Phase: P5c (closing slice of the three-way P5 SQL-Console split: P5a editor/run/cancel/multi-tab → P5b autocomplete/history/saved-queries/Save-as-Table → **P5c MotherDuck ATTACH + Connections panel**)
> Status: design approved, plan pending
> Spec ref: `docs/specs/2026-04-26-dat0-design.md` §6.5 (ATTACH), §8.3 (SQL Console); `docs/specs/2026-04-27-dat0-p2-engine-design.md` §7
> Predecessor: `docs/plans/2026-06-04-dat0-p5b-design.md` (merged PR #11 squash `102b91d`)
> Closes: **D-007** (MotherDuck ATTACH end-to-end) — full, unless the trim valve fires (then partial)

P5c delivers the credential-gated MotherDuck slice of P5 plus a general **Connections panel**:
runtime `motherduck` extension load, a keychain-backed `MotherDuckTokenStore`, end-to-end
`ATTACH 'md:'` against a real MotherDuck account, per-workspace persistence + background
auto-reconnect, a routing-tagged timing chip, CI-required integration tests, and a toast for the
P5b grid Save-as-Table failure backlog item. The user chose the **heavier** option at each fork
(general attachments manager, multi-feature whole slice), offset by a strict **trim valve**.

---

## §0. Locked decisions

| # | Decision | Rationale |
|---|----------|-----------|
| **D1** | **Connect UX = a Connections panel** (left dock, toggleable), not a palette-only command. | User choice (Q1). No connect surface exists today — `engine.attach()` is engine-only, test-exercised. The panel is the durable home for attachment state. |
| **D2** | **General attachments manager**: panel manages all attachments (one md + many sqlite), each with alias/status/detach + an `Attach SQLite…` add-flow. | User choice (Q2). Bigger than an md-only panel; the **trim valve** (§9) protects the slice. |
| **D3** | Integration tests read `MOTHERDUCK_TOKEN` from env and are **required in CI** — CI hard-fails if the secret is absent. | User choice (Q3). Strongest end-to-end guarantee, the whole point of D-007. Requires wiring a GitHub Actions secret **before** this lands. Forked-PR runs get no secrets — acceptable for a solo repo (§7). |
| **D4** | Timing chip = **routing-tagged elapsed**: `⏱ N ms · md` / `· local` / `· mixed`, flipping P5b's reserved `· local` suffix by which catalog the SQL referenced. | User choice (Q4). A single query runs once against one catalog, so a literal "local 38 / md 412" side-by-side isn't meaningful. Routing detection is spike-gated with a documented string-match fallback (§6). |
| **D5** | **Single MotherDuck account.** One token machine-wide under a fixed-but-labelable keychain key. Many sqlite attachments allowed. | User choice (Q6). One token already exposes every database in the account. Multi-account = additive later, no migration (key is a label string). |
| **D6** | **Persist token + auto-reconnect, scoped per-workspace.** Token in keychain (machine-global, set once). Attachment recorded per-workspace in `session.json` (v6→**v7**). On launch, only workspaces that had md attached reconnect — in the background, non-blocking. | User choice (Q5) + the per-workspace engine model (§1). "Reduce unnecessary processes": never load the extension or touch the network for non-md workspaces. |
| **D7** | **Architecture A** — engine is pure mechanism (token passed in, never stored by engine); the app owns a `ConnectionManager` (keychain store, panel, reconnect, routing tag). | User choice (Approach A). Keeps the engine testable without keychain/UI and matches how `sqlite_scanner` + `AttachOpts` already work. |
| **D8** | **Lazy extension load**: `INSTALL motherduck; LOAD motherduck;` runs on **first connect**, memoized once per process — not at app boot. | No boot cost for users who never touch MotherDuck. Same `OnceLock` memoization shape as `sqlite_scanner`, but deferred to first use rather than boot. |
| **D9** | Include the **P5b grid Save-as-Table toast** backlog item: surface that failure via the existing `banner::push` primitive. | User confirmed include. Small; reuses the PD-007 banner queue rather than new UI. |

---

## §1. Grounded API facts (verified against live code, not recalled)

Confirmed this session by reading source — load-bearing for the design.

- **`duckdb-rs` is `1.4.4`** (`Cargo.lock`). D-007's original blocker was the absence of a `motherduck`
  *Cargo feature*; P5c bets on **runtime `INSTALL motherduck`** from the extension repository (the same
  mechanism `sqlite_scanner` already uses), which does not require a Cargo feature. **Spike S1 gates this.**
- **Per-workspace engine.** Each open window is a `WorkspaceShell` bound to one `Arc<Session>`;
  `session.lock().engine` is a single DuckDB connection per workspace (`file_drop.rs:115`). ATTACH lives
  in that connection. Multiple workspaces/windows can be open, each independent.
- **Workspace = state dir + `session.json` + engine.** `WorkspaceShell::new(Arc<Session>, cx)`
  (`window.rs:107`); workspaces keyed by canonical `PathBuf` (`window_registry.rs:173`); `session.json`
  lives per state dir (`recovery_panel.rs:65`, `window.rs:137`). `OpenWorkspace` menu action exists.
- **Keychain crate ready** (`crates/dat0-keychain/src/lib.rs`): `Keychain::new(service) -> Result<Self>`,
  `.set(key, &[u8])`, `.get(key) -> Option<Vec<u8>>`, `.delete(key)`. A `#[cfg]` **no-op variant** exists
  for test/headless backends (lib.rs:135) — usable as the test backend.
- **Engine `attach()`** (`duckdb_engine.rs:531`): parses scheme via `attach::parse_scheme`; `Sqlite` arm
  builds SQL with `build_attach_sqlite_sql(rest, alias, &opts)` and runs it on `spawn_blocking` against the
  `Mutex`'d `conn`; **`MotherDuck` arm returns `EngineError::NotImplemented { feature: "MotherDuck" }`**
  (lines 535–540) — this is the implementation site. `detach()` is already generic (`build_detach_sql`).
- **`AttachOpts`** (`types.rs:149`): `{ read_only: bool, schema_filter: Option<Vec<String>> }`. P5c adds a
  `token: Option<String>` field, **excluded from `Debug`** (token must never reach logs/spans).
- **Extension bootstrap pattern** (`extension_bootstrap.rs`): `install_sqlite_scanner_at_app_boot(scratch)`
  memoizes via `static INSTALL_RESULT: OnceLock` running `INSTALL …; LOAD …;` once; a
  `__test_install_sqlite_scanner()` variant exists for tests. P5c mirrors this for `motherduck`.
- **`attach()` `#[instrument]`** currently logs only `dsn_scheme` + `alias` (line 530) — keep the token out
  of every span/field.
- **Shell layout** (`window.rs:2558`): `div().flex().flex_col()` of `tab_strip → pipeline_bar →
  sql_console_panel → body(flex_1) → overlays`. The Connections panel inserts as a left column wrapping the
  body row (§4). The `sql_console_panel` toggle pattern (`sql_console_visible` + `SqlConsoleToggle` action,
  view-scoped `on_action`) is the template for `ConnectionsToggle`.
- **Banner primitive**: `banner::push` / `banner::drain_pending` queue (PD-007, P2 `9ea964b`) — the toast
  reuse target.
- **Timing chip**: P5b ships `⏱ N ms · local`; `started_at`/elapsed already tracked. P5c flips the suffix.

---

## §2. Architecture (Approach A)

```
dat0-engine (mechanism — no keychain, no UI, token passed in)
  extension_bootstrap.rs
    + install_motherduck_at_app_boot(scratch)   // OnceLock memo, INSTALL motherduck; LOAD motherduck;
    + __test_install_motherduck()
  duckdb_engine.rs::attach()  MotherDuck arm:
    SET motherduck_token = <opts.token>;  ATTACH 'md:' AS {alias};   // on spawn_blocking
  types.rs::AttachOpts { read_only, schema_filter, token: Option<String> /* not Debug */ }
  error.rs  + typed variants: MotherDuckAuth / ExtensionLoad / (network surfaced as generic)

dat0-app (policy — owns lifecycle, keychain, UI, persistence)
  connections/
    mod.rs        ConnectionManager { attachments: Vec<Attachment>, … } per-workspace
    token_store.rs MotherDuckTokenStore over Keychain::new("dat0.motherduck"), key "token"
    panel.rs      Connections panel view (left dock)
  boot.rs         + background reconnect step for persisted md attachments
  session (v6→v7) + attachments: Vec<PersistedAttachment>   // NO token here
  query/timing    routing tag classifier (local/md/mixed)
  banner          Save-as-Table failure toast (existing primitive)
```

**Connect flow** (all async, off UI thread):
`install_motherduck` (memoized) → `token_store.get_token()` → `engine.attach("md:", "md",
AttachOpts{ token: Some(t), .. })` → verify (cheap `SELECT`/list) → `ConnectionManager` sets status →
panel re-renders. Failure → typed `EngineError` → panel `Error(msg)`.

---

## §3. Engine layer (mechanism only)

- **`install_motherduck_at_app_boot(scratch)`** — `OnceLock<Result<(),String>>`, runs
  `INSTALL motherduck; LOAD motherduck;` once/process; subsequent calls only LOAD (best-effort), mirroring
  `sqlite_scanner`. **Called lazily on first connect** (D8), not from `boot.rs`. `__test_install_motherduck()`
  test variant.
- **`attach()` MotherDuck arm** — replace the `NotImplemented` early-return. On the existing
  `spawn_blocking` against the locked `conn`: `conn.execute_batch("SET motherduck_token = '…'; ATTACH 'md:'
  AS {alias};")` with the token from `opts.token` and the identifier quoted. If `opts.token` is `None` →
  `EngineError::MotherDuckAuth` (caller must supply). Token interpolation uses DuckDB string-literal escaping;
  it is **never** logged.
- **Typed errors** — add `EngineError::MotherDuckAuth` (bad/expired/absent token) and
  `EngineError::ExtensionLoad { name }` (install/load failed). Network/transient failures surface through the
  existing generic DuckDB error path with a message; the app renders them as the panel `Error` state.
- **Token redaction** — `AttachOpts.token` excluded from `#[derive(Debug)]` (manual `Debug` or
  `#[debug(skip)]` equiv); `attach()`'s `#[instrument]` keeps only `dsn_scheme`/`alias`.
- **`detach()`** — unchanged; reused for both md and sqlite.

---

## §4. Connections panel (`connections/panel.rs`)

**Placement.** The shell body row becomes `flex_row` of `[connections_panel?][body flex_1]`. Panel is a
fixed-width (~`w_64`) left column with a right border, hidden by default, toggled by a new
`ConnectionsToggle` menu action + command-palette descriptor + key — modeled on `sql_console_visible` /
`SqlConsoleToggle` (view-scoped `on_action` on the shell root).

**Sections.**
1. **MotherDuck** — status pill: `Disconnected` / `Connecting…` / `Connected as md` / `Error`. Buttons by
   state:
   - Disconnected → **Connect** (opens token modal if no stored token; else one-click using the stored token).
   - Connected → **Disconnect** (`engine.detach("md")` + status reset), **Forget token** (`token_store.forget` +
     detach).
   - Error → **Retry** + the localized error text.
2. **Attached files** — list of sqlite `Attachment`s (`alias · path · status`), each with **Detach**; an
   **Attach SQLite…** button → native file picker → `engine.attach("sqlite:<path>", alias, opts)`. This is the
   general-manager surface (D2).
3. **Shallow catalog enumeration** *(trim-valve item ① — §9)* — under a Connected entry, list **database
   names** via `PRAGMA database_list` / `duckdb_databases()`. **Names only — no per-table `TableOrigin`
   recording** (D-012 stays deferred). Drops first if spikes/effort run hot.

**Token modal.** Reuse the `name_prompt`/overlay pattern (single masked field + Connect/Cancel). Submitting
stores via `token_store.set_token` then runs the connect flow. The raw token is held only for the duration of
the connect call; never persisted outside keychain.

---

## §5. Persistence + boot auto-reconnect

**`session.json` v6 → v7** — additive: `attachments: Vec<PersistedAttachment>` where
`PersistedAttachment { alias, kind: Md | Sqlite { path } }`. **No token, ever** — only the *fact* that md was
attached. v6 sessions load as v7 with empty `attachments` (forward-compatible default). Reuses the proven
migration ladder + atomic persist (same path as P5b's v5→v6).

**Boot reconnect (`boot.rs`).** On workspace launch, for each persisted `Md` attachment, spawn a background
task: `install_motherduck` → `token_store.get_token()` → `attach` → verify; panel shows
`Connecting…→Connected/Error`, **non-blocking** (window opens immediately). Workspaces with no md attachment
**never** load the extension or touch the network. Persisted `Sqlite` attachments reconnect by re-running
ATTACH on their stored path (best-effort; missing file → `Error` status, non-fatal).

---

## §6. Timing chip — routing tag (D4)

After a console run, classify which catalog(s) the executed SQL touched and render
`⏱ {elapsed} ms · {local|md|mixed}`.

- **Preferred detection (spike S2):** inspect the executed plan / referenced catalogs (e.g. via
  `duckdb_databases()` cross-referenced with the bound plan) to know whether `md` and/or the local scratch
  catalog were read.
- **Fallback (if S2 unreliable):** a pure string classifier — if the SQL contains a qualified `md.` reference
  → tag `md` (or `mixed` if it also references local tables), else `local`. **Documented limitation:** misses
  md tables referenced unqualified via a `USE md` default-catalog switch. Acceptable for the chip; recorded in
  the spec and as a PD-xxx if the fallback ships.

The classifier is a **pure unit** over `(sql, attached_aliases)` → `{local,md,mixed}`, unit-tested
independently of the engine.

---

## §7. Testing

**Engine unit (no network).** `parse_scheme` md; `install_motherduck` memoization; md-arm builds the correct
`SET motherduck_token` + `ATTACH 'md:' AS md` SQL with proper quoting/escaping; `AttachOpts.token` absent from
`Debug` output and from the `attach` span; `MotherDuckAuth` when token is `None`; DETACH.

**Integration — CI-required (D3).** Read `MOTHERDUCK_TOKEN` (or `motherduck_token`) from env. Live path:
`install_motherduck` → `attach 'md:'` → `SELECT` from a known md table/`md` system view → assert rows →
`detach`. **CI hard-fails if the secret is missing.** Prerequisites:
- Wire a **`MOTHERDUCK_TOKEN` GitHub Actions secret** into the workflow (heavy.yml and/or ci.yml) **before
  this lands**.
- **Redaction check:** assert the token never appears in test output / `--nocapture` / tracing.
- **Forked-PR caveat:** secrets are absent on fork PRs → those runs fail the integration job. Acceptable for a
  solo repo; documented. (If this bites, the fallback is `skip-if-absent` — but D3 chose required.)

**App.** `ConnectionManager` state transitions (Disconnected↔Connecting↔Connected↔Error);
`MotherDuckTokenStore` set/get/forget round-trip against the `#[cfg]` no-op keychain backend; session v7
round-trip + v6→v7 migration; routing classifier (local/md/mixed) over sample SQL; Save-as-Table failure →
`banner::push` fires.

**Manual UAT.** Append to the owed-UAT backlog (carries P4b/P4c/P5a/P5b): connect → query md → restart →
auto-reconnect → disconnect → forget → bad-token error → sqlite attach/detach → panel toggle → routing chip
shows `· md`/`· local`/`· mixed`.

---

## §8. Error surfacing

- **Connect failures** render in the panel's `Error(msg)` state (typed `EngineError` → localized message via
  `dat0-i18n`). No modal spam.
- **Grid Save-as-Table failure (D9, P5b backlog).** Currently log-only (`tracing::warn!`, no UI). Add a
  `banner::push` toast (reuse PD-007 queue) so the failure is visible. Small, additive.

---

## §9. Trim valve

If spike S1/S2 or overall effort runs hot, drop in this order — **mirrors P5b's valve discipline**:

1. **Shallow catalog enumeration** (§4.3) — panel lists attachment aliases/status only, no database-name tree.
2. **Multi-sqlite add-flow** (§4.2 `Attach SQLite…`) — sqlite stays SQL-only as today; panel still *shows*
   any sqlite attached via SQL.
3. **Never** the md credential core: connect → keychain persist → background reconnect → routing-tagged chip.
   This is the D-007 closure and must ship.

A fired valve makes D-007's closure **partial** (md end-to-end ships; the dropped panel affordances retarget
to a later phase) and is recorded in the deferral register.

---

## §10. Non-goals (explicit — out of P5c)

- **Multiple MotherDuck accounts** (single account; keychain key is labelable for later additive multi).
- **Writes / Save-as-Table *into* md** — local targets only.
- **Per-table `TableOrigin` recording** (D-012 stays deferred — names-only listing in §4.3).
- **md-vs-local network/exec split timing** (Q4 rejected; routing tag only).
- **AccessKit / a11y** for the panel (D-015 → P10).
- **Command-palette overlay UI** — still descriptors-only (P5b D4 stance unchanged); `ConnectionsToggle` is a
  registered descriptor + menu action + key, not a new overlay.

---

## §11. Spikes (T0 — gate the slice)

- **S1 — Extension load.** Does runtime `INSTALL motherduck; LOAD motherduck;` succeed with bundled
  `duckdb-rs 1.4.4`? D-007's blocker was a missing Cargo feature; runtime install is the bet. **Fail →
  escalate before building UI**; the slice narrows or D-007 re-defers with findings.
- **S2 — Routing detection.** Reliable catalog-touch detection vs the `md.` string fallback (§6). Determines
  whether the chip tag is plan-accurate or heuristic.

---

## §12. Deferral register updates

- **D-007** → `closed` (full) on ship, or `partial` if the trim valve fires (md end-to-end closes; dropped
  panel affordances retarget). Update Target/Status + a closure note with merge SHA.
- **D-012** — reaffirm `deferred` (names-only enumeration; no per-table origins).
- **D-015** (AccessKit) — unchanged (P10); note the new panel as additional a11y surface.
- Record any new **PD-xxx** from implementation/review (e.g. the routing fallback's limitation if it ships).

---

## §13. Open questions for plan phase

- Exact `EngineError` variant names + i18n keys for md auth/extension/network failures.
- Whether `SET motherduck_token` is per-connection-session or needs re-issuing per reconnect (spike S1 detail).
- md `AttachOpts.read_only` default — proposed **read-write** (user's own account); confirm at plan time.
- Verify env var name DuckDB/MotherDuck expects (`motherduck_token` vs `MOTHERDUCK_TOKEN`) for both the
  `SET` path and the CI secret.
