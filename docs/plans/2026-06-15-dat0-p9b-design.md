# dat0 P9b — MotherDuck: close the gaps (design)

**Status:** Brainstormed 2026-06-15. Supersedes the 2026-06-14 P9b stub
(`docs/plans/2026-06-14-dat0-p9b-design.md`). Ready for planning.
**Master-spec anchor:** §21.2 P9b (`docs/specs/2026-04-26-dat0-design.md:1126`).
**Branch:** `p9b-design` off `main` (`4aeb1a0`, P9a-2 merged).
**Sibling docs:** P9a `2026-06-14-dat0-p9a-design.md` (merged); P9c
`2026-06-14-dat0-p9c-design.md` (stub, greenfield).

---

## 1. Framing — verify-and-close-gaps, not a build

P5c (MotherDuck end-to-end + Connections panel, PR #12 `6d406e6`, closes D-007)
shipped ~80% of master-spec P9b: keychain token store, `ConnectionManager`,
per-window auto-reconnect, the `· md|local|mixed` routing chip, the Connections
panel, and the `precheck` flow. P9b audits that as-built surface against the
master-spec exit list and implements only the residue.

**Re-audited 2026-06-15. Confirmed present (do not rebuild):**

| Master-spec P9b item | As-built |
|----------------------|----------|
| Token in OS keychain | `connections/token_store.rs` `KeychainTokenStore` over `dat0_keychain::Keychain` |
| Process-shared token / per-window attach | `ConnectionManager` + P5c per-workspace auto-reconnect (workspace-mode `ATTACH 'md:'`, soft Disconnect) |
| Per-query local-vs-cloud routing chip | `connections/routing.rs` `classify_routing` + `connections.*` i18n |
| Connections / attachments UI | `connections/panel.rs` `render_connections` + `status_label` + `precheck` |
| Catalog lists attached MD databases | `ConnectionManager::md_databases()`; connect.rs `list_databases` filters `duckdb_databases()` `type='motherduck'` |

**Confirmed gaps (the P9b work):** Settings→MotherDuck section, a "Test connection"
button, a distinct catalog "Cloud" group, and a token-never-logged guard + exit
verification.

This is a **standalone thin PR** — it closes master-spec P9b explicitly and keeps
the security-sensitive P9c greenfield clean. **No schema bump**: settings stays v1,
session unchanged, no migration.

---

## 2. Decisions locked at brainstorm

### D1 — Connections panel is the canonical MotherDuck-settings surface

Master-spec names a "Settings → MotherDuck section (token entry + Test connection)".
**But the Settings window is an un-wired stub:** `settings_ui::open_settings_window`
is a `tracing::info!` placeholder (`settings_ui/mod.rs:48`), and existing sections
(theme, workspace) render placeholder text — real interactive widgets "mount when
the settings window is fully wired" (deferred P1.T13/T21). Adding a real
Settings→MotherDuck section is therefore blocked on that deferred work and is out of
P9b scope.

**Decision:** rule the existing **Connections panel** the canonical
MotherDuck-settings surface (it already owns token entry, connect/disconnect/forget,
status, and db enumeration). Add the "Test connection" affordance **there**. Mark the
master-spec "Settings section" exit item **satisfied-by-equivalent**; record the
deferral (§6). A decorative placeholder MotherDuck section in the un-opened settings
window was explicitly rejected — it ships dead, unreachable UI.

### D2 — Add a distinct "Cloud" catalog group

The catalog tree has three groups (Sources / Tables / Derived); MotherDuck attached
tables currently fall into **Sources**, mixed with local files. Master-spec P9b wants
cloud tables visibly distinct.

**Decision:** add a fourth **Cloud** group to `CatalogTree`, holding `Attached`
tables classified as MotherDuck. SQLite/file attachments stay under Sources. This is
the one real user-facing UI improvement in the slice.

**Classification rule (verified):** MotherDuck attach records
`TableOrigin::Attached { alias: <db_name>, source: dsn }` with `dsn == "md:"`
(`duckdb_engine.rs:721-730`, the `dsn.to_owned()` path); SQLite attach records
`source = <sqlite path/URI>` (`duckdb_engine.rs:769-778`). So:

> **Cloud ⇔ `TableOrigin::Attached { source, .. }` where `source.starts_with("md:")`.**

`starts_with` (not equality) also covers a future qualified DSN like `md:dbname`. This
is a pure data classification on engine-provided origins — **no cross-reference with
`ConnectionManager` is needed**, so the Cloud split lives entirely in pure `tree.rs`.

---

## 3. Scope — four deliverables

### 3.1 "Test connection" button (Connections panel)

A reversible probe affordance in the Connections panel MotherDuck section.

- New `ConnectionsEvent::TestMd` variant (`connections/panel.rs`).
- A thin pure-ish `test_connection` seam in `connections/connect.rs` that reuses the
  existing `precheck` → `run_connect` path. **No new engine surface** — workspace-mode
  `ATTACH 'md:'` is idempotent (P5c finding), so probing with the stored token is
  harmless and naturally leaves a good token in the Connected state.
- Behavior: token stored → run the probe, surface a transient success/error message
  (errors reuse the existing `connections.error.{auth,extension,network}` i18n keys);
  no token → route to the existing token prompt (same as Connect).
- Button shown in both Disconnected (when a token is stored) and Connected (re-verify)
  states. `handle_connections_event` gains a `TestMd` arm in `window.rs`.

### 3.2 Catalog "Cloud" group

- `CatalogTree` gains a `cloud: Vec<CatalogNode>` field (`catalog/tree.rs`).
- `build()` routes `Attached{source}` by the `md:` prefix → `cloud`, else → `sources`.
- `filter()` extends the token-AND retain to the new `cloud` vec.
- `catalog/panel.rs` renders a fourth section. New i18n key `catalog.cloud`.
- **Out of scope (do not touch):** the existing `Sources`/`Tables`/`Derived` group
  labels are hardcoded English string literals (`catalog/panel.rs:30-32`), a
  pre-existing inconsistency. Leave them — adding the one `catalog.cloud` i18n key for
  the new group is enough; retrofitting the others is scope creep.

### 3.3 Token-never-logged guard test

A headless unit test that locks the keychain-only boundary against regression. Set a
sentinel token in a `MemoryTokenStore`; serialize `Settings` to JSON and format
`ConnectionManager` / `ConnectionStatus` via `Debug`; assert the sentinel string is
absent from every surface. The structural truth already holds (token is keychain-only,
absent from the `Settings` schema and from `ConnectionManager` state) — the test
prevents a future field from leaking it.

### 3.4 Exit-criteria verification doc

`docs/plans/2026-06-15-dat0-p9b-uat.md`: token persists across windows/relaunch;
`SELECT FROM md.x.y` returns rows; routing chip correct per query; Cloud group shows
MD dbs distinct from local files; Test-connection success + failure (bad token).
Automatable parts get tests (§4); GUI/live-token parts join the standing UAT backlog.

---

## 4. Testing strategy

**Unit (headless, CI-safe, no live token):**
- `catalog/tree.rs`: MD `Attached{source:"md:"}` → `cloud`; SQLite
  `Attached{source:<path>}` → `sources`; `File`/`Derived` unchanged; `filter()`
  retains across the new `cloud` vec.
- `connections/connect.rs`: `test_connection` — no token → `NeedToken` (routes to
  prompt); token present → invokes the probe. The engine-touching arm stays behind the
  existing env-gated integration test, exactly like `run_connect`.
- Token-guard test (§3.3).

**Integration (env-gated `MOTHERDUCK_TOKEN`, both platforms in CI vs a live account —
the P5c pattern):**
- Extend the existing MotherDuck integration test: after attach, assert ≥1 table
  classifies into the Cloud group, and a `test_connection` probe returns success.

**Manual UAT (owed, GUI/live — joins the standing P4b/P4c/P5b/P5c/P6a/P8/P9a backlog):**
- Test-connection success toast + failure (bad token) error.
- Cloud group renders MD dbs distinct from local files; survives reconnect.
- Token persists across a 2nd window + relaunch; never in logs/`settings.toml`/telemetry.
- Routing chip still `· md|local|mixed` correct per query.

---

## 5. Components touched

| File | Change |
|------|--------|
| `connections/panel.rs` | `TestMd` event variant + button + transient result rendering |
| `connections/connect.rs` | `test_connection` seam reusing `precheck` + `run_connect` (no engine change) |
| `window.rs` | `handle_connections_event` arm for `TestMd` |
| `catalog/tree.rs` | `cloud` field + `md:` classification + `filter()` coverage |
| `catalog/panel.rs` | fourth "Cloud" section render |
| `dat0-i18n/.../en.json` | `catalog.cloud`, `connections.md.test`, `connections.md.test.ok` |
| tests | `tree.rs` cloud-grouping unit; token-guard unit; `connect.rs` test-connection unit; MD integration extension |

No engine crate change. No `dat0-engine` surface added. No settings/session schema bump.

---

## 6. Risks, unknowns, deferrals

**Low technical risk** — engine + keychain + routing are all proven in P5c CI against a
live token on macOS + linux. No new engine surface, no migration.

**The one new assumption — the `md:` prefix classification** — is verified against
`duckdb_engine.rs:721-730` (`source = dsn`, `dsn = "md:"`) and locked by the tree unit
test. `starts_with("md:")` covers a future qualified DSN.

**Primary risk = scope leakage** — re-litigating P5c design, or getting pulled into
wiring the whole settings window. D1 fences this off: Connections panel canonical,
settings-window work stays deferred.

**Unchanged pre-existing limit** — bare-name `table_origins` keying (P6a): a same-name
local vs attached table collide. Not P9b's job; documented.

**Deferrals recorded:**
- Real Settings → MotherDuck section — blocked on the un-wired settings window
  (P1.T13/T21). Revisit when that lands.
- Per-table cloud origins beyond db-name grouping (D-012 remainder) stay as-is.

---

## 7. Next step

`superpowers:writing-plans` → task-level implementation plan → subagent-driven-dev
(two-stage review per task) → CI both platforms (incl. the env-gated live-token MD
integration test) → manual UAT → merge, closing master-spec P9b.
