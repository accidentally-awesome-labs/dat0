# dat0 production v1 — manual UAT checklist

> Rewritten 2026-09-26 for the Dioxus build (`crates/dat0-ui`, with the
> non-UI logic in `crates/dat0-core`). It replaces the 2026-08-08 checklist
> (slice AX2), which walked the GPUI build: `crates/dat0-app`, deleted since,
> last seen at commit `95627c8`.
> Companion gate document: `docs/a11y.md`, re-audited 2026-08-13 against the
> Dioxus build (§11.3 says what to fold back into it).
> What this walks: every feature the "Progress" log of PD-023 in
> `docs/deferrals.md` records as restored, and the "Works in the current
> build" column of `README.md`'s Status table.
> Predecessor, still owed and **not** superseded:
> `docs/plans/2026-06-17-dat0-p10a-uat.md` (signing, notarization, install).
> Checked against the code at commit `6a4e66d`: PD-023's steps 5.1 to
> 5.11, after which PD-023 closed. What it left open is PD-033 to PD-040,
> each named beside the check that meets it; a result worse than its entry
> says is a new defect.

## Why this exists

The failure this document exists to catch is **"logic green, screen dead"**:
the code is correct, the tests are green, and the feature does nothing when a
human drives the app.

**PD-023 is the flagship.** The GPUI→Dioxus port rebuilt the components and
the shell chrome, but not the orchestration that connected them to
`dat0-core`: of 40 registered actions, 22 only logged, opened an empty dialog
or threw their reply away. Run in the SQL console only cleared the error;
packages and workspace folders were refused as files dat0 could not read;
the status bar's `egress 0 B` was a field nothing wrote. Every gate stayed
green, because each asserted that an id was claimed or a queue filled, not
that the screen changed. A project review found it by driving a Linux release
build. It had happened before — PD-016 (a funnel that did nothing), PD-017
(edits that always failed), PD-018 (`—` in every cell), PD-021 (banners
nothing drained) — and kept happening while PD-023 was paid down: PD-024
(banners raised after the first frame, never shown) and PD-032 (only the
first palette or prompt took the keyboard), each found by driving the
release build.

So the instructions below say **look at the screen**, not **assert the
state**. Every item is worded so that "it didn't visibly happen" is a
**failure**, not an ambiguity.

---

## What changed from the GPUI checklist

| GPUI checklist item | Dropped or replaced, and why |
|---|---|
| `cargo build -p dat0-app` with `DEVELOPER_DIR`/`SDKROOT` | The binary is `dat0` in `dat0-ui`, and a webview needs no Metal toolchain: the Xcode Command Line Tools are enough (`CONTRIBUTING.md`). |
| Samples in `crates/dat0-app/assets/` | They are in `crates/dat0-core/assets/`, and compiled into the binary for the hero. |
| Title-bar pill with eight engine states (`titlebar.*` strings) | The Dioxus pill reads `local`, `live` or `read-only` and reports no engine state; an engine failure shows in the status bar and a banner. |
| Traffic lights at (9, 9), 14 px logomark, no OS title | The lights are inset to (12, 18) over an 88 px spacer, the mark is a 16 px square, and Linux keeps its window-manager title bar. |
| Activity rail (48 px), one left panel at a time | Gone; the sidebar is one column with three sections. |
| Six frozen modal ids in `src/window/modals.rs` | One modal slot (`Modal` in `state.rs`, 14 variants) with `data-a11y-id="modal"` and a slug label. |
| "Two rings means a stale `FocusHandle`" | Focus is one CSS rule (`:focus-visible` in `app.css`); the one-ring check stays for other reasons. |
| SQL Console results pane (gpui-component `Table`) | The console has no results pane; rows land in the main grid. |
| `Root::render_dialog_layer` dialogs outside the trap | Workspace-in-use and Live Refresh are in the modal slot, with the same trap as every dialog. |
| MT2's debounced dock-size poll | The layout is a signal, written to the window's session 500 ms after its last change (`LAYOUT_DEBOUNCE`, `session_boot.rs`). |
| `docs/a11y.md` §3.12, §3.13, rows R1–R13 | They no longer exist; that document has sections 0–5 now. |
| `cargo test -p dat0-app --test theme_contrast_gate` | The gate is `cargo test -p dat0-core --test theme_tokens_contrast`. |
| High-contrast drop tint "MUST be yellow", ochre and yellow buttons, scan sweep | High contrast is now a light theme with a navy accent; the tint is the accent in every theme; amber and its ink are the same in all three; there is no sweep. |
| Fill handle, marching ants | The Dioxus grid draws neither (the `sel-edge-*` rules in `app.css` are applied by nothing). |
| PageUp/PageDown "inert" (`grid::keymap::Key`) | The grid does not bind them; the viewport scrolls natively. |
| Settings → Updates channel selector, Install & Restart | That section has one toggle; Install & Restart is in the update prompt. |
| Export dialog "four stops" | Two radio groups, a folder, a name, Export and Cancel. |
| Status bar in the v4 order | Step 5.8c redrew it (`status_bar.rs`): engine state, memory, rows, selection and the last query, then egress and the palette hint; the frame rate moved to the perf HUD. |
| "Toggle performance HUD" | The palette row is "Toggle Performance HUD"; no menu item, no chord. |
| `update_key_is_production` in `dat0-app`; "You are up to date" | The test is in `dat0-core`, `#[ignore]`d; the prompt says `dat0 is up to date.` |
| dat0's own About box | Nothing opens `Modal::About`; dat0 → About dat0 is the native panel. |
| Catalog Enter "panics under the harness" | The FILES rows are the window's tabs; Enter shows one. |
| Launcher collapsing to icon-only below 1080 px | The launcher narrows to 168 px and keeps its text; the sidebar hides. |

---

## Prerequisites

### Machines, and where dat0 keeps things

- A **clean machine** per platform (§11.1). A machine where dat0 has run has
  a `settings.toml`, a `recents.json`, a `scratch/` folder and perhaps
  keychain entries, all of which hide first-run defects.
- Several checks stage or remove files in dat0's folders:

  | What | macOS | Linux |
  |---|---|---|
  | `settings.toml`, `recents.json` | `~/Library/Application Support/dat0/` | `~/.config/dat0/` |
  | State root: `scratch/`, `inspect/`, `samples/`, `demo/`, `last-crash.json` | the same folder | `~/.local/share/dat0/` |
  | The log, `dat0.log` (Settings → Advanced → Open logs folder) | `~/Library/Caches/dat0/logs/` | `~/.cache/dat0/logs/` |

- `DAT0_CONFIG_DIR` relocates the config folder (on macOS the state root with
  it; on Linux the state root follows `XDG_DATA_HOME`), for re-running one
  section from fresh state. It does not relocate the OS keychain, where AI
  keys and the MotherDuck token live.

### The build

macOS needs the Xcode Command Line Tools (`xcode-select --install`). Linux
(Ubuntu 24.04) needs `CONTRIBUTING.md`'s list, the one CI installs:

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libsoup-3.0-dev libxdo-dev \
  libsecret-1-dev dbus-x11 gnome-keyring \
  libpango1.0-dev libfontconfig1-dev \
  libxkbcommon-dev libxkbcommon-x11-dev \
  libssl-dev
```

Both platforms, with the pinned Rust 1.97.0 (`rust-toolchain.toml`):

```bash
cargo build --release -p dat0-ui --bin dat0
RUST_LOG=info ./target/release/dat0 2>&1 | tee ~/dat0-uat.log
```

- The log goes to standard output and to `dat0.log` (the table above); attach
  the relevant lines to each defect.
- Record `git rev-parse --short HEAD`. Settings → Advanced shows
  `dat0 <version> (<sha>)`; the two must match.
- A local build carries the stub crash-report address from
  `.cargo/config.toml` (`https://stub@glitchtip.invalid/1`), so Send Report
  reaches nothing from it (§10.24). A CI release build carries the real one.

### Platform setup

- **macOS: keyboard navigation.** WebKit sends Tab to plain buttons only
  when System Settings → Keyboard → Keyboard navigation is on. Run §2 and §5
  with it on; then run §2's Tab cycle once with it off, and record every
  control Tab no longer reaches.
- **Linux: a desktop session**, not a bare container. The file and folder
  pickers go through the XDG desktop portal (`rfd` 0.15 with its default
  features): with no `xdg-desktop-portal` and a file-chooser backend running,
  every picker returns nothing and nothing says why.
- **Linux: an unlocked keyring** (Secret Service). Without one, AI keys and
  the MotherDuck token last only until dat0 quits (§10.21).
- **Network** you can switch off and on (§9, §10.22, §10.23).

### Test data

- `crates/dat0-core/assets/iris.csv` — 150 rows × 5 columns, `sepal_length`,
  `sepal_width`, `petal_length`, `petal_width` and `species` (the hero's
  **Iris** card). `crates/dat0-core/assets/chinook.sqlite` — 11 tables (the
  **Chinook** card).
