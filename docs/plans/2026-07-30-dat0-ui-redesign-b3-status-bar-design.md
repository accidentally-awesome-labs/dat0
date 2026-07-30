# Slice B3 — Status bar (UI redesign)

Branch: `feat/ui-redesign-b3-status-bar` off main `1e149f2`.
Master plan: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6 row B3.
Predecessors: B2 `b2ea4f2` (modals pt 2 + anchored overlays), B1 `abd47f2` (ModalHost),
A1–A6 (theme unification → surface migrations).

---

## 0. What this slice is

dat0 has no status bar. The information a data workbench should keep permanently
visible — how many rows and columns am I looking at, how much is selected, how long
did the last query take, what am I connected to — is today either invisible
(row/column/selection counts exist only as internal state), or buried inside a panel
that is closed by default (the SQL console's timing chip).

B3 adds a permanent, full-width status bar at the bottom of the workspace shell. It
is **read-only chrome**: no tab stops, no click targets, no new actions, no session
schema change. It reads cached scalars the shell already holds and renders text.

The master plan sizes this slice **S**. It is deliberately the smallest B-slice:
it is the last one before B4 (command palette) and B5 (DockArea), both of which are
structural. Nothing here should grow.

### Owner decisions taken at brainstorm (all as recommended)

| Decision | Chosen | Rejected alternatives |
|---|---|---|
| Segment set | Master-plan four **+ column count** (`1,234 rows × 12 cols`) | plan four exactly; rows+selection only; + active table name (duplicates the tab strip) |
| SQL timing chip | **Mirror it** — status bar shows it always, console keeps its own | show only when console hidden (info moves); move it (breaks 2 standalone-console tests) |
| Selection count | **Exact union area, arithmetic** — new pure `SelectionModel::selected_cell_count` | sum-of-areas with double-count caveat; exact-for-one-range-else-`k ranges` |
| Visibility | **Always mounted**, segments conditional inside | only while a real grid is mounted |
| Visual treatment | **Text-only, muted, no icons, no colour-coded state** | icon + text per segment (re-opens A5 asset work); colour-coded connection (needs new contrast pairs) |
| Test home | **Unit tests + one case appended to `tests/a11y_content.rs`** — zero new binaries | new `tests/status_bar.rs` (~0.3 Gi of macOS CI headroom); unit tests only (no mount gate) |

---

## 1. Verified facts this design rests on

All read from the tree at `1e149f2`, not assumed.

### 1.1 `SelectionModel::resolved_cells()` is O(selected cells) and must never be called per frame

`src/grid/selection.rs:172` builds a `BTreeSet<(usize, usize)>` over every selected
cell. Its doc comment says selection sizes "are always small (screen-space)", which is
true of its current callers (copy / cut / edit-commit) because they are user-initiated,
once per gesture.

`select_all()` (`selection.rs:153`) sets one range spanning the whole model, and the
model is constructed with `rows = ds.row_count` (`window.rs:6442`). So `Cmd+A` on a
1M-row × 20-col table makes `resolved_cells()` a 20-million-element `BTreeSet` build.
A status bar calls its inputs **every frame**. The invariant that makes
`resolved_cells()` safe is not a property of the function — it is a property of who
calls it and how often, and B3 changes that. Hence §2.2's arithmetic count.

### 1.2 The contrast gate covers muted-on-`background`, and explicitly warns about muted-on-fill

`tests/theme_contrast_gate.rs:46` gates `("muted.foreground", "background", 4.5)` across
all three builtins. There is **no** gated pair for muted text on `secondary`, and
`theme_contrast_gate.rs:288-290` records the reason: putting `text_muted` on a pill
fill measures ≈4.0 in dark, i.e. it fails AA and forces a palette decision.

⇒ The bar uses the plain `background` fill and is separated from the body by a top
border only. A distinct fill would drag palette retuning into a slice sized S.

### 1.3 The SQL console's timing chip is asserted by tests that mount the console *standalone*

`tests/a11y_content.rs:513` (`sql_console_renders_result_and_timing_content`) mounts the
console via `open_sql_console_window` — no `WorkspaceShell`, therefore no status bar.
`tests/motherduck_window.rs:214` asserts `has_label_contains("ms · md")` the same way.

⇒ Moving the chip out of `src/view/sql_console.rs` would break both. The chip stays;
the status bar mirrors it. Both existing assertions use the duplicate-tolerant
`has_label_contains`, so a second copy of the text in a shell-mounted test is safe.

### 1.4 i18n has no interpolation

`dat0_i18n::t(key) -> String` (`crates/dat0-i18n/src/lib.rs:18`) over a flat
`en.json`. There is no placeholder substitution anywhere in the tree. The established
pattern is numbers in `format!`, nouns from keys — exactly what the timing chip does
(`sql_console.rs:1199`: `format!("⏱ {ms} ms · {}", dat0_i18n::t(key))`).

Keys must be **literal**, never `format!`-constructed (the P9c-1 review finding).

### 1.5 Where the shell can mount a bottom row

`WorkspaceShell::render` (`window.rs:6337`) ends with a flat child list on the root:
`banner_host` → `tab_strip` → `pipeline_bar` → `sql_console_panel` → the body row
(`window.rs:6994`, a `flex_row` holding catalog / connections / AI docks, the body, and
the inspector / charts docks) → `popover_overlay` (`:7060`) → `editor_overlay` →
`modal_overlay` → `Root::render_sheet_layer` / `render_dialog_layer`.

The three overlays are `.absolute()`, so the bar's flow position is decided entirely by
the body row. Mounting it directly after the body-row `.child(...)` and before
`.children(popover_overlay)` puts it below every dock and keeps the overlays painting
above it.

### 1.6 The data the bar reads (all cached scalars, no I/O, no query)

| Segment | Source | Notes |
|---|---|---|
| rows | `data_source.row_count: u64` (`grid/data_source.rs:30`) | already the *active view's* count — filters rebind the source |
| cols | `self.column_view.len()` (`window.rs:2209`) | post-projection = what the grid paints; falls back to `ds.visible_column_count()` when the fold is empty |
| selection | `self.selection: Option<SelectionModel>` (`window.rs:2173`) | §2.2 |
| query | `sql_console.{running, last_elapsed_ms, last_routing}` (`view/sql_console.rs:153,179,181`) | `last_routing: Routing::{Local,Md,Mixed}` → `i18n_key()` |
| connection | `self.connections: ConnectionManager` (`window.rs:2351`) | `md_status()`, `sqlite()` |

`GridTableDelegate`'s doc comment at `grid/mod.rs:172` already promised this:
*"so the same source can be inspected by other views (e.g., status bar row-count badge
in a later task)"*. B3 is that task.

