# Sketch 009 — Redesign Landing v4 ("readability pass")

**Source:** `redesign-landing-v4.dc.html` (this directory) — imported 2026-08-04 from
Claude Design project `Dat0 marketing redesign concepts`
(`58e43481-56d0-480e-b93b-e153c8c71881`, file `Redesign Landing v4.dc.html`).
The sibling `support.js` in that project is the `dc-runtime` harness (a generic
`<x-dc>` template + `DCLogic` React shim). It carries **no design intent** and is
not reproduced here — only the `<x-dc>` template and the `Component extends DCLogic`
script matter.

**Decision (user, 2026-08-04): build the LIGHT rendering.** The design is *authored*
in dark hexes, but its `data-props` declares `theme` default `"light"`, and it ships a
37-pair dark→light token table whose sole purpose is that mapping. This also matches
the standing CLAUDE.md rule (light-only, no `.dark` block). Section 3 below resolves
every value so the mapping is not re-derived downstream.

Other `data-props` defaults: `accent: "blue"`, `density: "comfortable"`, `sidebar: true`.

---

## 1. What this replaces

v4 is **not** a re-skin of the current landing. It is a different page model.

| | Current (`v1.0` Artifact arc) | v4 |
|---|---|---|
| Page model | Document — scrolls, `Container` max-width, 7fr/5fr grid | **Application shell** — `100vh`, `overflow:hidden`, no page scroll |
| Chrome | `SiteHeader` + `SiteFooter` from `app/layout.tsx` | Own fixed titlebar + tab strip + status bar |
| Content | Hero → 5 `ChunkSection`s → `Finale`, sticky compiling worksheet | 8 **accordion panes**, one open at a time |
| Navigation | Native scroll | Wheel hijack, keyboard, tabs, sidebar, ⌘K palette |

The incompatibility is structural: `app/layout.tsx` wraps *every* route in
`SiteHeader`/`SiteFooter`, and v4 needs the full viewport with its own chrome.
`/docs` must keep the existing frame. This is a layout restructure, not a page swap.

---

## 2. Shell anatomy

All dimensions are literal from the source.

```
┌────────────────────────────────────────────────────────────┐
│ titlebar                                            h 44px │  fixed, z 60
│  ●●● (3 dots)  dat0▮  [● in development]      [Watch build]│
├────────────────────────────────────────────────────────────┤
│ tab strip                                           h 38px │  fixed, z 60
│  [🔍 search        ⌘K] │ overview │ compare │ … │ waitlist │
├──────────────┬─────────────────────────────────────────────┤
│ sidebar      │ pane stack                    (data-stack)  │
│ w 238px      │  ┌───────────────────────────────────────┐  │
│              │  │ ▼ OVERVIEW  Your data outgrew…  h 32px│  │  open pane:
│ CATALOG local│  │ ┌───────────────────────────────────┐ │  │  flex 1 1 auto
│  files       │  │ │ body — 2-col split                │ │  │  min-height 360px
│   sales.csv  │  │ └───────────────────────────────────┘ │  │
│   tradeoffs  │  └───────────────────────────────────────┘  │
│   events.pq  │  ┌───────────────────────────────────────┐  │  collapsed pane:
│  connections │  │ ▶ COMPARE   Every other tool…   h 32px│  │  flex 0 0 32px
│   pg · crm   │  └───────────────────────────────────────┘  │  body opacity 0
│   ai · byo   │  … 6 more collapsed headers                 │
│  packages    │                                             │
│   q2.dat0    │                                             │
│   q2-july    │                                             │
│   waitlist   │                                             │
│ ──────────── │                                             │
│ session · …  │                                             │
└──────────────┴─────────────────────────────────────────────┘
│ status bar                                          h 30px │  fixed, z 60
│ ● engine duckdb · mem 412 MB · rows N/1.2B · 60fps  … ⌘K   │
└────────────────────────────────────────────────────────────┘
```

- Shell grid: `grid-template-columns: 238px minmax(0,1fr)`, `padding-top: 80px`
  (44 + 38 − 2 borders), `height: 100vh`.
- Sidebar and pane stack are both `height: calc(100vh - 113px)` (80 top + 30 status + 3).
- Pane stack: `display:flex; flex-direction:column; gap:8px; padding:12px 20px; overflow-y:auto`.
- Pane transition: `flex-grow .45s, flex-basis .45s` on `cubic-bezier(0.2,0,0,1)`.
  Pane body cross-fades `opacity .3s`.