- `crates/dat0-core/assets/demo.dat0` — what `▶ Open demo.dat0` unpacks:
  tables `customer`, `genre`, `invoice`, `invoice_line`, `track` and
  `revenue_by_genre` (made by SQL); tab views `genre` (with `name` renamed
  `Genre`) and `revenue_by_genre`; a saved query `Top customers`; a saved
  chart `Revenue by Genre`; no replay sources.
- A large file: `cargo run --release -p dat0-fixtures -- --out ~/dat0-uat/large`
  writes `generated.csv` (1 GiB), `generated.parquet` and `generated.sqlite`.
- In `~/dat0-uat/`: `a/data.csv` and `b/data.csv` (same name, different rows);
  `live.csv` (a copy of iris.csv to edit while open); `latin1.csv` (a few rows
  including `café`, saved as Latin-1); `sales.csv` and `sales-v2.csv` (columns
  `region,amount`, different rows) and `other.csv` (other columns); and
  `mixed.csv`, whose delimiter changes part-way, so DuckDB's two samples of it
  disagree:
  `{ echo "id;name;city"; for i in $(seq 5000); do echo "$i;n$i;c"; done; for i in $(seq 60000); do echo "$i,n$i,c"; done; } > ~/dat0-uat/mixed.csv`
- A MotherDuck token, and an AI key (Anthropic, OpenAI or OpenRouter) with a
  model name it can use.
- `docs/deferrals.md` open in an editor. File defects **as you find them**
  (§11.2).

### Conventions

**Recording.** Every box gets `[x]` (observed, matches) or a `PD-` number
written beside it. An unchecked box at the end of the run means the run is not
finished. There is no third state. A box that names an open entry
(`PD-038`, say) is `[x]` when what you saw matches that entry.

**Chords** are written macOS-first: ⌘ is Ctrl on Linux, ⇧ is Shift, ⏎ is
Enter. The one keymap is `crates/dat0-core/src/keymap.rs`, and a hint on
screen must name this platform's chord: `⌘⇧Z` on macOS, `Ctrl+Shift+Z` on
Linux. **Strings** in backticks are quoted from
`crates/dat0-i18n/src/strings/en.json` or the source file named beside them.

**Order.** Run the sections in order; §1–§3 need a first run. §6.2, §7.2
and §7.4 use `uat-ws`, the workspace §10.10 saves: make it as
`~/dat0-uat/uat-ws` (File → Save Workspace… on a window holding iris.csv)
when first asked for. Dismiss banners (✕) between checks, so an old one is
not taken for a new one.

---

## Section 1 — Cold start

A window exists before its DuckDB session does: it opens `Booting` and the
session lands when built (`session_boot.rs`). That is the moment a test that
waits for "ready" never sees.

### 1.1 The booting shell is visible and honest

- [ ] Launch on a machine with no prior state. **Confirm: a window appears at
      once** — title bar, tab strip, sidebar, status bar — not a blank or
      white rectangle.
- [ ] **Confirm the first-run hero**: `Your data outgrew the spreadsheet.`,
      `It never outgrew your laptop.`, `Take a tour`, `Try the demo workspace`
      with the amber `▶ Open demo.dat0`, `Or start from a sample:` with the
      Iris, Chinook and NYC taxi cards, and `Open file…` — all present tense:
      no "concept render", "not shipped", "coming soon" or "in development".
- [ ] **Confirm: the tour opens by itself** (`Take a tour`, `Panel 1 of 7`).
      Skip it; quit; relaunch. **Confirm: no tour, and no first-run band.**
- [ ] **Confirm: while the engine opens, the drop area reads `Opening the
      data engine…`** over two skeleton bars, and the privacy line stays:
      `nothing imported · 0 bytes left this machine` in green until anything
      leaves. (Passing the 1 GiB CSV on the command line makes this longer.)
- [ ] Capture the first-run hero as `docs/img/first-run-hero.png` (macOS);
      `README.md` says this pass owes it.
- [ ] Click the **Iris** card. **Confirm: a tab `iris.csv`, 150 rows, columns
      `sepal_length`, `sepal_width`, `petal_length`, `petal_width`,
      `species`, the species named** (`setosa`, not `0`).
- [ ] In a new window's hero, click **NYC taxi**. **Confirm: `Sample data
      download failed` with the reason and `Retry`**, not a hang (PD-012: the
      checksum is still a placeholder).

### 1.2 A file dropped during boot must queue, not vanish

- [ ] Relaunch. While `Opening the data engine…` shows, drag iris.csv onto
      the window. **Confirm: the drop tint shows (§3.2), and the file opens by
      itself once the engine is ready** — no second drag, no error.
- [ ] Drop **two** files in quick succession during boot. **Confirm: both
      open, in the order dropped, the second one active.**
- [ ] `dat0 ~/dat0-uat/a/data.csv ~/dat0-uat/b/data.csv`. **Confirm: two
      tabs named `data.csv`, each showing its own file's rows** (PD-030).

### 1.3 Boot failure degrades, it does not hang

dat0 will not start without a writable state root, so break only `scratch/`.

- [ ] Quit. `chmod 000 "<state root>/scratch"`; launch. **Confirm: a window
      with the error banner `Could not open this window's data engine`**, the
      error text, `Try again` and no ✕; **the status bar's dot is red.**
- [ ] **Confirm: nothing retries by itself or hangs.** Drop a file: nothing
      opens, then or later.
- [ ] Still broken, press `Try again` twice. **Confirm: one failure banner,
      not three.**
- [ ] `chmod 755` it; press `Try again`. **Confirm: the dot turns green, a
      dropped file opens, and the failure banner goes.**

### 1.4 Quit during boot

- [ ] Launch and quit at once (⌘Q; Ctrl+Q on Linux).
      **Confirm: the process exits** rather than waiting on the session.

---

## Section 2 — Focus indicators

`docs/a11y.md` §1 marks A2 **PASS** on measured colour: one rule,
`:focus-visible { outline: 2px solid var(--d0-accent); outline-offset: -1px }`
in `crates/dat0-ui/assets/app.css`, whose accent the gate measures (`#03459b`
light, `#58a6ff` dark, `#00337a` high contrast). The grid's active cell and
the launcher draw the same accent as a 2 px inset. The gate cannot measure
**geometry** — whether a ring is drawn, visible and uncovered.

- [ ] Theme: light
- [ ] Theme: dark
- [ ] Theme: high contrast

For each theme, with iris.csv open and the console, inspector and chart
panes open (⌘⇧C; View → Toggle Inspector; View → Visualize):

- [ ] Tab through the whole window. **Confirm: at every stop a ring is
      visibly drawn**, not clipped or covered by a splitter, the grid header,
      a pane edge, the status bar or a dialog border.
- [ ] **Confirm: you can always tell where the keyboard is.** The grid's
      active cell keeps its accent box when the grid is not focused; if that
      reads as a second ring, file it.
- [ ] Click a cell. **Confirm: no ring round the grid** — `:focus-visible` is
      for keyboard focus.
- [ ] In the Export dialog (⌘E), Tab. **Confirm: the ring reads against the
      dialog's surface.** Shift-Tab: **the ring retraces its stops.**

---

## Section 3 — Themes and colour

### 3.1 Every surface, all three themes

Settings (dat0 → Settings…) → Theme has one button, `Theme: light`, which
cycles light → dark → high contrast and tells every window. The palette's
`Toggle Theme` switches light ↔ dark. The theme colours are gated by
`cargo test -p dat0-core --test theme_tokens_contrast` (`docs/a11y.md` §2);
this is a **look at it** pass.

For **each** theme, confirm each surface has no unstyled element, no colour
left from the last theme, and no text lost in its ground:

- [ ] Title bar, tab strip, sidebar and footer; the hero (first-run band,
      drop area, green privacy line, sample cards, recent list)
- [ ] Grid (sort and funnel marks, selection, active cell, `NULL`, `—`),
      filter popover, context menu, cell editor, pipeline bar, and banners
      with their 3 px left edge: info (accent), warning, error (red)
- [ ] SQL console (syntax colours, error and AI strips), inspector, chart
      pane, status bar, palette, perf HUD, every dialog in §5.1, and the
      Settings window with its reset confirmation
- [ ] **Confirm: a theme switch repaints everything at once.** A surface that
      keeps its old colours until reopened is a defect.
- [ ] **Confirm: high contrast is flat** — no shadows (`"shadow": false` in
      `crates/dat0-core/src/theme/builtins/high-contrast.json`).

### 3.2 File-drop tint

The whole window is the drop target. A held file gives it a 2 px inset
border in the accent and a 6 % accent wash (`.d0-window.is-drag-over::after`).