### 1.7 Carried forward — do not rediscover

- `a11y()` **and** `a11y_label()` both `push()` a new capture node. Never add a label to
  an element that already has one (A5).
- `focus_stop` has been 5-arg since A6a and `a11y::FOCUS_RING` no longer exists — B3
  adds no focus stops, so neither applies, and that is the point.
- `tests/style_lint.rs` matches banned colour constructors **in prose too** (three
  recorded instances). Doc comments in this slice must not spell them with call parens.
- The ratchet is `ALLOW = &[("window.rs", 1)]` and must not grow: read tokens, never
  literals.

---

## 2. Design

### 2.1 `src/view/status_bar.rs` — model + pure fns + one render fn

**Deviation from the master plan**, which names the file `src/status_bar.rs`. Every
other piece of shell chrome lives under `src/view/` — `pipeline_bar.rs`,
`sql_console.rs`, `name_prompt.rs`, `query_library.rs`, `saved_query_picker.rs`,
`cell_editor.rs`. `src/overlay.rs` sits at the root because it is a cross-cutting
*mechanism* (scrim, trap, anchored surface) rather than a rendered surface. A status
bar is a rendered surface. It goes in `src/view/`.

Shape mirrors `pipeline_bar.rs`: pure logic that is unit-testable with no window, plus
one free render fn taking `&App`. No entity, no state, no subscriptions.

```rust
/// What the last SQL run is doing, as far as the status bar is concerned.
pub enum QueryStatus {
    /// No console, or no run has completed yet.
    Idle,
    Running,
    Done { ms: u64, routing: Option<Routing> },
}

/// Everything the bar renders, snapshotted from the shell once per frame.
pub struct StatusBarModel {
    pub rows: Option<u64>,
    pub cols: Option<usize>,
    pub selected_cells: Option<usize>,
    pub query: QueryStatus,
    pub connection: String,
}

impl StatusBarModel {
    /// Segment strings in render order. Pure — the whole rendered text of the
    /// bar is assertable without a window.
    pub fn segments(&self) -> Vec<String>;
}

pub fn render_status_bar(model: &StatusBarModel, cx: &App) -> impl IntoElement;
```

The shell builds the model inside `render` from the fields in §1.6 and mounts
`render_status_bar(&model, cx)` at the position in §1.5. `WorkspaceShell` gains no new
field.