### Responsive (authored breakpoints)

| Max-width | Effect |
|---|---|
| 1180px | `[data-split]` → 1 column, gap 24px, drop right borders; `[data-metrics]` → 1 col; compare table becomes an `overflow-x:auto` region with `min-width:880px` rows |
| 1080px | Sidebar `display:none`; shell → 1 column; search gutter `flex:0 0 168px` |
| 900px | `[data-hide-narrow]` (the "Watch the build" button) hidden; main padding → 12px |

**Gap:** there is no phone-tier design. A `100vh`/`overflow:hidden` accordion with
wheel hijack is hostile below ~700px. Needs a decision — see §7.

---

## 3. Light palette (resolved)

The runtime rewrites inline styles *declaration by declaration*, so surface roles and
text roles map differently. Two rules matter:

1. **`#101318` stays `#101318` when used as a `color`** (`TEXT_SKIP`) — it is the ink
   on the amber CTA and must stay dark in both themes. As a *background* it maps to `#fcfcfb`.
2. **`#f5a623` as a `color` maps to `#8a3f07`**; as a fill/background/border it stays
   `#f5a623`. This is the design's own restatement of the existing amber hard rule.

`color`, `caret-color`, `-webkit-text-fill-color` are the text-role properties.

### Surfaces & structure

| Role | Dark (authored) | **Light (build this)** |
|---|---|---|
| page canvas | `#101318` | `#fcfcfb` |
| pane surface | `#141a21` | `#ffffff` |
| pane header | `#1a2028` | `#f7f8fa` |
| panel | `#171c23` | `#f1f3f5` |
| — | `#12161c` | `#fafbfc` |
| — | `#1a2029` | `#f3f4f6` |
| accent column / palette row active | `#1e2836` | `#e8f1fb` |
| rule, dim | `#2f3641` | `#dde2e8` |
| rule | `#343b45` | `#d0d7de` |
| input border | `#3d444d` | `#afb8c1` |
| chrome deep (titlebar, sidebar, status) | `#080b0e` | `#e8eaed` |
| chrome raised (buttons, pills) | `#0f1217` | `#eff1f4` |
| tab active | `#12171d` | `#e4e7ea` |
| chrome border | `#1b2027` | `#dfe3e8` |
| chrome border 2 | `#252b33` | `#ccd2d9` |
| tab-strip ground | `#05070a` | `#dcdfe4` |
| tab hover | `#0d1116` | `#e0e3e7` |
| tab divider | `#1f242c` | `#c8ced7` |
| search-field ground | `#03050a` | `#fdfdfd` |
| catalog row active / palette surface | `#20262f` | `#eef1f4` |

### Text

| Role | Dark | **Light** |
|---|---|---|
| bright (headings, values) | `#f0f3f6` | `#0a0b0d` |
| body | `#c9d1d9` | `#1f2328` |
| muted | `#9aa5b1` | `#4f5760` |
| chrome muted | `#78818c` | `#5a636e` |
| ink-on-amber | `#101318` | `#101318` *(unchanged — TEXT_SKIP)* |

### Semantic / syntax

| Role | Dark | **Light** |
|---|---|---|
| accent blue (tab underline, caret, ring, dat0 column) | `#58a6ff` | `#03459b` |
| green (ok, "never", local) | `#3fb950` | `#136229` |
| green 2 (SQL string) | `#56d364` | `#116329` |
| red (the wall, failure cells) | `#f85149` | `#a01420` |
| amber text (pill, sealed stamp, lineage `;;`) | `#e3a72f` | `#7a5200` |
| purple (SQL keyword) | `#bc8cff` | `#5a32a3` |
| purple 2 (SQL number) | `#d2a8ff` | `#6f42c1` |
| cyan (sqlite chip) | `#39c5cf` | `#0e565c` |
| **amber fill** (logomark, CTA, LINE chunk, 1.2B, scan sweep) | `#f5a623` | `#f5a623` *(unchanged)* |
| amber-as-text | `#f5a623` | `#8a3f07` |
| CTA hover fill | `#ffb733` | `#ffb733` *(not in token table — unchanged)* |

### Alpha values

