# dat0 — UAT Gap 2: AccessKit content-assertion in the gpui behavioral harness (design)

> **Design doc.** Brainstormed 2026-06-30, off `main` `865c9b1` (PR #37, Gap 3
> merged). Third and final hard gap from the 2026-06-29 UAT-automation research
> (`memory/dat0-uat-automation-research`): Gap 1 (visual regression) stays
> human-UAT, Gap 3 (async-flow harness) merged in #37, and **Gap 2 — rendered-text
> / content assertion — is what remains.** This slice builds a test-only,
> cargo-feature-gated AccessKit emitter from dat0's own render code and reads it
> with `kittest`, so the headless harness can finally assert *what text is on
> screen* and locate widgets *by label* instead of by hand-tuned pixels. It does
> **not** close D-015 (production screen-reader a11y stays blocked on a gpui
> AccessKit adapter). Next step after user review: `writing-plans`.

---

## Context

The P11a behavioral harness (`crates/dat0-app/tests/onboarding_gpui.rs`:
`#[gpui::test]` + `VisualTestContext` + `simulate_click` + `debug_bounds`, plus the
Gap 3 async harness) can open a real window, drive production code paths, click
real widgets, and assert on dialog presence + on-disk artifacts. It is near the
automatable ceiling for *behavior*. It cannot read **rendered text**.

Two concrete symptoms in the shipped harness:

1. The per-panel tour test is `#[ignore]`d with the note: *"gpui's test harness
   exposes no rendered-text extraction to read the panel's headline/body"*
   (`onboarding_gpui.rs:687`). We can prove a click fired, not that the right
   headline now shows.
2. Clicks are located by **empirically-tuned pixel constants** — `(777, 550)` for
   the carousel Skip button (`onboarding_gpui.rs:546`), `(1700, 40)` for the first
   sample card (`:618`). These are fragile: any layout change silently moves the
   hit-box, and the comments admit they are "empirically tuned for the fixed
   1920×1080 TestDisplay."

## Problem

gpui emits no accessibility tree (D-015), so there is no built-in way to (a) read
the text a view actually rendered or (b) find a widget by its label/role. Every
off-the-shelf content-assertion route (kittest, Appium/XCUITest, macOS AX API)
converges on that same missing tree. The research's verdict: the canonical tool is
**AccessKit + `kittest`**, but adopting it first requires *building* the AccessKit
tree, because gpui will not produce one on the pinned `=0.2.2`.

## Approach (decided in brainstorm)

Build the tree from **dat0's own render code**, test-only and feature-gated — **no
gpui fork**. Three layers, all keyed by **one string id**:

- **Emit (the real work):** a feature-gated extension method `.a11y(id, role,
  label)` on gpui elements. During a render captured by the harness it (1) pushes a
  node into a thread-local frame collector and (2) chains the existing
  `.debug_selector(|| id.into())`. Compiled out (identity function) when the
  capture feature is off → **zero production cost**.
- **Read (cheap, off-the-shelf):** snapshot the collector into an
  `accesskit::TreeUpdate`, hand it to `kittest::State::new(update)`, and query with
  `get_by_label` / `By(role, text)`. Verified against current kittest source
  (rerun-io/kittest, MIT OR Apache-2.0): `State` is a thin wrapper over
  `accesskit_consumer::Tree`; a hand-built `TreeUpdate` is sufficient and **no
  event loop is required** for querying (the `run_frame()` glue in kittest's
  integration example is framework-specific and unused here).
- **Click (reuse, proven):** recover the id string from a queried node and resolve
  geometry through the existing `cx.debug_bounds(id)` → `simulate_click`. Content
  (AccessKit node) and geometry (gpui hitbox) stay in lockstep because both are
  keyed by the same id.

Why Approach A (element-wrapper) over the alternatives considered: a per-view
`Accessible`/`semantics()` trait (B) is a *parallel* description maintained apart
from `render()`, so it can drift out of sync — the exact bug class this work should
catch. True render-path interception (C) needs to wrap gpui's element internals
against the pinned `=0.2.2` — effectively a fork, which the recorded decision rules
out. A is the only option that is render-time, fork-free, and drift-resistant at
once (the annotation lives at the same call site as the text it describes).

## Components

| Component | Location | Responsibility |
|---|---|---|
| `A11yExt::a11y(id, role, label)` | `dat0-app/src/a11y/` (new) | element-wrapper helper; pushes node + chains `debug_selector`; identity no-op when feature off |
| `FrameCollector` (thread-local) | `dat0-app/src/a11y/` | accumulate `{NodeId, role, label}` for the current render; side-map `NodeId → id-string` |
| `TreeUpdate` builder | `dat0-app/src/a11y/` | snapshot collector → flat `accesskit::TreeUpdate` under one synthetic root |
| `cx.a11y_snapshot()` + query/click combinators | harness test-support | drive a render, build `kittest::State`, expose `get_by_label` / `query_by_role` / `click(cx, By::…)` |
| dat0→accesskit `Role` map | `dat0-app/src/a11y/` | small enum mapping (`Button`, `Label`/`StaticText`, `Dialog`, `Cell`, `Row`, `Alert`) |