`segments()` is the seam that makes this slice cheap to test: the render fn maps
one segment string to one `div`, so proving the strings proves the content.

### 2.2 `SelectionModel::selected_cell_count()` — exact union area, no cell iteration

New pure method on `SelectionModel`, next to `resolved_cells`:

```rust
/// Number of distinct selected cells, computed arithmetically from the
/// rectangles — overlapping ranges are counted once, and the cost depends on
/// the number of RANGES, never on the number of selected cells.
pub fn selected_cell_count(&self) -> usize
```

Coordinate compression over the rectangles:

1. Collect the sorted, deduped row boundaries `{ r_lo, r_hi + 1 }` and column
   boundaries `{ c_lo, c_hi + 1 }` of every range (normalising `r0 > r1` /
   `c0 > c1`, which `extend_to` can produce).
2. Walk the compressed grid of `(row band × column band)` blocks. A block is either
   entirely inside a range or entirely outside it, so one `contains` probe on the
   block's lower corner decides it.
3. Sum `row_span * col_span` for covered blocks, with saturating arithmetic.

Cost is O(k²) blocks × O(k) probes in the **range** count k, with k bounded by the
number of `Cmd`+clicks. `select_all()` is k = 1 → one block → one multiplication.

The doc comment states, in one line, why the obvious implementation
(`resolved_cells().count()`) is wrong here, so the next reader does not "simplify" it
back. A unit test constructs a 1 000 000 × 20 model, calls `select_all()`, and asserts
`20_000_000` — if anyone re-routes this through `resolved_cells`, that test does not
fail, it hangs, which is a louder signal than a red assertion.

### 2.3 Segment content

| Segment | Rendered | Shown when |
|---|---|---|
| Shape | `1,234 rows × 12 cols` | a data source is mounted |
| Selection | `84 cells selected` | `selection.has_selection()` |
| Query | `Query 12 ms · local` / `Query running…` | a console exists and (is running or has completed a run) |
| Connection | `Local` / `MotherDuck` / `Connecting…` / `Connection error`, `· 2 attached` | always |

Numbers come from `format_count(u64) -> String` (`,` grouping, pure, unit-tested).
Nouns come from literal i18n keys. The connection string comes from
`describe_connection(&ConnectionManager) -> String`, which maps all four
`ConnectionStatus` variants (`connections/mod.rs:10`) and appends the attachment count
from `sqlite()` when non-empty.

Deliberate omissions: the query segment keeps the console chip's `N ms · routing` tail
verbatim so the two can never look like they disagree, but drops the chip's `⏱` prefix
for the word `Query` — the owner's chosen treatment is text-only, and a leading noun
also disambiguates a bare duration in a row of other numbers. `md_databases()` is not
shown, because it is only populated after a
successful Test-connection and would read as `MotherDuck · 0 db` while genuinely
connected.

### 2.4 Visuals — tokens only, ratchet untouched

```
h_flex()
  .w_full()
  .bg(cx.theme().background)          // §1.2 — the only AA-gated backdrop for muted text
  .border_t_1().border_color(cx.theme().border)
  .px_sp(Sp::S12).py_sp(Sp::S4)       // A2 spacing scale (SpStyled)
  .gap_sp(Sp::S12)
  → per segment: .text_role(TextRole::Small) (TypoStyled, 12px)
                 .text_color(cx.theme().d0().text_muted)
  → between segments: a 1px vertical rule in cx.theme().border
```

No icons and no glyphs: the segment strings carry no `Dat0IconName`, so A5's glyph grep
stays clean and no new SVG or NOTICE entry is needed. No colour-coded state either —
`Connecting…` and `Connection error` say what they are in words, which survives high
contrast and screen readers, and which keeps A3's contrast matrix unchanged.

Zero colour literals ⇒ `style_lint` stays at `[("window.rs", 1)]`.

### 2.5 a11y — content, not navigation

One `.a11y_label(AccessRole::Label, text)` per rendered segment. No `a11y()` container
node (nothing here is clickable, and a "Status bar" group label would be noise for a
screen reader while adding a node every existing snapshot has to tolerate). No
`focus_stop`, no `tab_index`, no `tab_stop`.

⇒ Every keyboard-nav cycle count in the suite is structurally unchanged. This is the
invariant the master plan asked for, and it is enforced by construction rather than by
assertion.

### 2.6 What does *not* change

