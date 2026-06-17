# dat0 Onboarding v1 — design (wireframes + tour script + sample-workspace selection)

**Status:** Approved design. **Design-only deliverable** — this document is the
artifact. No code, no tests, no `session.json`/settings schema change ships in
P9c-3.
**Implementation:** lands in **P11** (first-run-and-onboarding track). P11
consumes this document.
**Phase:** P9c-3 (last P9c slice; siblings P9c-1 secure plumbing, P9c-2 AI
features — both merged).
**Master-spec anchors:** §21.2 P9c scope + exit
(`docs/specs/2026-04-26-dat0-design.md:1161`, `:1172`); cross-cutting
"Onboarding & first-run" track (`:825`).
**Builds on (P3, already shipped):** the empty-state hero
(`crates/dat0-app/src/empty_state.rs`) and the three-sample catalog
(`crates/dat0-app/src/sample_data.rs` — Iris CSV, Chinook SQLite, NYC taxi
Parquet). This design *extends* those surfaces; it does not duplicate them.

---

## 1. Framing

The master-spec assigns first-run **onboarding design** to P9c and its
**implementation** to P11. Onboarding design benefits from the app being
feature-stable (charts, MotherDuck, Catalog/Inspector, AI all present), which it
now is — but because the slice ships no code, it has no runtime dependency on
P9c-1/P9c-2.

The whole design is governed by one read of the audience: dat0's users are
technical data people who dislike being hand-held. Every choice below leans
**least-intrusive, maximum-reuse** — enrich what P3 already built rather than
bolt on a separate onboarding subsystem.

---

## 2. Locked decisions

| # | Decision | Rejected alternative | Why |
|---|----------|----------------------|-----|
| **D1** | The mandated "first-run splash" **is the P3 empty-state hero, enriched** with a value-prop band + a [Take a tour] button, shown on first run only. No separate modal. | A welcome modal floating over the hero; an auto-starting guided tour. | Reuses the existing hero; one surface, not two; least intrusive for a technical audience. |
| **D2** | The skip-able tour is a **self-contained carousel modal** — ordered illustrated panels, decoupled from the live UI. | Spotlight/coach-marks anchored to real UI regions; diffuse inline contextual hints. | Cheapest and most robust to build in P11 (no element-anchoring, works before any data is loaded); still gives ordered, skip-able steps. |
| **D3** | The carousel is **7 panels**: Drop a file → Explore (grid) → Catalog + Inspector → SQL + NL→SQL → Charts → MotherDuck → Private by default. | A tight 5-panel core loop; a 3-panel minimal tour. | Full coverage of dat0's differentiators while still skip-able in one sitting. |
| **D4** | The featured "Try with sample data" path is a **curated `demo.dat0` workspace** (Chinook tables + a saved chart + a sample SQL tab + a small pipeline), built on the P8 package format. The three raw samples stay listed as a secondary option. | Feature Chinook as a raw open; feature NYC taxi (network-dependent). | Lands the user mid-exploration — the strongest single demo of the whole app. |
| **D5** | The carousel **auto-shows once** on the very first launch (persistent Skip), then is opt-in forever after via [Take a tour] + a Help menu entry. A persisted `first_run_done` flag gates the auto-show. | Purely opt-in (never auto-shows). | Matches the spec's "skip-able tour" wording — it appears, and you can skip it — without nagging on later launches. |

---

## 3. First-run state machine

```
launch ──► session empty (no open tabs AND no recents)?
              │ yes                              │ no
              ▼                                  ▼
        first_run_done flag?                 normal workspace
         │ false            │ true
         ▼                  ▼
   ENRICHED HERO        PLAIN P3 HERO
   + auto-open          (drop-zone + samples column,
     carousel once        no value-prop band,
   (Skip always shown)     no auto-popup)
         │
   finish OR skip ──► set first_run_done = true
```

- **`first_run_done`** is a persisted boolean. This design recommends it live in
  **settings** (the settings panel already exists), *not* in `session.json` —
  onboarding completion is per-install, not per-workspace. The exact field and
  any migration are P11's call; nothing is changed here.
- The carousel remains reachable forever via **[Take a tour]** on the hero band
  and a **Help › Take a tour** menu entry.
- "Don't show again" is **implicit**: finishing or skipping the carousel sets
  `first_run_done`. There is no separate checkbox.

---

## 4. Splash — the enriched hero (wireframe + copy)