| Role | Dark | **Light** |
|---|---|---|
| inset shadow | `rgba(0,0,0,0.6)` | `rgba(31,35,40,0.16)` |
| SEALED stamp ground | `rgba(16,19,24,0.9)` | `rgba(252,252,251,0.92)` |
| the-wall band ground | `rgba(248,81,73,0.09)` | `rgba(207,34,46,0.06)` |
| palette shadow | `rgba(0,0,0,0.55)` | `rgba(31,35,40,0.18)` |
| palette scrim | `#00000080` | `#1f232840` |

### Not token-mapped (carry through as authored)

`rgba(245,166,35,0)` / `rgba(245,166,35,0.09)` / `rgba(245,166,35,0.5)` (scan sweep
gradient), `rgba(245,166,35,0.18)` (LINE chunk fill), `#17181d` (logomark glyph
knockout), `#58a6ff4d` (`::selection` — light rule already overrides to `#03459b33`).

### Derived light-theme palette object (from `PAL.light`)

`bg #fcfcfb · panel #f1f3f5 · activeBg #e8f1fb · fg #1f2328 · bright #0a0b0d ·
muted #4f5760 · chromeRaised #eff1f4 · chromeMuted #5a636e · tabActive #e4e7ea ·
paneHead #f7f8fa · catActive #eef1f4 · catRule #5a636e · paneEdge #d0d7de ·
paneEdgeDim #dde2e8 · paneShadow 0 8px 24px rgba(31,35,40,0.10)`

Accent (light): `blue #03459b`, `amber #8a3f07`. Link `#03459b`, hover `#02306e`.

---

## 4. Type

- Sans: **Geist** (`400 500 600 700`). Mono: **Geist Mono** (same weights).
  Repo already self-hosts both via `next/font` — the design's Google Fonts `<link>`
  is a canvas artifact, **do not port it**.
- `body { text-wrap: pretty; -webkit-font-smoothing: antialiased }`.

| Use | Spec |
|---|---|
| h1 (overview) | `clamp(34px,3.4vw,48px)` / 600 / lh 1.03 / ls −0.035em |
| h2 (waitlist) | `clamp(28px,3vw,40px)` / 600 / lh 1.04 / ls −0.035em |
| h2 (compare) | `clamp(26px,2.6vw,34px)` / 600 / lh 1.06 / ls −0.03em |
| h2 (panes 3–7) | `28px` / 600 / lh 1.1 / ls −0.03em |
| lead (overview) | `15.5px` / lh 1.6 |
| body | `15px` / lh 1.58–1.6, `max-width: 62ch` |
| small body | `13px`–`14.5px` |
| mono data | `12.5px` |
| mono label | `11px` / ls 0.10–0.12em / uppercase |
| wordmark | Geist Mono 700 / ls −0.05em / 26px inline, 17px in titlebar |

`[data-row] > span` and `[data-gridhead] > span` are `nowrap; overflow:hidden; text-overflow:ellipsis`.

---

## 5. Pane inventory

Order is load-bearing (it is also the ⌘K palette order and the wheel/arrow order).
Each header is `<button data-pane-head>` — chevron, mono uppercase id, title, right-side meta.

| # | id | cat | Header title | Header meta | Body |
|---|---|---|---|---|---|
| 01 | `overview` | files | Your data outgrew the spreadsheet | `1.2 B rows · one laptop` | Hero copy + waitlist form + wordmark + meta line ‖ Two-tab grid with **the wall** at row 1,048,576 |
| 02 | `compare` | files | Every other tool asks for a trade | `scale · custody · proof` | Heading + lead over a 5-column comparison table, dat0 column ringed in accent blue, final row `→ trades away` / `nothing` |
| 03 | `grid` | files | Open a 12 GB file like a 5 MB one | `csv · parquet · sqlite` | Heading with **rolling format word** + lead + 3 metric tiles ‖ format chips + 5-row events grid + `grid ready · 0.4 s` |
| 04 | `query` | db | Join files with live databases | `one SELECT · no ETL` | 6-line SQL editor with line numbers + 3-row result set ‖ heading with **two rolling words** + lead + BYO-key AI panel |
| 05 | `replay` | pkg | Re-run last month on new data | `14 transforms · diffed` | Heading + lead + 2 metric tiles ‖ terminal, 8 lines, two of them **typed out** |
| 06 | `seal` | pkg | The whole workflow in one file | `.dat0 · sha-256` | `workflow.dat0` card — 5 chunk bars + 5 chunk rows + **SEALED stamp** (reveals on open) ‖ heading + lead |
| 07 | `privacy` | db | Guarantees you can read as code | `0 B egress` (green) | Heading + lead + receipts file list ‖ 5-row guarantee table; full-width **"deliberately not"** strip below |
| 08 | `waitlist` | pkg | Get early access | `not shipped yet` | Heading + file line + waitlist form + wordmark + © + links ‖ `lineage.json` comment block |