- [ ] Hold a CSV over the hero. **Confirm the border: dark blue `#03459b`
      (light), light blue `#58a6ff` (dark), navy `#00337a` (high contrast)**,
      with the hero text readable through the wash.
- [ ] Repeat with a tab open, over the grid. **Confirm the same.**
- [ ] Drag out without dropping. **Confirm: the tint clears completely.**
      Drop text dragged from another app: **nothing opens, nothing errors.**

### 3.3 The one amber control

- [ ] In each theme, **confirm `▶ Open demo.dat0` is amber `#f5a623` with ink
      `#101318`** (`#ffb733` on hover) — the same in all three (`.d0-cta`).
- [ ] **Confirm no other button is amber-filled.** The title bar's amber
      square is a mark, not a control.

### 3.4 A theme kept across launches (steps 5.9, 5.11e)

- [ ] Two windows, both light. Palette → `Toggle Theme` in one.
      **Confirm: both turn dark.**
- [ ] Settings → Theme, three presses. **Confirm: every window
      follows each press, and the button names the theme shown.**
- [ ] Choose dark with the toggle; quit; relaunch. **Confirm: the
      window opens dark.** Repeat, choosing high contrast in Settings.
- [ ] File → New Window. **Confirm: it opens in the chosen
      theme.** From high contrast, `Toggle Theme`: **it goes to light.**
- [ ] With Settings open, toggle from a workbench window.
      **Confirm: the Settings window repaints, and its button names the new
      theme.**
- [ ] Relaunch with dark kept. **Confirm: no light flash before
      the first frame.**
- [ ] Settings → Advanced → Reset all settings to defaults →
      Reset; relaunch. **Confirm: the window opens light.**

---

## Section 4 — Chrome

### 4.1 The menu bar

`crates/dat0-ui/src/menu.rs` builds it with muda: one bar for the process on
macOS, a GTK bar in each window on Linux. Each dat0 item's id is its action
id; nothing is built disabled (`menu::UNWIRED_LOCAL` and `router::UNWIRED`
are empty); chords come from the keymap.

| Menu | Items |
|---|---|
| dat0 | About dat0 · Settings… · Hide dat0, Hide Others, Show All (macOS only) · Quit dat0 ⌘Q |
| File | New Window · Open File… · Open Workspace… · Open .dat0 Package… · Open Recent ▸ (when there are recent workspaces) · Save Workspace… · Export as .dat0 Package… · Unpack .dat0 Package… · Replay .dat0 Package… · Export… ⌘E · Close Window ⌘W |
| Edit | Undo ⌘Z · Redo ⌘⇧Z · Cut · Copy · Paste · Select All |
| View | Toggle Catalog ⌘B · Toggle Inspector · Visualize · Toggle SQL Console ⌘⇧C · Run ⌘⏎ · Cancel ⌘. · AI Providers… · Connections… · full screen (⌃⌘F on macOS; `Toggle Full Screen`, F11, on Linux) |
| Window | Minimize (⌘M on macOS) · Zoom |
| Help | Take a Tour · Report a Bug… · Check for Updates… · Documentation · dat0 on GitHub |

On macOS, Quit, Close Window, Minimize, Zoom and full screen are AppKit's own
items. muda builds none of them on Linux, so there they are dat0's
(step 5.11d), with Ctrl+Q, Ctrl+W and F11 from the keymap; Hide, Hide Others
and Show All are macOS only.

- [ ] Focus the menu bar (macOS Ctrl+F2; Linux F10) and arrow through it.
      **Confirm: every item above for this platform is there, and none is
      greyed out.** Linux: **Ctrl+W closes the window focused last, Minimize
      and Zoom act on it, F11 toggles full screen, and Ctrl+Q quits** — each
      with a dialog up too.
- [ ] Activate one item from each menu by keyboard. **Confirm: each does what
      it says** (§10 walks them all). **Confirm: every chord shown works when
      pressed outside the menu, and acts once** (§5.6).
- [ ] Help → Documentation and dat0 on GitHub. **Confirm: the browser opens
      `https://dat0.app/docs` and the repository.** dat0 → About dat0:
      **the platform's panel names `dat0` and its version.**
- [ ] File → Open Recent. **Confirm: recent workspaces by folder name, newest
      first, at most ten.** One opened or saved this session appears from the
      next launch (PD-036).
- [ ] Two windows: use a menu item. **Confirm: it acts in the window focused
      last, once.** Close the first window: **the menu still acts** (PD-027).

### 4.2 Title bar

`TitleBar` in `crates/dat0-ui/src/components/shell.rs`: an 88 px spacer for
the macOS traffic lights, the wordmark `dat` with a 16 px amber square, the
window's name in muted mono, and a pill reading `read-only`, `live` or
`local`. On Linux it sits below the window manager's title bar (`dat0` in
every window) and the GTK menu bar. The Settings window has none: it is a
decorated window titled `Settings`.

- [ ] **Confirm: the bar is 44 px tall; on macOS the traffic lights sit in
      the spacer, clear of the wordmark.** Tab through the window: **focus
      never lands in the bar.**
- [ ] **Confirm the name**: `scratch` in a new window; the folder's name for
      a workspace (and at once after Save Workspace, §10.10); the file's name
      without `.dat0` for a package. **Confirm the pill**: `local`, or
      `read-only` in a package window.
- [ ] Connect MotherDuck (§10.23). **Confirm: the pill reads `live`**, and
      `local` again after Disconnect.
- [ ] macOS: **confirm dragging the bar moves the window, and double-clicking
      it zooms** (step 5.11d). In full screen, **record** whether the spacer
      leaves a gap. Linux: dat0's bar sits under the window manager's; a
      drag on it moves the window too.

### 4.3 Tab strip and the launcher

- [ ] **Confirm: the strip is 38 px tall, and starts with the launcher**, as
      wide as the sidebar (238 px by default), reading `search tables,
      queries…` and **`⌘K` on macOS, `Ctrl+K` on Linux.** Press that chord:
      **the palette opens** (step 5.8b); ⌘⇧P opens it too.
- [ ] Tab to the launcher. **Confirm: its accent focus shows, Enter opens the
      palette, and Escape returns focus to the launcher.**
- [ ] Narrow the window below 1080 px. **Confirm: the sidebar hides, the
      launcher narrows to 168 px with its text and chord, and the grid takes
      the sidebar's width** (step 5.11g: the grid used to vanish). Widen it
      again: the sidebar returns.
- [ ] **Confirm tab titles**: a file's name with its swatch; `chinook_Album`
      for a SQLite table; `Query 1` for a query's rows; the table's name for a
      table saved from a view or a query.
- [ ] Switch tabs by keyboard: **the strip's tabs cannot be reached
      (PD-040)**; the sidebar's FILES rows are the route (§5.4). A tab has no
      close control (PD-038).

### 4.4 Status bar

`StatusBar` (`crates/dat0-ui/src/components/status_bar.rs`) and
`crates/dat0-ui/src/chrome.rs`. At the left, a dot and the engine's state,
the memory, the rows in view, the selection and the last query (step 5.8c);
at the right, the egress figure and `<chord> commands`.

- [ ] **Confirm: 30 px tall, pinned to the bottom; the dot green while the
      window's engine is fine and red when it failed** (§1.3). With Reduce
      Motion on, it stops pulsing.
- [ ] **Confirm the engine reads what it is doing**: `engine duckdb ·
      starting` with an amber dot while the session opens, then `· native`;
      `· motherduck` while MotherDuck is connected (§10.23); `· failed` with
      the red dot (§1.3).
- [ ] **Confirm `mem <n> MB`**, the process's resident set, moving as you
      open and scroll the 1 GiB table and settling when idle.
- [ ] Open iris.csv. **Confirm `rows 1–<n> / 150`**, following the grid as it
      scrolls; `0 rows` for an empty result; nothing with no tab on screen.
      Select a 3 × 2 range: **`6 cells selected`**; one cell: `1 cell
      selected`.
- [ ] Run a query (§10.2). **Confirm `Query running…` while it runs, then
      `Query <n> ms`.**
- [ ] Settings → Updates: turn off `Automatically check for updates at
      launch`; quit; relaunch; do nothing that goes out. **Confirm: `egress 0
      B`, in green** — green only while nothing has left (step 5.11a).
- [ ] Help → Check for Updates…. **Confirm: the figure leaves `0 B` as the
      check starts, network on or off** (the request is counted when made,
      `crates/dat0-core/src/update/check.rs`), **in every window, and the
      sidebar footer and AI panel show the same figure.** If it stays `0 B`,
      the privacy claim is false: file it as `high`. **Confirm: the figure is
      no longer green, in the status bar or the sidebar.**
- [ ] Connect MotherDuck (§10.23). **Confirm: a `+` follows the figure**
      (`egress 1.2 KB+`) and stays after Disconnect: MotherDuck's own
      connection is not metered (D-033), so the figure is a floor.
