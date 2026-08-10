# dat0 production v1 — manual UAT checklist

> Slice AX2. Written 2026-08-08 against the production-v1 plan.
> Companion gate document: `docs/a11y.md` (re-audited the same day, slice AX1).
> Predecessor, still owed and **not** superseded:
> `docs/plans/2026-06-17-dat0-p10a-uat.md` (signing, notarization, install).

## Why this exists

This is the mechanism, not a formality.

Every high-severity defect in `docs/deferrals.md` was a **"logic green, screen
dead"** gap: the code was correct, the tests were green, and the feature did not
work when a human drove the app.

- **PD-016** — P4a shipped funnel-click, popover-apply and sort-zone-click
  wirings that no task owned. Four separate implementers each wrote "T13 wires
  this"; T13's plan covered none of them. Every unit test passed. Clicking a
  funnel did nothing.
- **PD-017** — the whole P4b edit/delete overlay was built on `__dat0_rowid`,
  a surrogate column that `register_file` never creates, because imports are
  DuckDB **VIEWs**, not base tables. Engine tests were correct and complete;
  edit and delete failed on every real import.
- **PD-018** — `GridTableDelegate::render_td` rendered the em-dash placeholder
  for **every** cell and never called `render_cell` or `page_for`; `page_for`,
  the only method that populates the page cache, had **zero** production
  callers. The grid showed `—`, copy read empty strings, and the cache stayed
  empty. All of P4b's edit/select/clipboard logic was test-green throughout.
- **PD-021** — `error_ux::push` enqueued banners into a global `PENDING` queue
  that **nothing** drained in the runtime render tree; only `#[cfg(test)]` code
  called `drain_pending`. Export success, export failure and the paste-reject
  banner were all invisible to the user.

Four defects, all of them invisible to what is now 177 integration-test binaries
across the workspace (126 in `dat0-app` alone), all of them caught in minutes by
a human driving the app. That is the specific failure mode this checklist
exists to catch, and it is why the instructions below say **look at the screen**
rather than **assert the state**.

Every item is worded so that "it didn't visibly happen" is a **failure**, not an
ambiguity.

---

## Pre-requisites

- A **clean VM** — see Section 10. A machine where dat0 has previously run has
  a `settings.toml`, a `recents.json`, and a session directory, all of which
  hide first-run defects.
- A release build. Remember the toolchain workaround:
  ```bash
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
  export SDKROOT=/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX26.5.sdk
  cargo build -p dat0-app --release
  ```
- Test data, in increasing size:
  - `crates/dat0-app/assets/iris.csv` — small, instant.
  - `crates/dat0-app/assets/chinook.sqlite` — multi-table.
  - `crates/dat0-app/assets/demo.dat0` — a package.
  - A **≥1 GB** CSV or Parquet, so page-cache misses are real. Generate with
    the `dat0-fixtures` crate.
- A MotherDuck token, for the attached-connection rows.
- `docs/deferrals.md` open in an editor. File defects **as you find them**, not
  from memory at the end — see Section 10.

**Recording convention.** Every box gets `[x]` (observed, matches) or a
`PD-` number written beside it. An unchecked box at the end of the run means the
run is not finished. There is no third state.

---

## Section 1 — Cold start

The window a user sees before anything is ready. Under EN4 the session opens
off-thread, so the shell renders while DuckDB is still opening — the exact
window where a "logic green, screen dead" gap is invisible to a test that waits
for `Ready`.

### 1.1 The booting shell is visible and honest

- [ ] Launch the app on a machine with no prior state.
- [ ] **Confirm: a window appears before the engine is ready.** Not a beachball,
      not a blank rectangle — the shell chrome with a skeleton placeholder where
      the drop zone goes.
- [ ] **Confirm: the title-bar pill reads `starting` in amber** during this
      window (`SessionPhase::Booting` → `titlebar.starting`). See §4.2.
- [ ] **Confirm: the status bar does not lie.** No `engine duckdb · native`
      badge before an engine exists; no `0 fps`.
- [ ] The hero copy is present tense and carries **no** honesty label. It must
      **not** say "concept render", "designed, not shipped", "not shipped yet",
      or show an "in development" pill. Those are the marketing page's labels
      for an unshipped product and are false inside the shipped app.
- [ ] The privacy line reads `nothing imported · 0 bytes left this machine`.

### 1.2 A file dropped during boot must queue, not vanish (EN4)

This is the row most likely to fail silently.

- [ ] Relaunch. Within roughly **200 ms** of the window appearing — before the
      pill turns green — drag a CSV onto the hero.
- [ ] **Confirm: the drop is accepted.** The hero must show drop-target feedback
      while booting; a dead drop zone is a defect.
- [ ] **Confirm: the file opens by itself once the pill turns green.** No second
      drag, no click, no error.