Sidebar catalog rows map to panes via `data-goto`: `sales.csv`→overview,
`tradeoffs.csv`→compare, `events.parquet`→grid, `pg · crm`→query,
`ai · byo key`→privacy, `q2.dat0`→replay, `q2-july.dat0`→seal, `waitlist.dat0`→waitlist.

---

## 6. Interaction

### Accordion
One pane open. Open = `flex:1 1 auto; min-height:360px`, body `opacity:1`, chevron
`rotate(0)`, border `paneEdge`, `box-shadow: paneShadow`. Closed = `flex:0 0 32px`,
body `opacity:0`, chevron `rotate(-90deg)`, border `paneEdgeDim`, no shadow.
Opening also scrolls the stack so the pane's header is at the top (`offsetTop − 6`).

### Navigation
| Input | Behavior |
|---|---|
| Wheel | Open pane scrolls its own body first; only at the scroll end does the wheel step panes. **One gesture = one pane**: `_wheelLock` set on step, cleared 30 ms after the wheel goes quiet; threshold `|Δ| ≥ 24`. |
| `↓` `PageDown` `Space` | next pane |
| `↑` `PageUp` | previous pane |
| `Home` / `End` | overview / waitlist |
| `⌘K` / `Ctrl-K` | toggle palette |
| Click | pane header, tab, sidebar row |

Key handler ignores events from `INPUT`/`TEXTAREA`. Wheel handler is
`{ passive: false }` and calls `preventDefault()` — needed for the hijack.

### ⌘K command palette
Modal, scrim `#1f232840`, `padding-top:14vh`, panel `min(560px,92vw)`.
Input filters the 8 panes on `id + label`; `↑`/`↓` move selection; `⏎` opens; `Esc` closes.
Selected row: `background #e8f1fb`, `box-shadow: inset 2px 0 0 #03459b`.
Empty state: *"no matches — dat0 doesn't ship yet, but the waitlist does."*

### Per-pane first-open animations (each runs once)
| Pane | Animation |
|---|---|
| compare | Rows fade in, `opacity .42s`, stagger `120 + i·70` ms |
| query | SQL lines fade in, stagger `160 + i·180` ms |
| replay | Terminal runs: each line appears; `[data-typed]` lines type at **24 ms/char** with a blinking caret that switches off when the line completes; 170 ms after a typed line, 330 ms after a static one |
| seal | Left card fades up (`opacity`+`translateY(14px)→0`, .6s); 420 ms later the SEALED stamp lands (`scale(2.2)→1` at `rotate(6deg)`, .45s) |

### Ambient
- **Rolling words** — every 1500 ms one of three slots swaps, round-robin (`fmt → jl → jr`),
  to a *random other* index. 3D roll: out `rotateX(0→−90deg)` 220 ms, in `rotateX(90→0deg)`
  240 ms delayed 220 ms, `perspective(520px)`. Alternating A/B keyframe pairs so a
  re-trigger restarts cleanly. Each slot has a fixed `width` (`4em`, `4.9em`, `6em`)
  and an amber `inset 0 -3px 0` underline, so no reflow.
  - formats: `Parquet CSV JSON SQLite Arrow DuckDB`
  - join left: `a CSV · a Parquet · a JSON · a SQLite · an Arrow · a DuckDB`
    (article rolls with the word; also drives the SQL `FROM 'sales.csv'` line)
  - join right: `Postgres MySQL ClickHouse Snowflake BigQuery MotherDuck`
    (alias `pg my ch sf bq md` drives the SQL `JOIN pg.crm.accounts`)
