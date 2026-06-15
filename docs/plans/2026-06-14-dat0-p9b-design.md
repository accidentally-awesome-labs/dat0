# dat0 P9b — MotherDuck transparent ATTACH (design STUB)

**Status:** SUPERSEDED — fully brainstormed 2026-06-15. See
`docs/plans/2026-06-15-dat0-p9b-design.md` for the locked design (D1 Connections-panel
canonical, D2 distinct catalog "Cloud" group). This stub is kept as historical record.

**Original status:** STUB — not yet brainstormed. Created alongside the P9a design so
the P9 split is recorded. Do a full brainstorm before planning.
**Master-spec anchor:** §21.2 P9b (`docs/specs/2026-04-26-dat0-design.md:1126`).
**Sibling docs:** P9a `2026-06-14-dat0-p9a-design.md` (charts, active); P9c
`2026-06-14-dat0-p9c-design.md` (AI, stub).

---

## 1. Critical framing — P9b is ~80% already shipped by P5c

P5c (MotherDuck end-to-end + Connections panel, PR #12 `6d406e6`, closes D-007)
delivered most of master-spec P9b. **This is a verify-and-close-the-gaps slice, not a
build.** Audit P5c as-built against the P9b exit list; implement only the residue.

### Already present (P5c)

| Master-spec P9b item | As-built |
|----------------------|----------|
| Token in OS keychain | `connections/token_store.rs` `KeychainTokenStore` over `dat0_keychain::Keychain` (service-scoped get/set/delete) |
| Process-shared token service | `KeychainTokenStore` + `ConnectionManager` (`connections/mod.rs`) |
| Per-window engine ATTACH from shared token | P5c "per-workspace auto-reconnect" (workspace-mode `ATTACH 'md:'`, soft Disconnect — see memory `dat0_p5c_brainstorm`) — **verify each new window attaches at startup** |
| Per-query local-vs-cloud timing/routing chip | `connections/routing.rs` `classify_routing(sql, md_databases) -> Routing` + i18n keys (the `· md\|local\|mixed` chip) |
| Connections / attachments UI | `connections/panel.rs` `render_connections` + `status_label`; token-present `precheck` |
| Catalog shows attached MD databases | `ConnectionManager::md_databases()`; P6a catalog groups by origin — **verify the grouping reads as "Cloud"** |

### Residue (the actual P9b work)

1. **Settings → MotherDuck section.** `settings/schema.rs` currently has only
   `profile / theme / telemetry / workspace` — **no MotherDuck section**. P5c put token
   entry in the **Connections panel**, not Settings. Master-spec P9b names a *Settings →
   MotherDuck section: token entry + "Test connection" button*. Decide: add a Settings
   section that shares the same `KeychainTokenStore`, **or** rule the Connections panel
   the canonical surface and mark this exit item satisfied-by-equivalent.
2. **"Test connection" button** — explicit affordance that runs a cheap probe (token
   precheck → `ATTACH 'md:'` → trivial query) and reports success/failure. Partially
   covered by `precheck` + status; confirm a user-visible test action exists.
3. **Catalog "Cloud" section** — confirm/relabel MD databases under a distinct "Cloud"
   group (master-spec wording) vs the generic Attached origin grouping.
4. **Exit-criteria audit** — token never in logs / `settings.toml` / telemetry;
   SELECT from `md.x.y` returns rows; routing chip correct per query; token persists
   across windows + relaunch.

---

## 2. Scope (to confirm at brainstorm)

Likely the smallest P9 slice — possibly even **folded into P9a-2 or P9c** rather than
its own PR, depending on how much residue survives the audit. Default assumption: a
standalone thin slice = Settings→MotherDuck section + Test-connection + Cloud-grouping
relabel + a written exit-criteria verification (much of it manual UAT, already owed).

## 3. Key decisions to resolve at brainstorm

- Settings section vs Connections-panel-as-canonical (item 1).
- Whether "Cloud" is a real new catalog grouping or a relabel of existing origin
  grouping.
- Token-never-logged: add a CI/test fixture asserting the token is absent from any
  serialized settings/log surface (mirrors the P9c payload-filter test discipline).
- Multi-account: master-spec P9b is single-account (matches P5c's "single md account").

## 4. Risks / unknowns

- Low technical risk — engine + keychain + routing are proven in P5c CI against a live
  token on both platforms.
- Main risk is **scope leakage**: re-litigating P5c design. Keep to the residue list.

## 5. Next step

Full brainstorm (`superpowers:brainstorming`) → confirm residue is real (re-audit
P5c as-built at that time) → decide standalone vs folded → spec → plan.