- [ ] Repeat, dropping **two** files in quick succession during boot.
      **Confirm: both open, in the order dropped.**

### 1.3 Boot failure degrades, it does not hang

- [ ] Make the session directory unwritable (`chmod 000`) and launch.
- [ ] **Confirm: the pill turns red and reads `engine unavailable`**, and a
      banner appears with the failure text.
- [ ] **Confirm: the app does not retry in a loop and does not hang.** The
      banner carries a retry action; nothing happens until you press it.
- [ ] Restore permissions, press retry, confirm recovery.

### 1.4 Quit during boot

- [ ] Launch and immediately quit (`Cmd+Q`), before the pill turns green.
- [ ] **Confirm: the process exits.** A hang here means the boot join handle is
      not dropped on the shutdown path.

---

## Section 2 — Focus indicators

`docs/a11y.md` §1.1 marks A2 **PASS** on measured contrast:
`ring`/`background` is gated at ≥4.5 and measures 7.37 (dark) / 8.79 (light) /
19.56 (high-contrast). What the gate cannot measure is **geometry** — whether
the ring is actually drawn, at a visible width, and not clipped by whatever the
focused element sits inside. That is this section.

Run the whole section **three times**, once per theme.

- [ ] Theme: dark
- [ ] Theme: light
- [ ] Theme: high-contrast

For each theme:

- [ ] Tab from the shell root through the full cycle. **Confirm: at every stop,
      a ring is visibly drawn.** A stop that takes focus with no visible ring is
      a defect even though the token contrast passes.
- [ ] **Confirm: the ring is not clipped** — not by a dock edge, not by a scroll
      container, not by a modal border. A ring drawn half outside its clip
      region is the same defect as no ring.
- [ ] **Confirm: exactly one ring is visible at a time.** Two rings means a
      stale `FocusHandle` is still painted.
- [ ] Focus an element inside the **grid** (which paints its own ring at
      `grid/mod.rs:589`) and confirm it is the same colour as the chrome rings.
- [ ] Focus an element inside a **modal** and confirm the ring reads against the
      modal surface, not just against the page background — the gate measures
      `ring`/`background`, and a modal is a different surface.
- [ ] Shift-Tab back through the cycle. **Confirm: the ring tracks backwards
      through the same stops, in reverse.**

---

## Section 3 — Themes and colour

### 3.1 Every surface, all three themes

Cycle dark → light → high-contrast via Settings ▸ Theme, and at each theme walk
every surface below. This is a **look at it** pass, not a token audit — the
contrast matrix is already gated by
`cargo test -p dat0-app --test theme_contrast_gate` (144 measurements, all
green as of 2026-08-08; see `docs/a11y.md` §2).

For **each** of the three themes, confirm the following render correctly, with
no unstyled element, no stuck colour from the previous theme, and no text that
disappears into its background:

- [ ] Title bar (logomark, wordmark, status pill, workspace label)
- [ ] Tab strip, including the ⌘K search affordance
- [ ] Activity rail (48 px, all icons)
- [ ] Sidebar / catalog panel, all three groups, and the session footer
- [ ] Data grid: header, zebra rows, hover row, active row, selected range,
      active cell, fill handle, marching ants
- [ ] Inspector panel column cards
- [ ] Charts panel
- [ ] Connections panel
- [ ] AI panel
- [ ] SQL Console: editor, toolbar, tab strip, results table, timing chip
- [ ] SQL Console transient bars: NL→SQL, Explain, error strip, history
- [ ] Status bar, every segment
- [ ] Banner host: info, warning, error
- [ ] Empty-state hero, including the amber primary button
- [ ] All six modals (Section 5.1)
- [ ] Settings window, all nine sections
- [ ] About box
- [ ] The perf HUD (Section 8)

- [ ] **Confirm: switching theme repaints everything immediately.** A surface
      that keeps its old colours until it is re-opened is a defect.
- [ ] **Confirm: high-contrast is flat.** No shadows, no elevation
      (`"shadow": false` in `high-contrast.json`); a drop shadow there is a
      defect.

### 3.2 File-drop tint — high-contrast MUST be yellow, not blue

This is a **real prior defect class** and gets its own subsection. The
drop-target token composites differently per theme, and the high-contrast theme
has no blue accent at all — its composite is `#333300`, a yellow wash
(`docs/a11y.md` §2.3). A blue tint in high-contrast means the drop zone is
reading the wrong token.

- [ ] Theme: **dark**. Drag a CSV over the hero and hold. **Confirm: the tint is
      blue-grey** (composite `#1a2737`).
- [ ] Theme: **light**. Same drag. **Confirm: the tint is pale blue**
      (composite `#dbe4ee`).