- **Scan sweep** — amber gradient bar, 6.5 s linear infinite, down the overview grid.
- **Live pulse** — 2.4 s ease-in-out infinite on the "in development" dot, the
  `pg · crm` dot, and the status-bar engine dot.
- **Caret blink** — 1.1 s step-end infinite.
- **Row counter** — the status bar's row number is a function of pane index:
  `round(1048571 + (1200448930 − 1048571) · (i/7)^1.4)`. Scrolling panes reads as
  scrolling a 1.2 B-row file.

### Reduced motion (authored)
`@media (prefers-reduced-motion: reduce)`: scan sweep `display:none`, rolling words
`animation:none`, roll-out layer `display:none`, pane transition `none`.
**Note:** the authored rule does *not* cover the terminal typing or the seal reveal.
The repo standard is `<MotionConfig reducedMotion="user">` plus honoring the query —
those two need explicit handling.

---

## 7. Open questions / deviations required

These are places where the design cannot be ported literally.

1. **Mobile.** No design below 900px beyond hiding the sidebar and one button. A
   `100vh` wheel-hijacked accordion needs a genuine small-screen mode (most likely:
   drop the hijack, let panes stack and scroll natively, keep the chrome).
2. **Wheel hijack + a11y.** Overriding scroll is a real accessibility cost and fights
   the repo's a11y gate (`eslint-plugin-jsx-a11y` recommended, Playwright checks).
   Needs an escape hatch and must not trap keyboard users.
3. **Fake form.** The design's forms are local `setState` mocks. The real
   `WaitlistForm` island (`useActionState`, Zod, honeypot, 5 states) replaces both
   mounts verbatim — do **not** port the design's form markup or its single success state.
4. **Honesty gate.** The design already self-labels well ("concept render — the
   workbench is designed, not shipped", "designed, not shipped", "not shipped yet",
   the persistent "in development" pill). Preserve every one. The forbidden-CTA grep
   (`Download`, `Sign up`, `Pricing`, `Get started`, …) must still pass — v4's only
   CTA is "Join the waitlist". ✅ compatible.
5. **Dead links.** `href="#"` on *Watch the build*, *docs*, *github*, *x / twitter* —
   need real targets (repo already has `GITHUB_URL`).
6. **LCP.** The hero must stay a Server Component (existing hard gate, 2500 ms CI
   budget). The shell's interactivity must be islands around server-rendered pane
   content, not a client-rendered page.
7. **Token system.** v4's palette is a second, larger palette than the current
   `:root`. It has to land in `app/globals.css` **and** `lib/tokens.ts` together
   (single-source rule) and clear `scripts/check-contrast.mjs`.
8. **Test fallout.** `tests/marketing.contract.test.mjs`, `tests/landing.spec.ts`,
   `tests/motion.contract.test.mjs`, `tests/responsive-overflow.spec.ts` all assert
   the v1.0 arc. They gate CI and will need rewriting against v4's contract.
9. **Orphaned components.** `components/artifact/*`, `components/motion/worksheet-compiler.tsx`,
   `components/sections/{hero,finale}.tsx`, `lib/dat0-format.ts` lose their only
   consumer. Deletion is a separate call — the docs `/spec` pages still reference the
   chunk model conceptually.

---

## 8. Verbatim copy

Locked strings, to be lifted exactly.

**Overview**
> Your data outgrew the spreadsheet.
> It never outgrew your laptop.

> dat0 is a native, local-first data workbench. Open a 12 GB Parquet like a 5 MB CSV,
> join it to live Postgres in one SELECT, and seal the whole analysis — data, queries,
> lineage — into a single file anyone can replay, inspect, or diff.

> No spam — one email when there's something to run.
> macOS + Linux · rust-native · apache-2.0 · built in public by Accidentally Awesome Labs
> Every spreadsheet stops on this row.
> xlsx hard limit · 1,048,576 · everything below is unreachable
> 1,199,400,352 more rows — read straight off your disk
> dat0 scrolls every row at 60 fps.
> ✓ end of file reached · nothing imported · 0 bytes left this machine
> concept render — the workbench is designed, not shipped · scroll to open the next pane ↓

**Compare**
> Every other tool asks for a trade.

> Spreadsheets trade away scale. The cloud trades away custody. Notebooks trade away
> proof. dat0's whole design is refusing that menu — so here it is as a result set,
> honest cells included.