**Tree shape — flat-under-root for v1.** All annotated nodes are direct children of
one synthetic root. kittest's label/role/text queries do not need hierarchy to find
a node, and flat sidesteps reconstructing parent/child from depth-first render
pushes. If scoping ("the button *inside this dialog*") is ever needed, add a
`.a11y_group(id, role, |…|)` guard later — explicitly **out of scope for v1**.

**Feature gating (one open plan-level detail).** Capture must be active for the
harness build and off in release. Preference: reuse a `test-support`-style feature
so CI needs no special flags; fall back to a dedicated `a11y-capture` feature with a
documented `--features` in `ci.yml`. Settled in the plan (the integration-test
crate-compilation boundary means plain `cfg(test)` is insufficient — a feature is
required).

## T0 spike (HARD GATE)

Before any broad rollout, prove the mechanism end-to-end on **one** element:

1. Add `accesskit` (^0.24), `accesskit_consumer` (^0.35), `kittest` (dev-dep). Per
   the D-015 register (re-checked 2026-06-23), `accesskit` is currently **absent
   from `Cargo.lock`** — gpui `=0.2.2` pulls none — so this is a purely additive
   dependency, not a coexistence gamble. `cargo tree` just confirms nothing else
   in the graph already pins a conflicting accesskit.
2. Implement the minimal `.a11y()` + `FrameCollector` + `TreeUpdate` builder +
   `cx.a11y_snapshot()`.
3. Annotate one existing element (the hero tagline or `hero-take-tour`) and assert:
   render → snapshot → `get_by_label` finds it; and `tree.click(cx, By::label(…))`
   opens the tour via `debug_bounds`.

**Go/no-go:** if capture-during-render does not fire (it should — `debug_bounds`
already returns real bounds in shipped tests, which proves the element-build +
layout passes run under `TestPlatform`; only GPU paint is the no-op), or the deps
clash, **stop and surface it** rather than grind. This is the research's "own-phase,
high-effort" item; the spike is the honest go/no-go.

## Proof targets (what broad coverage unblocks)

Annotate each surface's **render function** (one site covers all its cells/rows), so
"broad" stays bounded:

| Surface | File(s) | Nodes |
|---|---|---|
| Dialogs / onboarding | `onboarding/mod.rs`, `empty_state.rs` | panel title/body (`Label`); Next/Back/Skip/Get-started (`Button`) |
| Grid cells | `grid/*` | per-cell `Cell` with rendered (post-format) text, id `cell-{r}-{c}` |
| Inspector | `inspector/*` | profile/summary field rows (`Label`: name + value) |
| SQL results | `query/*` | results-grid cells + timing/status chip text |
| Errors | `error_ux/*` | user-facing message (`Alert`/`Label`) |

Concrete test wins: un-`#[ignore]` the per-panel Next/Back test with real headline
assertions; convert `skip_click_dismisses_and_writes_flag` `(777,550)` and
`hero_sample_click_imports_bundled_csv` `(1700,40)` to label-based clicks; add new
content-assertion tests per surface (grid cell text, inspector field, SQL result,
error message).

## Risks & mitigations

- **Capture-during-render may not fire** → T0 spike gates it; strong prior from
  working `debug_bounds`.
- **accesskit version clash** → low: gpui `=0.2.2` currently pulls no accesskit
  (D-015 re-check), so the add is purely additive; `cargo tree` in T0 confirms.
- **Drift between annotation and rendered text** → Approach A puts the annotation at
  the same call site as the text; the label *is* the rendered string (e.g.
  `.child(t(key))` and `.a11y(id, Label, t(key))` share the source). Teeth tests
  (below) catch any residual drift.
- **Flat tree insufficient for a future scoping need** → documented `.a11y_group`
  escape hatch; not built now (YAGNI).
- **Feature-flag forgotten in CI** → settled in plan; prefer auto-on
  `test-support`-style feature.

## Scope (NOT doing)

- **No gpui fork; no production a11y.** This is test instrumentation. The running
  app gains no screen-reader support, no OS AccessKit platform adapter, no
  gpui integration. **D-015 stays open.** (The node-emission annotations are a
  reusable down-payment if gpui ever ships an adapter, but that bridge is explicitly
  future work, not this slice.)
- **No pixel/visual-regression** (Gap 1 — stays human-UAT, blocked at the gpui
  level).
- **No hierarchy/scoping in the tree** (flat v1).
- **No accesskit→gpui action bridge** for clicking (kittest's example feeds
  ActionRequests back to egui's event queue; dat0 reuses `debug_bounds` +
  `simulate_click` instead — simpler, already proven).

## Testing

- **Teeth (house pattern):** every new content assertion is shown to *fail* when the
  rendered content is wrong (flip a panel index, mistype a label) — no vacuous
  greens. Mirrors the existing harness's teeth discipline.
- **Determinism / cross-platform:** labels are i18n strings, node ids are stable
  hashes, no timestamps/paths in the tree → byte-stable on macOS + Linux CI.
- **No new production code paths exercised in release builds** (feature off →
  `.a11y()` is identity), so the release binary and its tests are unchanged.

## Decomposition note

Single coherent slice: T0 spike (hard gate) → minimal emit/read/click infra →
per-surface annotation + tests, surface by surface. If the T0 spike fails the
go/no-go, this design is parked (not force-fit) and the gap returns to human-UAT
with the spike findings recorded, exactly as Gap 1 did.