- [ ] Theme: **high-contrast**. Same drag. **Confirm: the tint is YELLOW**
      (composite `#333300`). **A blue tint here is a P0 defect — file it
      immediately.**
- [ ] In all three, confirm the hero text remains readable **through** the tint.
- [ ] Repeat the three-theme drag over the **grid** (not the hero), which is a
      second drop target with its own surface.
- [ ] Drag out without dropping. **Confirm: the tint clears completely.** A
      stuck tint is a defect.

### 3.3 Brand amber, and the ink that sits on it

Light's brand amber is a **dark ochre** (`#985f00`) taking **white** ink; dark
and high-contrast use a bright amber taking near-black ink. Nothing may assume a
side — the ink is always read from `brand(cx).ink_on_brand`.

- [ ] Theme: dark. The hero's `Open demo.dat0` button is **bright amber**
      (`#f5a623`) with **near-black** text.
- [ ] Theme: light. The same button is **dark ochre** (`#985f00`) with **white**
      text. White-on-bright-amber or black-on-ochre is the inversion bug.
- [ ] Theme: high-contrast. The button is **pure yellow** (`#ffff00`) with
      near-black text.
- [ ] Hover the button in all three. **Confirm: the ink stays readable on the
      hover fill.** (Light's hover is *darker* than its base, deliberately.)
- [ ] **Confirm: this is the only amber-filled control in the app.** Every other
      primary button uses `cx.theme().primary`. A second amber button is a
      defect.
- [ ] Theme: high-contrast. **Confirm: the scan-sweep decoration is invisible**
      — it is set fully transparent (`#ffff0000`) there on purpose.

---

## Section 4 — Chrome

### 4.1 macOS menu bar — keyboard traversal

`docs/a11y.md` §3.3 R-row: no harness can drive the AppKit menu bar.

- [ ] Press `Ctrl+F2` (or `Fn+Ctrl+F2`) to focus the menu bar.
- [ ] Arrow through File / Edit / View / Window / Help.
- [ ] **Confirm: every item is reachable, and no item is greyed out that should
      be live** for the current state.
- [ ] Activate one item from each menu by keyboard. **Confirm: each does what it
      says.**
- [ ] **Confirm: every chord shown next to a menu item actually works** when
      pressed outside the menu.

### 4.2 Title bar (UI3) — all eight pill states × three window modes

The complete mapping, from `TitleBarModel::pill` in `src/view/title_bar.rs`:

| # | Condition | Dot | Label |
|---|---|---|---|
| 1 | `SessionSlot::Booting` | amber (`brand.amber`) | `starting` |
| 2 | `SessionSlot::Failed(_)` | red (`theme.danger`) | `engine unavailable` |
| 3 | `EngineStatus::Initializing` | amber | `starting` |
| 4 | `EngineStatus::Ready`, no attachment | green (`brand.ok`) | `local` |
| 5 | `EngineStatus::Ready`, MotherDuck attached | green | `motherduck` |
| 6 | `EngineStatus::Closing` | amber | `closing` |
| 7 | `EngineStatus::Closed` | red | `engine stopped` |
| 8 | `EngineStatus::Failed(_)` | red | `engine unavailable` |

Reach each state:

- [ ] **1** — launch and watch the first frames (Section 1.1).
- [ ] **2** — launch with the session directory unwritable (Section 1.3).
- [ ] **3** — the brief window after the session is `Ready` but before the
      engine reports. If you cannot catch it by eye, open a ≥1 GB file at launch
      to widen it.
- [ ] **4** — steady state after a normal launch, no connections.
- [ ] **5** — attach MotherDuck via Connections. **Confirm the label changes to
      `motherduck`, and the dot stays green.**
- [ ] **6** — `Cmd+Q` and watch the last frames.
- [ ] **7** — detach/close the engine without quitting.
- [ ] **8** — kill the engine underneath a running window (e.g. corrupt the DB
      file mid-session).
- [ ] **Confirm the bar NEVER renders "in development".** That string is the
      marketing page's honesty label for an unshipped product. Its presence is a
      P0 defect.

Now the three window modes. Repeat for each:

**Windowed:**
- [ ] The bar is **44 px** tall.
- [ ] macOS traffic lights sit at **(9, 9)** and do not overlap the logomark.
- [ ] The logomark is a 14 px rounded square in `brand(cx).amber`.
- [ ] The wordmark reads `dat0` in **Geist Mono 700, 17 px**.
- [ ] **Dragging the bar moves the window.**
- [ ] **Double-clicking the bar zooms the window.**
- [ ] The workspace/file label is right-aligned and muted.
- [ ] **No OS window title is drawn** — the bar is transparent and the title
      field was dropped.

**Zoomed** (green button / double-click):
- [ ] All of the above still hold; nothing reflows or clips.

**Fullscreen** (`Ctrl+Cmd+F`):
- [ ] **Confirm: the traffic-light padding collapses** (handled upstream) and
      the logomark moves left to fill it. Leftover 80 px of dead space is a
      defect.
- [ ] Exit fullscreen. **Confirm: the padding comes back.**

**Tab stops:**
- [ ] **Confirm the title bar adds ZERO tab stops.** Tab from the shell root
      through a full cycle and confirm focus never lands in the bar. If it does,
      `docs/a11y.md` §3 must be re-measured — see its §3.13 closing note.

Also verify the **Settings window's** title bar: wordmark plus the section name
only, no status pill.

### 4.3 Tab strip — the ⌘K search stop (UI4)

Unlike the title bar, this **does** add one tab stop, on purpose.

- [ ] The strip is **38 px** tall.
- [ ] At a window width ≥ **1080 px**, the search affordance is **168 px** wide
      and shows a magnifier icon, the search label, and a right-aligned `Kbd`
      chord.
- [ ] **Confirm the chord shown is the real one** — the one that actually opens
      the palette when pressed. A hint that lies is a defect (this is exactly
      what SH4 removed the old `ActionDescriptor.keybinding` field to prevent).
- [ ] Narrow the window **below 1080 px**. **Confirm the affordance collapses to
      icon-only** and does not overlap the tabs.
- [ ] **Tab from the shell root. Confirm the search stop is reachable** and
      shows a focus ring.
- [ ] **Press Enter on it. Confirm the command palette mounts.**
- [ ] Escape the palette. **Confirm focus returns to the search stop**, not to
      the shell root.
- [ ] Press the chord directly (not via the stop). **Confirm the palette opens.**

### 4.4 Status bar (SH1)

The v4 order is: `engine duckdb · native` │ `mem …` │ `rows …` │ selection │
fps │ query chip │ `egress …` │ `⌘K commands`. Height 30 px.

- [ ] **Booting** — no engine badge at all (there is no engine to report).
      A badge here is a defect.
- [ ] **Idle, local** — badge reads `engine duckdb · native`; `mem` shows a
      humanized RSS; `rows` shows the bound table's count; **the fps segment is
      OMITTED entirely.** An `fps —` or `0 fps` in the bar is a defect: the
      em-dash belongs in the HUD, the bar stays quiet.
- [ ] **Running a query** — the query chip appears and shows live timing.
- [ ] **Attached** — with MotherDuck attached, the badge reads
      `engine duckdb · motherduck`.
- [ ] Select a range in the grid. **Confirm `N cells selected` appears** and
      tracks the selection.
- [ ] Scroll the grid. **Confirm the `rows <lo>–<hi> / <total>` segment tracks
      the viewport.**
- [ ] **`egress 0 B` renders in green** (`brand(cx).ok`) on a session that has
      made no network call. **This must be a measured zero, not a constant** —
      verify by doing the next item.
- [ ] Trigger a network call (update check, or attach MotherDuck). **Confirm the
      egress segment leaves zero and stops being green.** If it stays `0 B`
      after a real network call, an egress seam is missing its
      `telemetry::egress::record_sent` call and the privacy claim is false.
      That is a P0 defect.
- [ ] The `⌘K commands` hint is present at the right end and shows the real
      chord.

### 4.5 Sidebar session footer (SH2)

- [ ] Three caption rows are present at the bottom of the sidebar panel.
- [ ] The window/tab counts are correct. Open a second window and confirm they
      change.
- [ ] AI row shows the provider name when configured, and `not configured`
      (`sidebar.ai.none`) when not.
- [ ] Egress row agrees with the status-bar segment.
- [ ] **Confirm the footer adds no tab stops** — it is chrome.

---

## Section 5 — Keyboard: the surfaces automation could not reach

Every subsection here corresponds to a row in `docs/a11y.md` §3.13. Automated
coverage exists for everything **not** listed here; do not re-walk what a test
already drives.

### 5.1 The six modals and their focus traps

The six frozen a11y ids (`src/window/modals.rs:137-142`) — these strings are a
contract the a11y suites assert against and must not change:

| Modal | Frozen a11y id | How to open |
|---|---|---|
| Name prompt | `name-prompt-modal` | Save a view / save-as-table |
| MotherDuck token prompt | `md-token-prompt-modal` | Connections ▸ MotherDuck ▸ connect |
| AI entry prompt | `ai-entry-prompt-modal` | AI panel ▸ set key |
| Export dialog | `export-modal` | View ▸ Export |
| Saved-query picker | `saved-picker-modal` | SQL Console ▸ load query |
| Command palette | `command-palette-modal` | `Cmd+Shift+P` or the tab-strip search stop |

For **each** of the six, in **each** of the three themes:

- [ ] The modal renders with a visible scrim over the shell.
- [ ] **Focus lands inside the modal on open** — you do not have to press Tab to
      get in.
- [ ] **Tab cycles only within the modal** and wraps last → first.
- [ ] **Shift-Tab wraps first → last.**
- [ ] Clicking the shell behind the scrim does **not** move focus out.
- [ ] `Escape` dismisses, **exactly once** — no double-cancel.
- [ ] **On dismiss, focus returns to the control that opened the modal**, not to
      the shell root.
- [ ] **At most one modal is ever open.** Try to open a second (e.g. press the
      palette chord while the export dialog is up): **confirm the chord is
      inert.**

Modal-specific:

- [ ] Export: the format radio group is **one** tab stop, and arrows change the
      selection within it. Four stops total in the cycle.
- [ ] Export: arrows change the scope too; Enter exports with the *selected*
      scope and format.
- [ ] Saved-query picker: arrows move the active row, Enter picks, `Delete`
      removes the active row.
- [ ] Name prompt: focus order is **field → OK → Cancel**.
- [ ] Command palette: typing filters, arrows move and clamp, Enter runs.

### 5.2 SQL Console results pane

`docs/a11y.md` §3.13 R3 — the results pane is an upstream `gpui-component`
`Table`; no dat0 test drives its internal keyboard handling.

- [ ] Run a multi-row query.
- [ ] Tab into the results pane. **Confirm it takes focus with a visible ring.**
- [ ] **Arrow up/down through result rows. Confirm the active row moves and the
      pane scrolls to follow.**
- [ ] Arrow past the last row. **Confirm it clamps** rather than wrapping or
      scrolling into nothing.
- [ ] Tab out. **Confirm focus leaves the pane** and lands on the next control.

### 5.3 Data grid — copy and page-scroll

R1 and R2.

- [ ] Select a range of cells with `Shift`+arrows.
- [ ] Press `Cmd/Ctrl+C`.
- [ ] **Paste into a spreadsheet (Excel / Numbers / Google Sheets). Confirm the
      cells land in the right shape** — the right number of rows and columns,
      tabs between cells, newlines between rows.
- [ ] Paste into a plain text editor. **Confirm cells containing tabs, newlines
      or quotes come back quoted correctly.**
- [ ] Copy a **discontiguous** selection. **Confirm the bounding rectangle is
      what lands**, per the codec's documented behaviour.
- [ ] Copy from a cell showing `—` (an unloaded page). **Confirm you do not get
      a literal em-dash in the clipboard** — this is the PD-018 failure shape.
- [ ] Press `PageUp` / `PageDown` in the grid. **This is not implemented**
      (`grid::keymap::Key` has 16 variants and none is a page-scroll). Confirm
      the keys are inert and do **not** cause a misbehaviour — no crash, no
      partial scroll, no focus loss. If page-scroll is wanted for v1, file it.

### 5.4 Catalog tree — Enter on a leaf, and the three groups

R5 and R6.

- [ ] Tab into the catalog tree. Arrow to a **leaf** (a table, not a group).
- [ ] **Press Enter. Confirm the table opens in a tab.** This path panics under
      the test harness (`tests/catalog_nav.rs:443-450` — `tokio::spawn` with no
      entered runtime) so it is human-only; a panic **here**, in production,
      would be a P0 defect.
- [ ] Arrow to a **parent** and press Enter. **Confirm it toggles collapse**
      rather than opening anything.
- [ ] Collapse a group, quit, relaunch. **Confirm the collapse state persisted.**

The three fixed groups (SH2), in this order:

- [ ] **`FILES`** — registered file tables land here.
- [ ] **`CONNECTIONS`** — an attached MotherDuck table lands here, **not** under
      FILES.
- [ ] **`PACKAGES`** — `.dat0` entries from recents. Activating one opens the
      package.
- [ ] Each group header is an uppercase caption.
- [ ] **An empty group shows its header PLUS one muted empty-state row** —
      `No files open` / `No connections` / `No packages`. A bare header with
      nothing under it is a defect.
- [ ] The default sidebar width is **238 px**. If the three groups are unusable
      at that width, raise it to 280 and record the deviation in SH2's doc
      comment (this is a sanctioned contingency, not a defect).
- [ ] Resize the sidebar. **Confirm the new width persists across a restart.**

### 5.5 Connections panel — keyboard

R7. The render arms are content-asserted; nothing presses Tab into them.

- [ ] Tab into the Connections panel. **Confirm each control takes focus with a
      visible ring, in paint order.**
- [ ] **Press Enter on Connect. Confirm the token prompt mounts** (and run its
      trap checks from §5.1).
- [ ] **Press Enter on Test connection. Confirm the result arm renders** —
      connected, disconnected, or error-with-retry.
- [ ] On the error arm, **confirm Test is hidden and Retry is shown**, and Retry
      is keyboard-operable.
- [ ] **Press Enter on Disconnect. Confirm the attachment drops** and the
      title-bar pill returns from `motherduck` to `local` (§4.2 state 4).
- [ ] Close the panel. **Confirm none of its controls remain tab stops.**

### 5.6 Settings window — all nine sections by keyboard

R10. Six of nine sections are pointer-automated only; pointer operability proves
wiring, not tab order.

Open Settings and, for **each** section, Tab through every control and operate
it with the keyboard alone:

- [ ] Sidebar: **arrow or Tab between the nine section entries**, and confirm
      the content pane follows.
- [ ] **Profile** — name field and author identity fields: reachable, typeable,
      value persists.
- [ ] **Theme** — the cycle selector: reachable, `Enter`/`Space` advances it,
      and **the app repaints immediately**.
- [ ] **Memory Budget** — numeric input: reachable, typeable, persists.
- [ ] **MotherDuck** — token input, Connect, Test connection: all reachable and
      operable.
- [ ] **AI** — provider selector, API key input, Test connection: same.
- [ ] **Telemetry** — toggle reachable and `Space`-operable (automated, but
      confirm the visual state actually flips).
- [ ] **Networked Workspaces** — same.
- [ ] **Updates** — auto-check toggle, **channel selector**, and **Install &
      Restart** button: all reachable and operable.
- [ ] **Advanced** — log-level cycle, version line, and the reset control.
      **Confirm the reset confirm dialog gates the action** — reset must not
      happen until you confirm.
- [ ] After each change, quit and relaunch. **Confirm the setting persisted.**

### 5.7 Inspector and lineage — keyboard

R11 and R12. Both surfaces are content- or pointer-automated only; no test
presses a key into either.

- [ ] Bind a table and open the **Inspector**.
- [ ] Tab into the panel. **Confirm each column card is reachable and shows a
      focus ring.**
- [ ] **Reach the base/view profile toggle by keyboard and operate it with
      `Enter`/`Space`. Confirm the profile actually switches** — the cards must
      recompute, not just the label.
- [ ] **Scroll the column-card list by keyboard.** Arrow or Page keys must move
      the list, and the focused card must stay in view. A card that takes focus
      while scrolled out of sight is a defect.
- [ ] Tab out. **Confirm focus leaves the panel** and no card keeps its ring.
- [ ] Save a chart so a **lineage node** appears in the chain.
- [ ] **Tab to the lineage node. Confirm it is reachable and rings.**
- [ ] **Press Enter on it. Confirm the chart panel reopens with the saved spec
      restored** — the same outcome a click produces.
- [ ] Expand and collapse a lineage step by keyboard. If there is no keyboard
      path at all, that is the finding — file it rather than skipping the row.
- [ ] Close the Inspector and Charts panels. **Confirm neither leaves a tab
      stop behind.**

---

## Section 6 — Docks

### 6.1 Resize, collapse, restore

- [ ] Open each dock (left rail panels, right inspector/charts, bottom console).
- [ ] Resize each by dragging its splitter. **Confirm the content reflows** and
      nothing clips.
- [ ] Collapse each. **Confirm focus lands somewhere live**, not on a hidden
      control.
- [ ] **Confirm a collapsed panel's controls are no longer tab stops** — Tab a
      full cycle and check nothing invisible takes focus.
- [ ] Activate a second left-rail panel. **Confirm the first closes** — one left
      panel at a time.
- [ ] Quit and relaunch. **Confirm every dock's open/closed state and size came
      back.**

### 6.2 Debounced persistence survives a crash (MT2)

`docs/a11y.md` §3.13 R4. `tests/dock_layout_persist.rs` (21 tests) covers the
**quit/close** path. This is explicitly **not** that path: MT2 added a 500 ms
debounced poll precisely because `Dock` is not an `EventEmitter` and a resize
emits nothing, so before MT2 a crash lost every dock size.

- [ ] Launch. Resize the left dock to an obviously non-default width.
- [ ] **Wait at least 2 seconds.** Do not quit. Do not close the window. Do not
      touch anything else.
- [ ] From another terminal, find the pid and `kill -9 <pid>`.
      **`SIGKILL`, not `SIGTERM`** — `SIGTERM` may run the graceful path and
      would prove nothing.
- [ ] Relaunch. **Confirm the dock came back at the width you set.**
      If it came back at the default, the debounce never fired and MT2 is not
      working — file it.
- [ ] Repeat for the **bottom console height** and the **right dock width**.
- [ ] Negative control: resize a dock and `kill -9` **within 200 ms**, before the
      debounce. The old size coming back is *expected* here, not a defect — this
      confirms you are actually testing the debounce and not a synchronous write.

---

## Section 7 — Dialogs that only exist at runtime

### 7.1 The `open_dialog` layer — same-machine, conflict, live-refresh

`docs/a11y.md` §3.13 R8 and R13. These three dialogs are painted through
`Root::render_dialog_layer`, a **separate layer** from the shell's modal
overlay (`src/window/render.rs:695-703`), so the a11y capture never sees them
and none of the automated modal-trap assertions apply.
`tests/single_instance.rs` proves the UDS message dispatches exactly once to
the main thread; it cannot prove a **visible** dialog. Also owed as
`docs/plans/2026-06-17-dat0-p10a-uat.md` §2.5.

- [ ] Launch the app and leave it running.
- [ ] From a second shell, launch it again with no arguments.
- [ ] **Confirm the second invocation exits** rather than opening its own
      process window.
- [ ] **Confirm a second window appears in the FIRST instance, in front and
      focused.**
- [ ] Repeat with a file argument. **Confirm the file opens in a new window of
      the first instance.**
- [ ] Where the same-machine-in-use dialog is shown (a locked workspace),
      **confirm its buttons take focus and respond to Enter and Escape.**
- [ ] **Concurrency-conflict dialog (P7b).** Open the same workspace from a
      second machine (or forge a foreign `lock.json` holder) so
      `workspace_in_use_modal::open_conflict_dialog` fires.
      **Confirm the dialog is VISIBLE** — it is painted through
      `Root::render_dialog_layer`, a different layer from the shell's modal
      overlay, which is exactly the wiring that failed silently in PD-021's
      shape. **Confirm its buttons take focus, respond to `Enter`, and that
      `Escape` dismisses.**
- [ ] **Live-refresh confirm dialog.** Modify a registered source file on disk
      while the app is watching it. **Confirm the confirm dialog appears, is
      keyboard-operable, and that accepting it actually refreshes the grid.**
- [ ] For both dialogs, **confirm focus returns to a live control on dismiss** —
      these are not `mounted_modals` entries, so the automated
      restore-focus assertions in `docs/a11y.md` §3.12 do not cover them.
- [ ] Quit the first instance, then launch again. **Confirm a fresh instance
      starts** — the lock file was released, not orphaned.

### 7.2 Recovery panel sheet

R9. `tests/recovery_panel.rs` covers the scan/discard/open **logic** in 4 tests.
No test mounts the sheet.

- [ ] Create orphaned session directories (kill the app mid-session a few times).
- [ ] Relaunch. **Confirm the recovery banner appears with the correct orphan
      count.**
- [ ] Open the recovery panel. **Confirm each orphan row is keyboard-reachable
      and its Open / Discard buttons are Enter-operable.**
- [ ] Discard one. **Confirm the row disappears and the directory is gone from
      disk.**
- [ ] Open one. **Confirm the session loads.**
- [ ] Confirm the sheet dismisses on `Escape` and returns focus.

---

## Section 8 — Perf HUD (MX1)

The HUD is the instrument every performance claim in the README and on the
marketing page is measured against. If it shows nothing, nothing is measured.

- [ ] Open the **command palette** and run **`Toggle performance HUD`**
      (action id `perf.hud.toggle`).
- [ ] **Confirm the HUD appears**, pinned bottom-right, over the shell.
- [ ] **Confirm all four lines show live values, not placeholders:**
  - [ ] `<n> fps`
  - [ ] `p50 <n> / p95 <n> / p99 <n> ms`
  - [ ] `rss <humanized>`
  - [ ] `pages <resident> / <cap>`
- [ ] Open a **≥1 GB** file and **scroll the grid hard**.
      **Confirm every number moves** — fps dips, the percentiles rise, RSS
      grows, and pages-resident climbs toward the LRU cap.
      A number frozen while scrolling means it is not wired.
- [ ] Stop scrolling and **wait more than 500 ms** (`FrameClock::IDLE_AFTER`).
      **Confirm the fps readout becomes an em-dash `—`.**
      **It must NEVER read `0`.** Zero is a measurement; the em-dash is the
      honest answer when there is nothing to measure. `0 fps` here is a defect.
- [ ] Scroll again. **Confirm fps comes back from `—` to a live number.**
- [ ] Sit on the **empty-state hero** with the HUD open and do nothing (SH3).
      **Confirm the frame interval is flat.** A sawtooth here means the hero is
      still reading `recents.json` / `settings.toml` from disk every frame,
      which is exactly what SH3 fixed.
- [ ] Confirm the HUD is **not a tab stop** — Tab a full cycle with it open.
- [ ] Toggle it off. **Confirm it disappears completely.**
- [ ] Confirm the HUD renders legibly in all **three themes**.

---

## Section 9 — Update check against the production key

The embedded pubkey in `crates/dat0-app/assets/minisign-public-key.txt` was
byte-identical to the test fixture until RL1 step 1. This walks the real chain.

- [ ] Confirm `cargo test -p dat0-app --test update_key_is_production` **passes**
      — it fails by design until the production key is generated. If it still
      fails, RL1 step 1 has not been done and the rest of this section is
      meaningless.
- [ ] Launch and run **Help ▸ Check for Updates**.
- [ ] **Confirm the checking state renders** (an alert, not a frozen menu).
- [ ] With no newer release available: **confirm "You are up to date"** — no
      crash, no error banner.
- [ ] With a newer release published: **confirm the update prompt renders** with
      the new version, and offers Install and Later.
- [ ] **Confirm the manifest signature is verified against the production key.**
      Point the client at a manifest signed with a *different* key and
      **confirm it is REJECTED**. Acceptance here would mean the client trusts
      anything.
- [ ] Mutate one byte of a validly signed manifest. **Confirm rejection.**
- [ ] Press **Later**. **Confirm the prompt dismisses and does not immediately
      reappear.**
- [ ] Press **Install**. **Confirm the download runs, the signature is verified,
      and the app restarts on the new version.**
- [ ] **Confirm the update check moved the status bar's egress segment off
      `0 B`** (§4.4). If it did not, `update/check.rs` is missing its
      `// egress-seam` record and the privacy claim is false.
- [ ] Run a background (non-manual) check. **Confirm it is silent when up to
      date and silent on error** — a background check must not interrupt.

---

## Section 10 — Closing: clean VM, and filing what you found

### 10.1 Run it on a clean VM

Everything above must be walked on a machine where dat0 has **never** run.
Follow the same procedure as `docs/plans/2026-06-17-dat0-p10a-uat.md` §1:

```bash
# Clone the gold image to a throwaway VM (see docs/ci-mac-vm-runner.md)
tart clone dat0-runner-base dat0-uat-v1
tart run dat0-uat-v1
```

A real machine where dat0 has never been installed is equally valid. A machine
with an existing `settings.toml`, `recents.json` or session directory is **not**
— first-run defects are invisible there, and Sections 1, 5.4 and 7.2 all depend
on genuinely empty state.

- [ ] The macOS pass ran on a clean VM or a never-installed machine.
- [ ] The Linux pass ran on a clean Ubuntu 24.04 VM or container.
- [ ] `docs/plans/2026-06-17-dat0-p10a-uat.md` (signing, notarization, install,
      double-click, artifact verification) was walked in the same session —
      it is a prerequisite, not an alternative.

### 10.2 File every defect before declaring done

**This plan is not complete until every defect found here is a written entry in
`docs/deferrals.md`.** A defect that lives only in a chat message is a defect
that ships.

Numbering, as of 2026-08-08:

- **`D-` (deferrals):** D-030, D-031 and D-032 are taken. **New deferrals start
  at D-033.**
- **`PD-` (plan defects):** PD-022 is the highest in use. **New plan defects
  start at PD-023.**

Check the file before you write — this checklist is being run after several
concurrent slices, so both counters may have advanced.

For each defect record: the section number here, what you did, what you saw,
what you expected, the theme and window mode if relevant, and a severity.
Severity `high` is reserved for the failure mode this document exists to catch:
**logic green, screen dead.**

### 10.3 Fold the results back into `docs/a11y.md`

`docs/a11y.md` §3.13 lists ten UAT-pending rows (R1–R10) and a closing note
naming four surfaces that had no automated a11y coverage when it was written —
the title bar (UI3), the tab-strip search stop (UI4), the v2 status bar (SH1)
and the sidebar groups and footer (SH2).

- [ ] For every R-row this run **passed**, update its §3 row from
      **UAT-pending** to **UAT-verified**, dated, with the section number here
      as its evidence.
- [ ] For every R-row this run **failed**, leave it pending and link the new
      `PD-` number.
- [ ] Fold the four post-audit surfaces into §3 with whatever automated coverage
      landed with them, and delete the closing note once it is no longer true.
- [ ] Re-run `cargo test -p dat0-app --test theme_contrast_gate` and refresh
      §2 and §5 of `docs/a11y.md` if any palette token moved during this run.
- [ ] Only then flip A1 in `docs/a11y.md` §1 from
      "Substantially automated; 9 rows still UAT-pending" to **PASS**.

### 10.4 Sign-off

- [ ] Every box above is `[x]` or carries a `PD-` number.
- [ ] Every `PD-` number written here exists in `docs/deferrals.md`.
- [ ] Date, machine, OS version and build SHA recorded below.

```
Run by:
Date:
macOS VM:            build SHA:
Linux VM:            build SHA:
Defects filed:
```