- No session schema change, no persistence, no new settings.
- No new actions, no new key bindings, no menu items.
- `grid/mod.rs` is untouched → the macOS grid-scroll bench carries structural-nil risk.
- The SQL console keeps its own timing chip verbatim.
- The tab strip keeps `tab_id` + the dirty dot; the bar does not repeat them.

---

## 3. Tests

### 3.1 Unit — `src/view/status_bar.rs` and `src/grid/selection.rs`, no window

- `format_count`: `0`, `1`, `999`, `1000`, `1_234_567`.
- `selected_cell_count`: empty; single cell; one rectangle; two disjoint rectangles;
  two **overlapping** rectangles (the case sum-of-areas gets wrong); a `Cmd`+click
  inside an existing range; `select_all()` on a 1 000 000 × 20 model.
- `describe_connection`: all four `ConnectionStatus` variants × {no attachments, two
  attachments}.
- `StatusBarModel::segments`: empty model (connection only); full model; selection
  present but zero rows; `QueryStatus::Running` vs `Done`.

### 3.2 Rendered — one case appended to `tests/a11y_content.rs`

`status_bar_renders_content` reuses the file's existing `open_workspace_window` helper
(`tests/a11y_content.rs:112-131`), which mounts a real `WorkspaceShell` exactly as
production does. It imports the same fixture the Task-5 cell test uses, settles the
async harness, captures, and asserts the shape and connection segments are present as
`Label` nodes.

Assertions use the duplicate-tolerant `has_label_contains` / `has_label_any`, never the
unique-match `has_label` / `query_by_role` — those panic on duplicates, and the query
segment deliberately duplicates the console chip's text.

### 3.3 Non-vacuity

Per the A6 lesson, each new rendered assertion is proven red before it is trusted:
perturb the segment text, confirm failure, revert, `touch` the file (an `mv`-revert
backwards-dates it and cargo silently reuses the stale binary), re-run green.

---

## 4. Invariants and gates

**T0 gate, before any real content is written.** Mount the bar with a single
placeholder segment and run the full `a11y-capture` suite. New `Label` nodes can break
another test's unique-match query — `has_label` and `query_by_role` panic on duplicate
matches — and the cheapest moment to discover that is before the content exists.

**T0 RESULT (run 2026-07-30): the gate fired, and not in the predicted way.**
110 binaries green, one failure: `tests/a11y_spike.rs:96` asserts
`snap.click_ids.len() == 7` — an exact node count that exists as a *frame-bracket
double-render proof* for the hero, not as a content assertion. Any added capture site
breaks it, whatever the label says. So the real hazard was never duplicate labels; it
was an exact-count invariant in an unrelated file.

The count is legitimately 8 once B3 lands: on the empty-state hero the bar renders its
connection segment and nothing else (no data source ⇒ no shape, selection, or query
segment), so it contributes exactly one deterministic site. Task 3 updates the constant
and its explanatory comment. Nothing else in the suite reacts to a new node.

Local gate (the substitute gate; `cargo test --workspace` and `cargo bench` remain
unrunnable on this machine, see the dev-workflow memory):

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- `cargo test -p dat0-app` — all binaries
- `cargo test -p dat0-app --features a11y-capture` — 111 binaries, 0 failures
- `cargo test -p dat0-app --features a11y-capture,gallery`
- `tests/style_lint.rs` 4/4, ratchet unchanged at `[("window.rs", 1)]`
- `scripts/i18n-check.sh` (warn-only) — new `status.*` keys present, no duplicate keys
  silently overwriting existing values

CI: no new test binary ⇒ macOS `DISK[after-live-ai]` should hold at ≈4.8 Gi (the #65
hotfix line is 2.9 Gi). Post-merge main run must still be verified **at step level** —
a green job can mask a skipped bench — and the artifact downloaded for the ns/iter.

---

## 5. Owed human glance

First permanently-visible chrome added since A6. Worth a look in all three themes,
high contrast most of all:

- the bar's top border against the body and against the SQL console panel's own border
- muted text legibility at `TextRole::Small` (12px) on `background`
- the segment rules — 1px `border` may vanish in high contrast or read as heavy in dark
- the bar with an empty state (connection segment alone) vs a full grid with a selection

---

## 6. Non-goals

- Interactivity of any kind — clicking the connection segment to open the Connections
  panel is B4-or-later territory, and adding a click target here would add a tab stop
  and change nav cycle counts.
- Progress/spinner rendering for long imports — that is the banner's job.
- Deduplicating the console timing chip. That question is properly settled at B8, when
  the console becomes a dock panel.
- Any DockArea work (B5+), any palette work (A-series is complete).