On first run only, the P3 hero gains a top value-prop band and a featured demo
CTA. Everything below the band is the existing P3 hero, unchanged.

```
┌─ dat0 ──────────────────────────────────────────────┐
│  ◆ dat0                                              │
│  Explore data at native speed — local and private.   │
│                                   [ Take a tour › ]   │
│ ┌──────────────────────┬───────────────────────────┐ │
│ │                      │  Try the demo workspace   │ │   ◄─ featured CTA
│ │   Drop a file here   │  [ ▶ Open demo.dat0 ]     │ │
│ │   or  [ Open file… ] │                           │ │
│ │                      │  Or start from a sample:  │ │
│ │   CSV · Parquet ·    │   • Iris    150-row CSV   │ │
│ │   JSON · SQLite      │   • Chinook multi-table   │ │
│ │                      │   • NYC taxi 50MB Parquet │ │
│ └──────────────────────┴───────────────────────────┘ │
└──────────────────────────────────────────────────────┘
```

**Copy**

- **Headline:** `dat0`
- **Tagline:** "Explore data at native speed — local and private."
- **Tour affordance:** `[ Take a tour › ]` (top-right of the band).
- **Featured CTA:** "Try the demo workspace" → `[ ▶ Open demo.dat0 ]`.
- **Secondary samples:** "Or start from a sample:" followed by the existing
  three entries from `sample_data::entries()` (Iris / Chinook / NYC taxi),
  rendered exactly as P3 already does — this design only demotes them beneath the
  featured demo CTA.

**Notes for P11**

- The band and featured CTA render **only when `first_run_done == false`**. On
  every later empty state, the plain P3 hero renders unchanged.