- [ ] Turn the launch check back on (the default); quit; relaunch; open
      nothing. **Confirm: the hero's line says what the status bar does** —
      `nothing imported · 412 B left this machine` beside `egress 412 B`, say
      — **and neither is green.** A hero still saying `0 bytes` after the
      check is the constant step 5.11a replaced: file it as `high`.
- [ ] **Confirm the right end reads `⌘K commands` (macOS) or `Ctrl+K
      commands` (Linux)**, and that chord opens the palette.

### 4.5 Sidebar and its footer

- [ ] **Confirm: 238 px by default, headed `catalog` and `local`, with
      `FILES`, `CONNECTIONS`, `PACKAGES` in that order**, each heading folding
      its section. **An empty section shows one muted row**: `no files yet —
      drop one here`, `no connections`, `no packages`. A bare heading is a
      defect.
- [ ] **Confirm: FILES has one row per tab**, with its swatch, and a click
      shows that tab; **CONNECTIONS a row per attached SQLite file or
      MotherDuck database**, with its table count and its tables indented;
      **PACKAGES the recently opened packages**, a click opening one.
- [ ] **Confirm the footer**: `session · 1 window · 0 tabs`, `ai none`,
      `egress 0 B` (green only while nothing has left). One file: **`1 tab`**, singular. File → New Window: **both
      footers read `2 windows`** without a click; close one: **`1 window`**.
      The footer adds no Tab stop.

---

## Section 5 — Keyboard, surface by surface

`docs/a11y.md` §3 lists the headless suites that drive the focus chain and
says what they cannot settle: the harness has no layout and no document
focus, so Tab wrapping, focus pulled back into a dialog and focus handed back
on dismissal are left to `crates/dat0-ui/examples/modal_trap_probe.rs`. This
section is the human half. Where a surface has **no** keyboard path, that is
the finding: file it rather than skip the row.

### 5.1 The modal slot and its trap

One dialog at a time (`Modal`, `crates/dat0-ui/src/state.rs`), painted by
`crates/dat0-ui/src/components/modals.rs`: a scrim, then a dialog
(`data-a11y-id="modal"`) whose header shows a small label and a title.

| Label · title | Open it with | Closed by a click outside or ✕ |
|---|---|---|
| `name` · the prompt's title | Save Query…, Save as Table…, Set Value…, chart Save, AI Save key / Save model, NL→SQL, MotherDuck token | no |
| `export` · `Export` | ⌘E | no |
| `connections` · `Connections` | View → Connections… | no |
| `report` · `Report a Bug` | Help → Report a Bug… (§10.24) | yes |
| `workspace` · `Workspace may be open elsewhere` | §7.2 | no |
| `refresh` · `Refresh will discard edits` | §7.3 | no |
| `saved` · `Saved Queries…`; `history` · `History` | the console's toolbar | yes |
| `recovery` · `Recover unfinished work` | palette → `Review previous sessions`, when something is left to recover | yes |
| `import` · `Import CSV` | a CSV the sniff cannot settle (§10.28) | no |
| `ai` · `AI Providers` | View → AI Providers… | yes |
| `update` · `Checking for updates…` | Help → Check for Updates… | yes |
| `tour` · `Take a tour` | Help → Take a Tour | no |

`Modal::About` exists; nothing opens it.

For **each** dialog above, in each theme:

- [ ] **Confirm: a scrim dims the window. Where focus lands on open**: the
      name prompt's field takes it; in every other dialog it stays on the page
      behind until the first Tab, which enters the dialog (PD-040).
- [ ] Escape straight after opening. **Confirm: it closes, exactly once** —
      not the palette or console behind it as well.
- [ ] **Confirm: Tab cycles only inside and wraps last → first; Shift-Tab
      wraps first → last.**
- [ ] Click behind the scrim. **Confirm: focus stays inside**; the dialog
      closes only where the table says yes, and one that does not shows no ✕.
- [ ] **Confirm: on dismissal, focus returns to where it was.**
- [ ] ⌘K: **nothing opens over the dialog.** With a tab open, a name prompt
      up and text typed, press ⌘E and use File → Export…: **the prompt stays,
      with its text** (step 5.11c: Export used to replace it). ⌘B, and a
      theme toggle from Settings: **the theme changes under the dialog, the
      sidebar does not.**

Dialog by dialog:

- [ ] Name prompt: **field → the confirm button (`Save`, `Set`, `Connect` or
      `NL→SQL`) → `Cancel`**; Enter confirms once; empty shows `Enter a name`
      and stays open; a pasted tab shows `A name cannot contain line breaks or
      tabs`; keys and tokens are typed hidden.
- [ ] Export: **the format group (`CSV`, `JSON`, `Parquet`) and the scope
      group (`Current view`, `Full table`) are one Tab stop each, arrows
      choose**, and Enter on `Export` writes what was chosen.
- [ ] Saved Queries… and History: **arrows move the row, Enter opens it in a
      new query tab**; in Saved Queries, delete removes a row and the list
      stays open. Tour: **`‹ Back`, `Next ›`, `Skip`, `Get started` on panel
      7**; Escape and Skip end it for good.

### 5.2 Command palette

- [ ] ⌘K. **Confirm: typing goes into `Type a command…` and filters; ↑/↓ move
      and stop at the ends, scrolling to keep the ring in view; Enter runs and
      closes; Escape closes; Tab stays inside**; a query matching nothing shows
      `No matching commands`. Close and reopen: **the second opening takes
      typing too** (PD-032).