Rows: `Open a 12 GB file` · `File ⋈ live database` · `Where the rows live` ·
`Re-run it next month` · `Diff two analyses` · `Hand it to a colleague` · `→ trades away`.
Columns: `capability` · `dat0` · `spreadsheet` · `cloud bi / warehouse` · `notebook`.

**Grid**
> Drop a 12 GB ⟨format⟩ file. Scroll like it's 5 MB.

> CSV, Parquet, JSON, SQLite — dat0 opens files in place with a native DuckDB engine
> and a GPU-drawn 60 fps grid. Not a browser tab, not Electron: Rust, on your metal.
> No upload, no import wizard, no waiting.

**Query**
> Join ⟨a CSV⟩ file with a ⟨Postgres⟩ table. One query.

> Point dat0 at files and live databases and ask in one SELECT — no ETL, no load step,
> no warehouse bill. Full SQL with autocomplete. Prefer plain English? A
> bring-your-own-key AI drafts the query; you review before anything runs.

> ai draft · bring your own key — off until you add one
> sent → column names only · never sent → row values, results, keys

**Replay**
> Next month's report is one command.

> A .dat0 isn't a dead export — it replays. Point last month's analysis at a fresh
> extract and every transform re-runs, with a schema check standing guard. Then diff
> two analyses like code: schema, lineage, row counts, queries. Unix exit codes, so CI
> can watch your numbers.

Terminal, in order:
```
$ dat0 replay q2.dat0 --source sales.csv=./july.csv     ← typed
✓ schema compatible — 14 transforms replayed
→ wrote q2-july.dat0 · sha-256 verified
$ dat0 diff q2.dat0 q2-july.dat0                         ← typed
~ monthly · 12 → 13 rows
+ saved query "july outliers"
exit 1 — differences found (scriptable)
$ ▮
```
> per the v1 CLI design · dat0-packages.md — designed, not shipped

**Seal**
> One file. Sealed. Yours.

> Data, transforms, queries, session, lineage — the whole workflow packs into a single
> .dat0 file with a sha-256 chain. Email it, archive it, attach it to the issue. Inside
> it's plain Parquet and tagged JSON, so pandas, Polars, or Spark can read it without
> dat0 installed.

> opens without dat0 → plain parquet + tagged json · pandas · polars · spark

Chunks: `00 · data` parquet · 12.4 GB — `01 · transforms` 14 steps · json —
`02 · queries` 6 saved — `03 · session` layout · sort · filters —
`04 · lineage` sha-256 9f2e…c41a.

**Privacy**
> Not a promise.
> A code path.

> Every tool says "privacy-first." dat0 is open source, so you don't have to take the
> word for it — the guarantees are functions you can read. AI is bring-your-own-key
> (Anthropic, OpenAI, OpenRouter, or your local Ollama) and off until you add one.

Receipts: `telemetry/redaction.rs` · `ai/schema_ctx.rs` · `ai/ssrf.rs` · per `docs/privacy.md`.

| your SQL & results | **never** transmitted |
| row values → AI | **never** by default — schema names only |
| API keys | OS keychain — **never** in files, logs, telemetry |
| crash reports | opt-in, self-hosted, paths redacted |
| cloud compute | only when you attach it — **0 bytes** egress by default |

Deliberately not: `✕ a BI tool — no dashboards to babysit` · `✕ a notebook — no hidden
state` · `✕ a DB client — files come first` · `✕ cloud-required — ever`
> A tool that tries to be everything ends up being a browser tab. dat0 is a workbench:
> files in, proof out.

**Waitlist**
> The workflow is one file.
> workflow.dat0 · 5 chunks · sha-256 9f2e…c41a · designed, not shipped

`lineage.json`:
```
;; every other tool asks for a trade: scale, custody, or proof.
;; notebooks hide their state. dashboards can't explain themselves.
;; dat0 doesn't ship yet — this page is the design.
;; the waitlist is chunk 05: write access is you.
```

**Chrome**
Titlebar: `in development` · `Watch the build`.
Status bar: `engine duckdb · native` · `mem 412 MB` · `rows N / 1,200,448,930` ·
`60 fps` · `designed and developed by Accidentally Awesome Labs` · `⌘K commands`.
Sidebar footer: `session · 1 window · 3 tabs` · `ai provider not configured` · `egress 0 B`.