- `[ Open file… ]` is an explicit button beside the existing drop-zone affordance
  (discoverability for users who don't think to drag); it triggers the same
  open-file path the menu already uses.

---

## 5. Tour carousel (wireframe + 7-panel script)

```
┌─ Take the tour ─────────────────────────────────┐
│                                                 │
│        [ illustration / annotated shot ]        │
│                                                 │
│   Drop a file                                   │
│   Open CSV, Parquet, JSON or SQLite — or just   │
│   drag one in. No import wizard, no waiting.     │
│                                                 │
│  [ Skip ]        ● ○ ○ ○ ○ ○ ○      [ Next › ]  │
└─────────────────────────────────────────────────┘
```

**Chrome (every panel)**

- **[ Skip ]** bottom-left — always visible, dismisses the whole carousel and
  sets `first_run_done`.
- Dot pager (center) shows position; **[ ‹ Back ]** appears from panel 2 on.
- **[ Next › ]** bottom-right; on panel 7 it becomes **[ Get started ]** (closes
  the carousel, lands on the hero).
- The carousel is **decoupled from the live UI** (D2): panels are
  self-contained, so no anchoring to real elements and no dependence on whether
  data is loaded.

**Panel script** — each panel is `{ illustration spec, headline, one-line body }`.
Illustrations are **placeholders** here; P11 produces the real assets (static
illustrations or annotated screenshots).

| # | Headline | Body copy | Illustration spec |
|---|----------|-----------|-------------------|
| 1 | Drop a file | Open CSV, Parquet, JSON or SQLite — or just drag one in. No import wizard, no waiting. | A file dropping onto a data grid. |
| 2 | Explore fast | Sort, filter, and edit millions of rows at 60 fps. Your edits are instant and non-destructive. | A grid with active sort + filter glyphs. |
| 3 | Know your data | Every table is auto-profiled — distributions, null rates, top values, and full lineage — right in the Inspector. | An Inspector column card with a mini histogram. |
| 4 | SQL, or plain English | Write DuckDB SQL with autocomplete, or ask in plain English and let AI draft the query. You always review before it runs. | A SQL tab with the NL→SQL chip highlighted. |
| 5 | See it | Turn any result into a bar, line, or scatter chart in a click. Export as PNG. | A simple bar chart. |
| 6 | Go to the cloud | Connect your MotherDuck account to query cloud DuckDB right beside your local tables. | A cloud icon and local tables side by side. |
| 7 | Private by default | Everything runs locally. AI is bring-your-own-key and off until you add one — your data and keys stay on your machine. | A lock over a local disk. |

**Copy invariants** (must survive P11 implementation)

- Panel 4 explicitly states AI-drafted SQL **is reviewed before it runs** —
  consistent with the never-auto-run discipline from P9c-2.
- Panel 7 states AI is **off until a key is added** and data stays local —
  consistent with the P9c privacy posture and the privacy doc finalized in P11.

---

## 6. Sample workspace — `demo.dat0`

The featured one-click path (D4). A **curated `.dat0` package** that opens into a
rich, mid-exploration state rather than a bare table.

**Contents**

- **Chinook tables** attached (albums, artists, tracks, invoices, customers,
  genres, …) — a multi-table dataset that exercises the Catalog, SQL joins, and
  the Inspector.
- **One saved chart** — "Revenue by genre" (bar), with its lineage to the source
  query intact.
- **One SQL tab** — a pre-filled "Top customers" query. It is **not** auto-run;
  the user clicks Run (consistent with the never-auto-run discipline).
- **One grid** with a small projection pipeline applied, so the PipelineBar is
  visible.
- Opens with the **saved-chart tab focused**.

**Build constraint for P11 (important)**

`demo.dat0` **must be authored via in-app export**, not the cold CLI exporter.
Per P8 deferral **D-025**, derived origins (the saved chart's lineage, the
pipeline) are in-memory-only and a *cold CLI* `.dat0` export **flattens** them,
while the *in-app* export path **preserves** lineage. So the production flow is:
inside dat0, drop Chinook → build the chart + pipeline + query → in-app
**Export `.dat0`** → check the artifact into `crates/dat0-app/assets/` → bundle
via `include_bytes!` (mirrors how `chinook.sqlite` is already bundled; expect
~1–2 MB). On load, P11 should verify the chart's lineage survives the
round-trip.

**Maintenance:** `demo.dat0` is a frozen binary artifact — it must be
re-authored whenever the `.dat0` package format or the features it showcases
change. Tracked as a deferral (§9).

**The three raw samples are unchanged.** Iris / Chinook / NYC taxi stay listed
in the hero (secondary to the demo CTA) and keep their existing
`sample_data.rs` behavior, including the NYC-taxi remote-fetch + retry-banner
path.

---

## 7. README polish (first-run section)

Add a **Quick start / First run** section to `README.md`:

- Install → launch → "Try the demo workspace" (or drop a file).
- One screenshot of the enriched hero.
- A 3-bullet feature highlight: native-fast grid · SQL + charts · bring-your-own
  AI.
- A privacy line up top: local-first; AI is off until you add a key.
- Links to AI BYOK setup and the privacy doc (the privacy doc is finalized in
  P11 per the cross-cutting track).

The README edit itself lands in P11 alongside the rest of the implementation.

---

## 8. Out of scope (→ P11)

P9c-3 ships **only this document**. All of the following is P11:

- `first_run_done` detection + persistence (recommended: a settings field).
- Enriched-hero value-prop band + featured demo CTA render.
- The carousel widget (modal, pager, Skip/Back/Next, Help-menu entry).
- Authoring + bundling `demo.dat0`; verifying lineage round-trips.
- Producing the seven illustration assets.
- The README first-run edit.

No code, no tests, no `session.json`/settings schema change in P9c-3.

---

## 9. Deferrals / rejected, recorded

- **Spotlight / coach-mark tour** — rejected in favor of the self-contained
  carousel (D2); revisitable post-v1 if telemetry shows users want in-context
  guidance.
- **Inline contextual hints** (per-surface one-time chips) — rejected; doesn't
  match the spec's "ordered steps" framing. Could complement the carousel later.
- **Localized onboarding copy** — the i18n `t("…")` structure exists; onboarding
  strings ship English-only at v1, structured for later locales.
- **`demo.dat0` refresh process** — no automation; the artifact is re-authored
  by hand when the format or showcased features drift. Candidate for a small
  build-time check in a later phase.

---

## 10. Verification (how P11 confirms it met this design)

Because the slice is design-only, "tests" here are the acceptance checks P11
runs against this document:

- First launch (empty session, `first_run_done == false`) shows the enriched
  hero **and** auto-opens the carousel exactly once.
- Skipping or finishing the carousel sets `first_run_done`; subsequent launches
  show the plain P3 hero with no auto-popup, and [Take a tour] / Help still open
  the carousel.
- `[ ▶ Open demo.dat0 ]` opens the curated workspace with the saved chart's
  lineage and the pipeline **intact** (round-trip verified).
- The three raw samples still load exactly as before.

---

## 11. Terminal state

This slice is **design-only**, so the brainstorming flow ends at this committed
document — there is **no `writing-plans` step and no execution** in P9c-3. P11
consumes `docs/design/onboarding-v1.md` as its input.