- [ ] **Confirm: rows with a chord show this platform's** (`Undo ⌘Z`, `Undo
      Ctrl+Z`), **and no two rows read the same**: `Save Query as Table…` and
      `Save View as Table…` (step 5.11e).
- [ ] Run every row at least once across this checklist. **Confirm: each has
      a visible effect.** `Cancel Import` acts only during an import.

### 5.3 Data grid

- [ ] Tab to the grid. **Confirm: it takes focus once; arrows move the active
      cell and the view follows; Shift+arrows extend; ⌘+arrow jumps to the
      edge; ⌘A selects all; Space selects the row, Shift+Space the column;
      Escape collapses the selection.**
- [ ] Enter. **Confirm: the cell editor opens**; Enter commits and moves down,
      Tab right, Shift-Tab left, stopping at the row's ends; Escape cancels; a
      value the column cannot hold shows `Not a valid value for this column`
      and keeps the editor open.
- [ ] PageUp, PageDown, Home, End: **record** what they do (the grid does not
      bind them; the viewport scrolls). **Confirm: no crash, no lost focus.**
- [ ] Open the context menu from the keyboard (Menu key or Shift+F10).
      **Record whether it opens.** Sort and filter a column without the mouse:
      **there is no way** (PD-040) — the sort and funnel marks take no
      focus, and neither the palette nor the context menu sorts or filters.
- [ ] In an open context menu, **arrows skip disabled items, Enter picks,
      Escape closes**. In an open filter popover, **it has the keyboard
      (PD-032) and Tab reaches the operator, value, `Apply`, `Cancel`,
      `Clear`.**

### 5.4 Sidebar tree

- [ ] Tab to the sidebar. **Confirm: the tree takes focus once with a ring;
      ↑/↓ move a cursor row; Enter or Space shows a FILES tab, folds a
      CONNECTIONS database, opens a table, opens a package; ←/→ fold and
      unfold; a folded section's rows are skipped.**
- [ ] Tab again. **Record where focus goes**: the rows and headings are each
      a Tab stop in a webview (PD-040), where the tree should be one.

### 5.5 SQL console

- [ ] ⌘⇧C. **Confirm: the console opens, headed `SQL editor` with `⌘⏎ run`,
      its Run chip `Run ⌘⏎`; on Linux both read `Ctrl+⏎`** (step 5.11e).
- [ ] **Confirm: the query-tab strip is one Tab stop; ←/→ switch tabs;
      Delete closes a tab (never the last); Tab in the editor indents; Escape
      leaves the editor for Run** (Cancel while running), and closes the error
      and AI strips one at a time.

### 5.6 One press, one effect

Most command chords live twice: in the keymap the window's root key handler
reads (`shell.rs`) and as a menu accelerator (`menu.rs`). One press must do
one thing, once, on both platforms.

- [ ] Grid focused, ⌘B once. **Confirm: the sidebar hides and stays hidden**;
      again, it returns. Same for ⌘⇧C and the console.
- [ ] Sort two columns, then ⌘Z once. **Confirm: exactly one step comes off**
      (the pipeline bar shows one); ⌘⇧Z, exactly one back. ⌘E: **one Export
      dialog.** ⌘⏎ in the editor: **History gains one entry, not two.** On
      macOS, WebKit hands the menu a key the page did not mark handled, so a
      chord acting twice here is the defect this row exists for.
- [ ] In the SQL editor, type a word, then ⌘Z; then in a name prompt's field.
      **Confirm: the typing is undone and the grid's view does not change.** On
      Linux, GTK offers a key to the menu's accelerators before the page: if
      Ctrl+Z undoes a grid step instead, file it. (⌘M is macOS only; Ctrl+Q and
      Ctrl+W quit and close on Linux, §4.1.)

### 5.7 Settings window

- [ ] Tab through `Profile`, `Theme`, `Memory Budget`, `MotherDuck`, `AI`,
      `Telemetry`, `Networked Workspaces`, `Updates`, `Advanced`. **Confirm:
      Enter or Space selects one and the body follows, and every control works
      by keyboard alone**: Name and Email; `Theme: …`; the memory field with
      `Applies to new windows. Restart to apply everywhere.`; `Open
      Connections Panel` and `Open AI Panel` (in the workbench window focused
      last); the three toggles; `Learn more about privacy`.
- [ ] Advanced: **confirm `dat0 <version> (<sha>)`; `Log level: …` cycles
      `error` → `warn` → `info,dat0=debug` → `debug`; `Reveal config file`
      opens the config folder; `Reset all settings to defaults` asks `Reset
      settings?` and nothing resets until `Reset`; `Open logs folder` opens the
      `logs` folder, whose `dat0.log` holds this session's lines** (step
      5.11f). Change each setting, quit, relaunch: **every change stuck.**

### 5.8 Panes and panels

- [ ] Inspector: **confirm the mode toggle is reachable and Enter flips it.**
      A saved chart's lineage row **cannot be reached by keyboard** (PD-040);
      a click opens the chart.
- [ ] **Confirm: the chart pane's type and axis buttons, `Save`, `PNG`, `SVG`,
      and every control of the Connections and AI dialogs (`Retry` included),
      are reachable in reading order and work with Enter.** Close each pane:
      **none of its controls stays a Tab stop.**

---

## Section 6 — Docks and panes

### 6.1 Resize, collapse, restore

- [ ] Open the sidebar (⌘B), the right column (Toggle Inspector and Visualize:
      two stacked panes) and the console (⌘⇧C); drag each splitter. **Confirm:
      the content reflows, and no pane can be pushed off screen.**
- [ ] Collapse each pane from its header and by its command. **Confirm: focus
      lands on something live, nothing hidden takes Tab, and with both right
      panes closed the grid takes the whole width.** At the minimum window
      size (720 × 480): **nothing overlaps.**
- [ ] File → New Window. **Confirm: the defaults** (sidebar open, console and
      right column closed). A layout belongs to a window's session and comes
      back with it — a reopened workspace, a recovered window — not with the
      app.

### 6.2 The layout survives a crash

A layout change reaches the session 500 ms after the last one. A scratch
session holding only a layout is swept at the next launch, so give it
something worth keeping.

- [ ] Open iris.csv and sort a column; drag the sidebar to an obviously odd
      width and the console taller. **Wait 2 s**, then `kill -9 <pid>` (not
      SIGTERM). Relaunch → `Review` → `Open` (§7.4). **Confirm: that sidebar
      width, console height and sort come back.** Repeat on a workspace.
- [ ] Negative control: resize, then kill within ~200 ms. **Confirm: the
      previous size comes back** — proof you tested the debounce.

---

## Section 7 — Dialogs and windows that only exist at runtime

### 7.1 Second launch

- [ ] With dat0 running, launch it again from a terminal. **Confirm: the
      second process exits at once, and a new window of the first appears, in
      front and focused.** With a CSV: **it opens in that new window.** With a
      `.dat0`: **a read-only package window opens**, and an empty window
      beside it (PD-039).
- [ ] Close the first window, launch again. **Confirm: a window still
      appears** (PD-027). Quit (⌘Q; Ctrl+Q on Linux), launch: **a fresh
      instance starts.** (Also owed as the P10a UAT's §2.5.)

### 7.2 A workspace already in use

- [ ] With `uat-ws` (§10.10) open, File → Open Workspace… → `uat-ws` from
      another window. **Confirm: the open window comes forward; no second.**
- [ ] Close it. Settings → Networked Workspaces → on. Write
      `uat-ws/.dat0/lock.json` as another machine would:
      `{"pid": 1, "hostname": "uat-other-host", "started_at": "1790000000",
      "dat0_version": "0.1.0", "tombstoned": false}`. Open `uat-ws`.
      **Confirm: `Workspace may be open elsewhere`, naming `uat-other-host`,
      with `Cancel` and `Open anyway`**; the buttons take focus and answer
      Enter; Escape cancels; a click outside does nothing.
- [ ] `Cancel`: **nothing opens.** `Open anyway`: **it opens, and `lock.json`
      names this machine**; close it: **`"tombstoned": true`.** Turn the
      setting off.

### 7.3 Live Refresh

Only the active tab's file is watched (`components/grid/refresh.rs`).

- [ ] Open `live.csv`; change a value in a text editor and save. **Confirm: a
      warning banner `live.csv changed on disk` with `Refresh`, once per burst
      of saves.** With only sorts and filters in the view, `Refresh`: **the new
      value shows, the sort and filter still apply, the banner goes, and no
      dialog asks.**
- [ ] Edit a cell and delete a row in dat0; change the file; `Refresh`.
      **Confirm: `Refresh will discard edits`, reading `Re-importing discards 1
      cell edit(s) and 1 row deletion(s). Filters, sorts, and column changes
      will be kept. Continue?`, with `Cancel` and `Refresh anyway`.** `Cancel`:
      nothing changes. `Refresh anyway`: **the edits go, the sort stays**, and
      focus returns to a live control.
- [ ] Remove from the file the column you sort on; refresh. **Confirm:
      `Refreshed, but some transforms couldn't replay — the file's columns
      changed.`, and the bare table.** Palette → `Refresh from source` on a
      query's tab: **`This tab has no file to read again`.**
- [ ] Change the file of a tab that is not active, then show that tab.
      **Record whether the change is noticed.**

### 7.4 Recovery

- [ ] Open iris.csv, sort it, type `SELECT 42` in the console unrun;
      `kill -9`; relaunch. **Confirm: `1 item(s) to recover from a previous
      session` / `Restore them or discard them.` with `Review`.** `Review`:
      **`Recover unfinished work`, `Orphaned sessions`, one row `iris` with
      `Open` and `Discard` — and not the window you are in** (PD-025).
- [ ] `Open`. **Confirm: a window titled `scratch` with the tab, its sort and
      `SELECT 42`.** Make two more orphans and `Discard` one: **its row goes,
      and so does its folder under `<state root>/scratch/`**; discarding the
      last closes the dialog.
- [ ] Open a file, do nothing else, quit, relaunch. **Confirm: no banner, and
      that window's scratch folder is gone.**
- [ ] With nothing to recover, palette → `Review previous sessions`.
      **Confirm: `Nothing to recover: no earlier session left work behind`,
      and no dialog** (step 5.11c).
- [ ] Two windows: type SQL in the second, close it. In the first, palette →
      `Review previous sessions`. **Confirm: it is listed, and `Open` brings
      its SQL back.**
- [ ] Close `uat-ws`; delete `uat-ws/.dat0/manifest.json`. Open it: **`Incomplete
      workspace` / `A previous Save Workspace was interrupted. The folder may
      be corrupted.`** Relaunch: **the banner counts it; `Review` lists it
      under `Interrupted workspaces` as `<path> (promote didn't finish)`;
      `Resume` opens it and restores `manifest.json`.** With `workspace.duckdb`
      deleted too, `Resume` says **`Could not open workspace`**, that the save
      stopped before moving the database.

---

## Section 8 — Perf HUD

The instrument every performance claim is measured against
(`crates/dat0-ui/src/components/perf_hud.rs`).

- [ ] Palette → `Toggle Performance HUD`. **Confirm: bottom-right over the
      shell, four lines:** `<n> fps`, `p50 <n> / p95 <n> / p99 <n> ms`,
      `rss <size>`, `pages <resident> / <cap>` (`pages —` on the hero).
- [ ] Open the 1 GiB CSV and scroll hard. **Confirm: every number moves** —
      a frame rate, rising percentiles, growing rss, pages nearing the cap. A
      number frozen while scrolling is not wired.
- [ ] Stop. **Confirm: within a second, `— fps`, never `0 fps`**
      (`IDLE_AFTER` is 500 ms); scroll again, and a number returns. On the
      idle hero, **`— fps` holds** (the HUD's own text is not a paint).
- [ ] ⌘↓ in the 1 GiB table. **Confirm: the last row shows** (PD-026).
- [ ] **Confirm: not a Tab stop, legible in all three themes, and gone
      completely when toggled off.**

---

## Section 9 — Update check

No release is published yet, and the embedded key is still the test fixture:
`cargo test -p dat0-core --test update_key_is_production -- --ignored` fails
by design until RL1 step 1 (`docs/release-prerequisites.md`). Until it
passes, walk the prompt and leave the signature box for the run after.

- [ ] Help → Check for Updates…. **Confirm: `Checking for updates…` at once,
      then one of `dat0 is up to date.` (`OK`), `Update failed: <reason>`
      (`OK`), or `Update available: <version>` (`Later`, `Install &
      Restart`)** — no crash, no hang. Network off: **`Update failed: …`
      within 20 s, and the egress figure still moved.**
- [ ] While it still reads `Checking for updates…`, open View → AI
      Providers…. **Confirm: the answer arrives as a banner and the AI panel
      keeps its place.**
- [ ] Launch check on (the default): **a launch is silent unless an update is
      found.** Off: **no request at all** (`egress 0 B`).
- [ ] `Later`: **closes, and does not come back.** `Install & Restart`: where
      dat0's folder is writable it downloads, verifies and restarts on the new
      version; where not, it opens the Releases page. A failure shows `Update
      failed` with the reason.
- [ ] With the production key and a release: a manifest signed with another
      key, and one with a byte changed. **Confirm: both refused.**

---

## Section 10 — The PD-023 walk

One subsection per entry in PD-023's Progress log, in the log's order. Each
restored feature must **do something you can see**. What PD-023 left open is
named where it shows (PD-033 to PD-040); a result worse than its entry says
is a defect.

### 10.1 Event routing (PD-027)

- [ ] Two windows. ⌘B, then a palette row, in the second. **Confirm: both act
      in the second only.** Settings → Theme: **both windows repaint.** Close
      the first: **menus, chords, the palette and a second launch still work.**

### 10.2 SQL console

- [ ] Open iris.csv. In `Query 1`: `SELECT * FROM iris LIMIT 50`, ⌘⏎.
      **Confirm: a grid tab `Query 1` with 50 rows, active.** Change the limit
      and run it again: **the same tab shows the new rows at once**, not `—`
      until a scroll. A second query tab: **a tab `Query 2`.**
- [ ] `CREATE TABLE t AS SELECT 1 AS x`: **an info banner `Statement ran`
      with the statement, and no tab.** `EXPLAIN SELECT 1`: **`Statement ran`
      and `It returned rows, which the grid cannot show yet.`** (PD-033).
      `DESCRIBE iris`, `SUMMARIZE iris`: **rows in the grid.**
- [ ] `SELECT * FROM nope`: **the console's error strip shows DuckDB's
      message**; `Dismiss error` clears it. `SELECT count(*) FROM
      range(20000000000)` then Cancel (⌘.): **`Cancelled`, and the window
      stays responsive.**
- [ ] An empty tab: **`There is no statement to run`**, and run from the
      palette with the console shut, **the console opens to say it.** Two
      statements, caret in the second: **only the second runs.**
- [ ] `History`: **every run, `ok` or `err`; one reopens in a new query
      tab.** `Save Query…` as `uat q`: **`Query saved`**; `Saved Queries…`
      lists it, opens it, deletes it. `Save as Table…` on the `LIMIT 50` query
      as `first50`: **a tab `first50`, and `Table saved`.**
- [ ] Type `SELECT * FROM ` and then `first50.`. **Confirm: completion offers
      the window's tables, then that table's columns.** SQL in two query tabs,
      `kill -9`, recover (§7.4): **both tabs and their SQL return.**

### 10.3 The grid's view

- [ ] Hover a header: **`↕` (sort) and `▾` (funnel) show at its right.** Click
      the sort mark: **▲, ▼, off.** Shift-click another's: **both sorted, `▲1`
      and `▲2`.**
- [ ] A funnel: **the popover opens at the column, with its most common
      values** (`Showing n of m distinct values; type to add more` when cut
      short). Apply: **rows filter, the funnel stays lit.** Filter a second
      column, then `Clear` the first: **only the first filter goes.**
- [ ] **Confirm the pipeline bar**: `pipeline`, the file's name, then a chip
      per step joined by `then`; a chip jumps the view there; its ✕ (`Remove
      step`) removes it. ⌘Z and ⌘⇧Z: **one step back, one forward.**
- [ ] Drag a header by its grip (left edge). **Confirm: the column moves, its
      width with it.** Its right edge resizes it. Sort tab A, visit B, return:
      **A kept its view.** `SELECT DATE '2026-01-01' AS d, true AS b`: **the
      cells read `2026-01-01` and `true`**, not `(Date32)` or `(Boolean)`.

### 10.4 The grid's edits

- [ ] Enter, type, Enter. **Confirm: the value shows and a step joins the
      pipeline**; Undo takes it back; the file on disk never changes.
- [ ] Copy (⌘C) a 3 × 2 range into a spreadsheet: **3 rows, 2 columns.** Into
      a text editor: **tab-separated, a cell holding a tab, newline or quote
      quoted.** A scattered selection: **its bounding box, empty where
      unselected.** 50,000 rows of one column of the 1 GiB table: **no line is
      `—`.**
- [ ] Paste (⌘V) a 2 × 2 block from a spreadsheet: **the values land**; one
      the column cannot hold is skipped with `Some cells were not pasted`.
- [ ] Cut (⌘X): **copied, then `NULL`.** Fill Down (⌘D): **the top value
      fills down, as typed** (`5.1` stays `5.1`). Delete: **`NULL`.** Palette →
      `Set Value…` (`Set value`, `Set`): **the value lands**, or `Some cells
      were not set`.
- [ ] Right-click: **`Copy`, `Cut`, `Paste` | `Fill Down`, `Set NULL` |
      `Delete Row(s)` | `Delete Column`**, the selection-bound ones disabled
      with no selection; a right-click inside a selection keeps it (on macOS,
      Ctrl-click too). `Delete Row(s)`: **the rows go; Undo returns them.**
      `Delete Column` on the last column: `A view keeps at least one column`.
- [ ] Over 10,000 cells in one edit, or 100,000 in one copy: **`Too many cells
      for one step`** (PD-034). An edit on a query's tab:
      **`These rows cannot be edited`.** In a package window: **`This workspace
      is read-only`**, and the context menu enables only Copy.

### 10.5 Save View as Table

- [ ] On iris: sort, filter, hide a column; pipeline bar → `Save View as
      Table…` as `view_saved`. **Confirm: a tab `view_saved` with the view's rows, in
      order, without the hidden column or any `__dat0_rowid__src`; `Table
      saved`.** Its inspector lineage: **`iris` with `transform (n ops)`.**

### 10.6 Export…

- [ ] ⌘E on iris.csv. **Confirm: `Export`, pointed at iris.csv's folder.**
      `Current view` as CSV: **the view's rows, order and visible columns under
      their names, no row-id column; `Export complete` with the path.** `Full
      table` as Parquet, and JSON: **both readable in dat0.**
- [ ] `Choose folder…`: **the picker, and the dialog keeps the typed name.** A
      name with `/`: `A file name cannot contain a path separator`; empty:
      `Enter a file name`; an unwritable folder: `Export failed`. Over an
      existing file: **record** that it is overwritten without asking.
- [ ] ⌘E with no tab open. **Confirm: `Nothing to export`, and no dialog**
      (step 5.11c: the dialog opened and wrote nothing).

### 10.7 Live Refresh

- [ ] §7.3 walked on this platform.

### 10.8 Recovery

- [ ] §7.4 walked on this platform.

### 10.9 Open Workspace, and the demo

- [ ] File → Open Workspace… → `uat-ws` (§10.10). **Confirm: a window titled
      `uat-ws` with its tabs, views and SQL.** A folder without `.dat0/`:
      **`Not a dat0 workspace — no .dat0/ directory found.`** Relaunch:
      **`uat-ws` is under the hero's `Recent` and File → Open Recent, and both
      open it.**
- [ ] Drop the `uat-ws` folder on a window, then run `dat0 ~/dat0-uat/uat-ws`.
      **Confirm: each opens the workspace in a window of its own** (step
      5.11b; the command line also leaves an empty window, PD-039). A plain
      folder: **`Not a dat0 workspace — no .dat0/ directory found.`**
- [ ] First-run hero → `▶ Open demo.dat0` (to see the band again: Settings →
      Advanced → Reset, then File → New Window). **Confirm: a window `demo`
      with tabs `genre` (its `name` column headed `Genre`) and
      `revenue_by_genre`, and `Top customers` in Saved Queries, which runs.**
      Each click unpacks a fresh copy under `<state root>/demo/`.
- [ ] Inspector on `revenue_by_genre`. **Confirm: `Used by` lists `Revenue by
      Genre`, and a click draws the bar chart** (step 5.11b: its stored
      source is schema-qualified). **Record** the lineage `Sources`: none are
      expected (PD-031).

### 10.10 Save Workspace

- [ ] A scratch window with iris.csv (sorted), SQL in a query tab and a
      query's result tab: File → Save Workspace…. **Confirm: `Choose a folder
      for the workspace`, able to make a folder.** Make `~/dat0-uat/uat-ws`.
      **Confirm: `Workspace saved` with the path; the title bar reads
      `uat-ws`; iris keeps its sort; the result tab closes and its SQL
      stays**; `.dat0/` holds `workspace.duckdb`, `session.json`,
      `manifest.json`.
- [ ] Save again there: **`Already saved — this session is already a
      workspace.`** From another window into `uat-ws`: **`That folder is
      already a workspace. Open it with File > Open Workspace.`** From a package
      window: **`This workspace is read-only`.** During the long query of
      §10.2: **after about 5 s, `Save Workspace waits for the query that is
      running. Try again when it finishes.`, and the window carries on.**
- [ ] Quit; reopen `uat-ws`. **Confirm: tabs, views and SQL are back, and a
      change to iris.csv still offers `Refresh`.**

### 10.11 The Save Workspace nudge

- [ ] New scratch window: three view steps (or a saved query or chart).
      **Confirm: once, `Save this as a workspace to keep it?` with `Save
      Workspace`, which opens the folder picker** — and never in a workspace or
      package window.

### 10.12 Open Package

- [ ] File → Open .dat0 Package… → demo.dat0. **Confirm: a window `demo`,
      pill `read-only`, its tabs with their views, `Top customers` saved.**
      Dropping it or passing it on the command line **does the same**; it is
      listed under PACKAGES and the hero's `Recent`, and both reopen it.
- [ ] Delete a listed package's file; click its row. **Confirm: `Could not
      open package` with the path.** A truncated copy: **record what shows**
      (expected: `Could not open this window's data engine`, the reason, `Try
      again`). Close the package window; relaunch: **its folder under
      `<state root>/inspect/` is gone.**

### 10.13 Unpack Package

- [ ] Put your own `~/dat0-uat/unpacked/data/customer.parquet`. File → Unpack
      .dat0 Package… → demo.dat0 → `Choose a folder to unpack the package into`
      → `~/dat0-uat/unpacked`. **Confirm: `Package unpacked`, an editable
      window `unpacked`, and your file untouched.**
- [ ] Into `uat-ws`: **`That folder is already a workspace. Choose another
      folder to unpack into.`, nothing written.** A truncated package: **`Unpack
      package failed`, no `.dat0/` left behind.**

### 10.14 Export Package

- [ ] In `uat-ws`: File → Export as .dat0 Package…. **Confirm: `uat-ws.dat0`
      suggested; `Package exported`.** Open it: **its tables, views, saved
      queries and charts.** **Record** whether a table saved from a view keeps
      its source; after the reopen in §10.10 it is expected not to (PD-031).

### 10.15 Replay Package

Replay rebuilds a package's tables on new data, and refuses a package whose
sources do not match the files given.

- [ ] Scratch window: open `sales.csv`; in the console, `SELECT * FROM sales
      WHERE amount > 10` → `Save as Table…` as `big` (a table made this way
      keeps its SQL); export the package as `sales.dat0`.
- [ ] File → Replay .dat0 Package… → `sales.dat0`. **Confirm: a picker titled
      `Choose the file to read in place of sales.csv`** (one per source); pick
      `sales-v2.csv`; `sales-replayed.dat0` is suggested; **`Package
      replayed`**, and the new package shows v2's rows in `sales` and `big`.
- [ ] With `other.csv`: **`Replay package failed` with the reason, no new
      package.** demo.dat0: **`This package has no source files to replay
      against.`**

### 10.16 Perf HUD

- [ ] §8 walked on this platform.

### 10.17 SQLite

- [ ] Drop chinook.sqlite. **Confirm: a tab `chinook_Album`; CONNECTIONS lists
      `chinook`, `11`, its tables indented.** Click `Artist`: **`chinook_Artist`.**
      Click `chinook`: **it folds.** The same from the hero's Chinook card,
      File → Open File… (`.sqlite`, `.sqlite3`, `.db`) and the command line.
- [ ] A copy named `chinook.bin`: **attached too** (the header decides), under
      a new alias, `chinook_2`. The same file again: **no second entry.** Edit
      a cell: **refused (`These rows cannot be edited`)**; record whether its
      reason, which speaks of a query's results, fits.
- [ ] `sqlite3 ~/dat0-uat/empty.sqlite "CREATE TABLE t(x); DROP TABLE t;"`,
      dropped: **`That SQLite file has no tables`.** A truncated copy: **`Could
      not open the SQLite file`, and nothing left under CONNECTIONS.**
- [ ] With chinook attached, `kill -9` and recover. **Confirm: attached again
      before its tabs return, with rows.** With the file moved away: **`A SQLite
      file could not be attached again` with the path; the other tabs return.**
      Save Workspace with it attached: **its tabs still show rows.**

### 10.18 Updates

- [ ] §9 walked on this platform.

### 10.19 Charts

- [ ] No tab, View → Visualize: **`Open a table to chart it`.** On sales.csv:
      **titled `sales.csv`, a type chosen from the columns, `X: —`, `Y: —`,
      `Select columns to render a chart`**; the axis buttons cycle its columns
      and never offer `__dat0_rowid`.
- [ ] X `region`, Y `amount`, bar: **it draws.** Cycle the type: **one missing
      an axis says so in the chart's place.** Filter `region` to one value:
      **it redraws from the filtered rows**; Undo restores it; a cell edit
      shows. Switch tabs and back: **the sales chart is kept.**
- [ ] `PNG`, `SVG` (enabled once drawn), palette → `Export Chart as PNG`:
      **`Chart exported` with the path, in the colours on screen** (try dark).
      Nothing drawn: **`There is no chart to export yet`.**

### 10.20 Saved charts and the inspector

- [ ] Chart `Save` (enabled once an axis is picked): **`Save chart as…`
      pre-filled from the type and axes; `Chart saved`.** On a `Query 1` tab:
      **`Save the query's rows as a table to keep a chart of them`, no prompt.**
- [ ] Toggle Inspector on iris: **`iris — Profiling…`, then `iris — 150 rows ·
      5 cols`** (the row id not counted), one card per column with stats,
      distinct, null lines and a small chart. With a filter, flip the toggle:
      **`Current view` counts filtered rows, `Whole table` all; flipping back
      shows Whole table's own numbers.**
- [ ] **Confirm the lineage** on `sales`: `Sources` sales.csv (`file
      import`), then `sales`, then `Used by` your saved chart (`chart`); a click
      shows the chart over the sales tab. `first50` (§10.2) shows `iris` as
      `SQL reference`. No tab: **`No table selected`.**

### 10.21 AI key entry

- [ ] View → AI Providers…. **Confirm: `AI Providers` with the enable toggle,
      the provider cycle (`None`, `Anthropic`, `OpenAI`, `OpenRouter`,
      `Custom`), `No key`, `Save key`, the model and `Save model`, the
      endpoint and sample-row toggles, `Test connection` and an egress line.**
- [ ] `Save key`: **`Paste your API key`, hidden**; save: **back in the panel,
      `Key set`, the key nowhere in clear, the log included.** Cancelled: **back,
      `No key`.** `Save model`: `Model name (e.g. gpt-4o)`. `Test connection`:
      **a result, and egress moves.**
- [ ] Keyring locked or missing: **`The keychain could not be opened, so this
      key lasts only until dat0 quits`**, and a relaunch loses it; with the
      keyring it stays. **The footer reads `ai <provider>`** once AI is on,
      with a key and a model.

### 10.22 NL→SQL and Explain

- [ ] AI not ready: **`NL→SQL` and `Explain` shown disabled.** Ready:
      `NL→SQL` → `Describe the query` → "total amount per region in sales".
      **Confirm: the answer streams in a strip; `Stop` keeps what came;
      `Insert` puts the statement, without its code fence, in a new query tab,
      and it runs; `Discard` drops it**; both buttons are disabled while an
      answer arrives.
- [ ] `Explain` on SQL: **an explanation streams; `Close` closes it**; on an
      empty tab Explain is disabled. Network off: **the strip shows a failure,
      not a hang.** After `Forget key`: **the buttons disable, or `AI needs a
      provider, a key and a model: set them in the AI panel`.**

### 10.23 MotherDuck, and detaching a SQLite file

- [ ] View → Connections… (or Settings → MotherDuck → `Open Connections
      Panel`). **Confirm: `Connections`, MotherDuck `Disconnected` with
      `Connect`, `Attached files` with chinook and `Detach`, and `Attach
      SQLite…`.**
- [ ] `Connect`: **`MotherDuck token`, hidden; `Connecting…`, `Connected as
      md`, the account's databases in the dialog and under CONNECTIONS**, a
      table opening as a tab, egress gaining its `+`. `Test connection`:
      **`Connection OK`.** A wrong token: **`MotherDuck token was rejected.
      Check the token and try again.` with `Retry`.** Offline: **`Could not
      reach MotherDuck.`**
- [ ] `Disconnect`: **its databases leave CONNECTIONS.** `Forget token`: **the
      next Connect asks again.** Reopen a session recorded as connected, token
      kept: **it connects by itself.**
- [ ] `Detach` chinook: **its tabs close, it leaves CONNECTIONS and the dialog,
      and a recovered session does not attach it.** `Attach SQLite…`: **a
      picker, and the file attaches** as in §10.17.

### 10.24 The crash report, and Report a Bug

A release build has no crash trigger, so stage what the panic hook writes.
With dat0 not running:

```bash
printf '%s' '{"message":"uat staged crash","backtrace":"uat","version":"0.1.0"}' \
  > "<state root>/last-crash.json"
```

- [ ] Settings → Telemetry → `Enable crash report submission` on; stage;
      launch. **Confirm: `dat0 quit unexpectedly` — `dat0 closed unexpectedly
      last time. Send an anonymous crash report to help fix it? You can add a
      note below.` — a note field, `Cancel`, `Send Report`, in the first window
      only.**
- [ ] `Cancel` (or Escape, or a click outside): **`last-crash.json` is gone,
      and the next launch asks nothing.** Stage again, `Send Report`: **it
      closes (up to 5 s), the file is gone, and a banner reads `Report sent.
      Thank you.`**; a CI build's report reaches GlitchTip with the note
      (record which build you ran). **Confirm: the egress figure moved, and
      is no longer green.**
- [ ] Crash reports off (the default); stage; launch: **no dialog, and the
      file deleted unsent.** Crash reports on, `first_run_done = false` in
      `settings.toml` (so the tour opens first), stage, launch: **the offer
      waits, and the file stays for a later launch.**
- [ ] Crash reports on: Help → Report a Bug…. **Confirm: `Report a Bug` —
      `Describe what happened. A diagnostic report (no file contents or query
      text) will be attached.` — a note field, `Cancel`, `Send Report`.**
      Crash reports off: **`Crash reports are off, so nothing can be sent from
      here…`, naming Settings → Telemetry, with `Close` and no `Send Report`**
      (step 5.11a). Turn them on in Settings without relaunching; Help →
      Report a Bug… again: **it can be sent now.**
- [ ] On a CI build, launch with crash reports on and open Help → Report a
      Bug…; turn them off in Settings → Telemetry with the report still up;
      `Send Report`. **Confirm: `Report not sent`, and no event reaches
      GlitchTip.** Turning the opt-in off stops sending at once (step 5.11a).

### 10.25 Honest chrome

- [ ] §4.3 (⌘K and its hints), §4.4 (the measured egress figure) and §4.5 (the
      window count, `1 tab`) walked on this platform.

### 10.26 A theme kept across launches (steps 5.9, 5.11e)

- [ ] §3.4 walked on this platform.

### 10.27 The status bar and the title bar (step 5.8c)

- [ ] §4.2's pill and §4.4's segments walked on this platform.

### 10.28 The import wizard (step 5.10)

- [ ] Drop `latin1.csv`. **Confirm: `Import CSV` opens, saying `This file is
      not UTF-8; dat0 cannot read it`, and Import cannot go on; `Cancel`
      opens nothing.**
- [ ] Drop `mixed.csv`. **Confirm: `Import CSV` opens on the `Dialect` step,
      its columns what DuckDB reads under the delimiter it chose.** (If it
      opens without the wizard, record that and use any file that brings it
      up.) Change `Delimiter`: **the columns follow**; clear it: `Choose a
      delimiter`, and `Next` waits.
- [ ] `Columns`: untick one (`Import`) and rename another (`Import as`); two
      of the same name show `Two columns share this name`. `Confirm` lists
      `Columns to import:`; `Import`: **a tab with exactly those columns,
      renamed, and the file's rows.** With `First row is a header` off: **the
      header line is a row.**
- [ ] With a dialog up, drop `mixed.csv`: **the dialog keeps its place, and a
      banner says to close it and open the file again.**
- [ ] Change the file and `Refresh` its tab: **record what it reads**; it
      reads with automatic detection, not the dialect chosen (PD-037).

### 10.29 Window chrome and what the chrome says (step 5.11)

- [ ] §4.1's Linux window items, §4.2's title bar, §4.3's narrow window,
      §4.4's measured privacy line, §5.1's dialog guard, §5.5's run hint and
      §5.7's log walked on this platform.

### 10.30 What PD-023 left open — confirm and file

- [ ] Each known gap, where it shows, matches its entry: PRAGMA and EXPLAIN
      rows (PD-033, §10.2), the edit caps (PD-034, §10.4), closing a scratch
      window asks nothing (PD-035: close one holding work and relaunch), Open
      Recent until relaunch (PD-036, §4.1), the wizard's dialect on refresh
      (PD-037, §10.28), no tab close (PD-038, §4.3), the empty window at
      launch (PD-039, §7.1), keyboard reach (PD-040, §5), lineage after a
      reopen (PD-031, §10.9).

---

## Section 11 — Closing: a clean machine, and filing what you found

### 11.1 Run it on a clean machine

Everything above must be walked where dat0 has **never** run. For macOS,
follow `docs/plans/2026-06-17-dat0-p10a-uat.md` §1:

```bash
# Clone the gold image to a throwaway VM (see docs/ci-mac-vm-runner.md)
tart clone dat0-runner-base dat0-uat-v1
tart run dat0-uat-v1
```

A real machine where dat0 was never installed is equally valid. One with a
`settings.toml`, `recents.json`, `scratch/` folder or keychain entry from an
earlier run is **not**: §1, §3.4, §7.4 and §10.24 depend on empty state. On
Linux, use a clean Ubuntu 24.04 VM with a desktop session, a portal and a
keyring; a bare container cannot show the pickers or keep a key.

- [ ] The macOS pass ran on a clean VM or a never-installed machine.
- [ ] The Linux pass ran on a clean Ubuntu 24.04 VM with a desktop session.
- [ ] `docs/plans/2026-06-17-dat0-p10a-uat.md` (signing, notarization,
      install, double-click, artifact verification) was walked in the same
      session — it is a prerequisite, not an alternative.

### 11.2 File every defect before declaring done

**This plan is not complete until every defect found here is a written entry
in `docs/deferrals.md`.** A defect that lives only in a chat message is a
defect that ships.

Numbering, as of 2026-09-26:

- **`D-` (deferrals):** D-038 is the highest in use (D-016, D-017 and D-035
  were never assigned). **New deferrals start at D-039.**
- **`PD-` (plan defects):** PD-040 is the highest in use. **New plan defects
  start at PD-041.**

Check the file before you write: either counter may have moved. Follow its "How to add an entry": the next id, a row in the
at-a-glance table, a full entry.

For each defect record the section number here, what you did, what you saw,
what you expected, the platform and OS version, the theme, the build SHA and
a severity. `high` is reserved for the failure this document exists to
catch: **logic green, screen dead.** A defect in a feature §10 walks names
the PD-023 step that restored it; if `README.md`'s Status table lists that
feature under "Works in the current build", add the new number to that row's
"Known gaps".

### 11.3 Fold the results back into `docs/a11y.md`

Its sections as they stand (rewritten for the Dioxus build, 2026-08-13):

- [ ] **§1, the §21.2 checklist.** A1 reads "PASS, automated". While PD-040 is
      open (the grid's sort and funnel, the data tabs, the lineage rows, the
      sidebar's Tab stops), A1 is not a pass: change its status and evidence
      and link PD-040 and any number this run adds. Add this run's §2 result, dated, as
      the by-eye evidence beside A2's measured one.
- [ ] **§2, WCAG contrast.** Re-run `cargo test -p dat0-core --test
      theme_tokens_contrast`. If §3 found text lost in its ground while the gate
      passed, the gate missed a pair: file it; never relax a threshold.
- [ ] **§3, keyboard reachability.** "What the harness can and cannot assert"
      leaves three trap guarantees to `modal_trap_probe` (Tab wraps, focus is
      pulled back, focus is handed back on dismissal). Record §5.1's result for
      each dialog, per platform, dated, as the by-hand evidence.
- [ ] **§4, the screen-reader stance.** It says no screen-reader UAT has been
      performed. Leave that sentence unless a VoiceOver or Orca pass was run;
      if one was, record what was heard.
- [ ] **§5, the tightest-pair watch.** Refresh it only if a token moved.
- [ ] Link the first-run hero capture (§1.1) from `README.md` and remove its
      "Screenshot pending" note.

### 11.4 Sign-off

- [ ] Every box above is `[x]` or carries a `PD-` number.
- [ ] Every `PD-` number written here exists in `docs/deferrals.md`.
- [ ] Date, machine, OS version and build SHA recorded below.

```
Run by:
Date:
macOS machine/VM:          OS version:          build SHA:
Linux machine/VM:          OS version:          build SHA:
Defects filed:
```
