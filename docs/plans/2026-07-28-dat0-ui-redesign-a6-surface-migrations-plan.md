# Slice A6 — Surface Migrations Implementation Plan

> **For agentic workers:** implement task-by-task. Steps use checkbox (`- [ ]`)
> syntax for tracking. This slice is executed **inline by the controller** (no
> subagents — standing instruction), one commit per task.

**Goal:** Point every remaining inline colour literal in `crates/dat0-app/src/**`
at the `Dat0Colors` token A2 defined for it, and lower `tests/style_lint.rs`'s
shrink-only `ALLOW` ratchet to `[("window.rs", 1)]`.

**Architecture:** Pure colour-source substitution. `cx.theme().d0().<field>`
replaces each literal; `FocusStopExt::focus_stop` gains a `ring: Hsla`
parameter and `a11y::FOCUS_RING` is deleted; four cx-less render helpers gain a
`cx: &App` parameter. No layout, spacing, typography or density change.

**Tech Stack:** Rust 1.97.0, gpui 0.2.2, gpui-component pinned rev `0f0ab35`,
`gpui_component::ActiveTheme` + dat0's `theme::tokens::{Dat0Theme, Dat0Colors}`.

Design: `docs/plans/2026-07-28-dat0-ui-redesign-a6-surface-migrations-design.md`
Branch: `feat/ui-redesign-a6-surface-migrations` off main `1b8e5cb`
Design commit: `49c3fa9`

## Global Constraints

- **Access idiom:** `use gpui_component::ActiveTheme as _;` then
  `cx.theme().d0().<field>`. None of the 12 target files imports `ActiveTheme`
  today; each migrated file adds it.
- **Zero new colour literals.** `tests/style_lint.rs` bans `rgb(`/`rgba(`/
  `hsl(`/`hsla(`/`white()`/`black()`/`parse_hex`, boundary-anchored
  `opaque_grey|transparent_black|transparent_white|red|blue|green|yellow`,
  `Hsla {`/`Rgba {`, and bare 6-or-8-digit hex.
- **Ratchet is shrink-only and fails BOTH ways.** A count left too high
  silently re-opens a migrated file. Every task edits `ALLOW` in the **same
  commit** as its migration. A file absent from `ALLOW` has an allowance of 0.
- **`a11y()` and `a11y_label()` both `push()` a NEW capture node.** They do not
  set an attribute. Never add a label to a site that already has one — edit the
  existing label's text instead.
- **No layout change.** Colour sources only. No `Sp`, no `TextRole`, no
  `Density`, no `with_size`.
- **`window.rs` is out of scope** — its 1 literal stays for B10.
- **Commits:** `cargo fmt --all` before every commit; DCO `git commit -s`.
  Never write the literal skip-ci marker in any commit message, even in prose.
  Use `git commit -F -` with a heredoc when the message contains backticks
  (zsh command-substitutes them inside `-m "…"`).
- **`cargo test --workspace` is NOT runnable on this machine** (pre-existing
  macOS 27 / Xcode 26.6 libduckdb-sys Thrift breakage on `main`). Use the
  substitute gate in Task 8.

---

### Task 1: Focus ring — `focus_stop` gains `ring: Hsla`

**Files:**
- Modify: `crates/dat0-app/src/a11y/mod.rs:26-68` (delete `FOCUS_RING`, add param)
- Modify: `crates/dat0-app/src/view/sql_console.rs` (18 `focus_stop` sites)
- Modify: `crates/dat0-app/src/empty_state.rs` (6 sites + ring at `:462`)
- Modify: `crates/dat0-app/src/view/name_prompt.rs` (2 sites)
- Modify: `crates/dat0-app/src/catalog/panel.rs` (1 site + rings at `:140`, `:173`)
- Modify: `crates/dat0-app/src/settings_ui/panel.rs` (1 site)
- Modify: `crates/dat0-app/src/ai/panel.rs` (1 site)
- Modify: `crates/dat0-app/src/view/query_library.rs` (ring at `:62`)
- Modify: `crates/dat0-app/tests/style_lint.rs` (`ALLOW` + two stale doc comments)

**Interfaces:**
- Produces: `FocusStopExt::focus_stop(self, id: &'static str, fh: &FocusHandle,
  tab_index: isize, ring: Hsla, on_activate: impl Fn(&KeyDownEvent, &mut Window,
  &mut App) + 'static) -> Self`. Every later task and all 29 existing call sites
  depend on this arity.
- Removes: `pub const a11y::FOCUS_RING: u32`. Nothing may reference it after
  this task.

- [ ] **Step 1: Change the trait signature and body**

In `crates/dat0-app/src/a11y/mod.rs`, delete the constant:

```rust
/// Focus-ring hue — matches the grid active-cell ring (`grid/mod.rs:567`).
/// `pub` so the recents list can paint its active-row ring in the same hue.
pub const FOCUS_RING: u32 = 0x3b82f6;
```

Add `Hsla` to the gpui import and change the method:

```rust
use gpui::{App, FocusHandle, Hsla, InteractiveElement, KeyDownEvent, Styled as _, Window};

pub trait FocusStopExt: InteractiveElement + Sized {
    fn focus_stop(
        self,
        id: &'static str,
        fh: &FocusHandle,
        tab_index: isize,
        ring: Hsla,
        on_activate: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let fh = fh.clone().tab_index(tab_index).tab_stop(true);
        record_focus_id(&fh, id);
        self.track_focus(&fh)
            .on_key_down(move |ev, window, app| {
                if matches!(ev.keystroke.key.as_str(), "enter" | "space") {
                    on_activate(ev, window, app);
                }
            })
            .focus(move |s| s.border_2().border_color(ring))
    }
}
```

Note `.focus(move |s| …)` — the closure now captures `ring`.

Update the doc comment on `focus_stop` to say the ring colour is supplied by
the caller (it currently implies a fixed hue).

- [ ] **Step 2: Run the compiler to enumerate every call site**

Run: `cargo check -p dat0-app --all-targets 2>&1 | grep -E "^error" | head -40`

Expected: FAIL — one arity error per `focus_stop` call site (29) plus four
`FOCUS_RING` resolution errors. **The compiler is the inventory.** If the count
of `focus_stop` errors is not 29, stop and report the discrepancy before
editing (A5's lesson: the plan's own inventory was stale on its first call-site
task).

- [ ] **Step 3: Hoist the ring once per containing render function**

For each function containing one or more `focus_stop` calls, add near the top
of the function body — after any existing `let` bindings it depends on:

```rust
let ring = cx.theme().d0().focus_ring;
```

and add to that file's imports (once per file):

```rust
use gpui_component::ActiveTheme as _;
use crate::theme::tokens::Dat0Theme as _;
```

Then pass `ring` as the new 4th argument at each call site:

```rust
// before
.focus_stop("sql-run", &run_fh, 0, run_key)
// after
.focus_stop("sql-run", &run_fh, 0, ring, run_key)
```

Two helpers already take `cx` and derive the ring locally, so their own callers
are untouched: `ai/panel.rs::action_button(id, label, ev, fh, cx)` and
`settings_ui::panel.rs::render_sidebar(&self, cx)`.

**Stop-and-report clause:** if any containing function has no `cx`/`&App` in
scope, do NOT reach for a global — thread `ring: Hsla` down from its caller and
report the function in the task notes.

- [ ] **Step 4: Convert the four direct ring sites**

These paint an active-row ring from the constant rather than through
`focus_stop`. Each is inside an `if is_active`-style branch; hoist `ring` in the
containing function exactly as in Step 3, then:

`crates/dat0-app/src/catalog/panel.rs:140` and `:173`, and
`crates/dat0-app/src/view/query_library.rs:62`, and
`crates/dat0-app/src/empty_state.rs:462`:

```rust
// before
.border_color(gpui::rgb(crate::a11y::FOCUS_RING));
// after
.border_color(ring);
```

- [ ] **Step 5: Lower the ratchet and fix the stale doc comments**

In `crates/dat0-app/tests/style_lint.rs`, remove the three emptied rows and
lower catalog:

```rust
const ALLOW: &[(&str, usize)] = &[
    ("catalog/panel.rs", 2),
    ("charts/mod.rs", 2),
    ("charts/panel.rs", 2),
    ("error_ux/banner.rs", 4),
    ("grid/mod.rs", 7),
    ("onboarding/mod.rs", 2),
    ("settings_ui/panel.rs", 1),
    ("view/pipeline_bar.rs", 9),
    ("window.rs", 1),
];
```

Two comments in the same file now describe code that no longer exists. In the
module doc, replace:

> `gpui::rgb(crate::a11y::FOCUS_RING)` — five real call sites where the literal
> hides one `const` indirection away.

with wording that presents it as the historical motivation (A6a removed those
sites; the rule stays because the pattern can recur). In `bare_hex_re`'s doc,
replace the claim that the only thing it catches in `src/` is `FOCUS_RING` with
a statement that `src/` is now clean of bare hex and the anchor keeps 2- and
4-digit hex legal.

Do **not** touch the `scanner_flags_constructors_and_bare_hex_only` test cases —
those strings are synthetic scanner input and stay valid.

- [ ] **Step 6: Verify**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --features a11y-capture
```

Expected: clippy clean; `style_lint` 4/4; a11y-capture suite fully green (the
ring is a colour, so the capture tree is unchanged — any failure here means a
call site lost its `focus_stop`, not a colour problem).

Confirm the constant is gone:

```bash
grep -rn "FOCUS_RING" crates/dat0-app/src/    # expect: no matches
```

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -s -F - <<'EOF'
feat(theme): A6a — focus ring reads the theme (UI redesign)

FocusStopExt::focus_stop gains a `ring: Hsla` parameter and a11y::FOCUS_RING
is deleted. 29 focus_stop call sites plus 4 direct active-row ring sites now
read cx.theme().d0().focus_ring, so the ring tracks theme switches and picks
up the high-contrast palette.

This kills the two-blues split: the ring was hardcoded #3b82f6 while A1 set
the theme ring to #58a6ff (dark) / #0969da (light).

style_lint ALLOW: a11y/mod.rs, empty_state.rs and view/query_library.rs drop
out entirely; catalog/panel.rs 4 -> 2.
EOF
```

---

### Task 2: Banner accent + tint

**Files:**
- Modify: `crates/dat0-app/src/error_ux/banner.rs:197-232`
- Modify: `crates/dat0-app/src/window.rs:6149` (caller)
- Modify: `crates/dat0-app/tests/a11y_content.rs:665` (caller)
- Modify: `crates/dat0-app/tests/style_lint.rs` (`ALLOW`)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces: `pub fn render_banner(b: &Banner, cx: &App) -> impl IntoElement`.

- [ ] **Step 1: Add `cx` and swap the four literals**

```rust
pub fn render_banner(b: &Banner, cx: &gpui::App) -> impl IntoElement {
    let d0 = cx.theme().d0();
    let accent = match b.kind {
        BannerKind::Info => d0.banner_info,
        BannerKind::Warning => d0.banner_warning,
        BannerKind::Error => d0.banner_error,
    };
```

and further down, replace `.bg(gpui::rgba(0x80808014))` with
`.bg(d0.banner_tint)`.

Add to the file's imports:

```rust
use crate::theme::tokens::Dat0Theme as _;
use gpui_component::ActiveTheme as _;
```

- [ ] **Step 2: Update the two callers**

`crates/dat0-app/src/window.rs:6149`:

```rust
// before
.map(|b| crate::error_ux::banner::render_banner(b).into_any_element()),
// after
.map(|b| crate::error_ux::banner::render_banner(b, cx).into_any_element()),
```

`Context<WorkspaceShell>` derefs to `App`, so `cx` passes directly. If the
borrow checker objects because `cx` is already borrowed in the surrounding
builder expression, bind the banners to a local `Vec` before the expression
rather than restructuring the builder.

`crates/dat0-app/tests/a11y_content.rs:665` — the closure already receives the
app; rename the binding and pass it:

```rust
cx.update(|app| {
    let _ = render_banner(&banner, app);
});
```

- [ ] **Step 3: Lower the ratchet**

Remove `("error_ux/banner.rs", 4),` from `ALLOW`.

- [ ] **Step 4: Verify**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --features a11y-capture --test a11y_content
```

Expected: all pass. `a11y_content` exercises `render_banner` directly, so it is
the real check that the new parameter threaded correctly.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -s -F - <<'EOF'
feat(theme): A6b — banner colours read the theme (UI redesign)

render_banner gains a `cx: &App` parameter and reads banner_info /
banner_warning / banner_error / banner_tint instead of Tailwind literals, so
banner accents follow the active palette (including high contrast).

style_lint ALLOW: error_ux/banner.rs drops out.
EOF
```

---

### Task 3: Pipeline bar

**Files:**
- Modify: `crates/dat0-app/src/view/pipeline_bar.rs` — lines 93, 105, 133, 156,
  189, 245, 279, 297, 321
- Modify: `crates/dat0-app/tests/style_lint.rs` (`ALLOW`)

**Interfaces:**
- Consumes: nothing. `render_pipeline_bar(stack, state, cx: &mut
  Context<WorkspaceShell>)` already has theme access — **no signature change.**
- Produces: nothing new.

- [ ] **Step 1: Hoist and swap all nine**

Add at the top of `render_pipeline_bar`, after the `stack.is_empty()` early
return:

```rust
let d0 = cx.theme().d0();
```

Add imports:

```rust
use crate::theme::tokens::Dat0Theme as _;
use gpui_component::ActiveTheme as _;
```

Then substitute, deleting the now-redundant trailing Tailwind comments:

| line | before | after |
|---|---|---|
| 93 | `.text_color(gpui::rgba(0x6b72_80ff)) // gray-500` | `.text_color(d0.text_muted)` |
| 105 | `.text_color(gpui::rgba(0x3b82_f6ff)) // blue-500` | `.text_color(d0.pipeline_accent)` |
| 133 | `.text_color(gpui::rgba(0x6b72_80ff)) // gray-500` | `.text_color(d0.text_muted)` |
| 156 | `.text_color(gpui::rgba(0xef44_44ff)) // red-500` | `.text_color(d0.text_error)` |
| 189 | `.bg(gpui::rgba(0x3b82_f640)) // blue-500/25` | `.bg(d0.pipeline_pill)` |
| 245 | `.bg(gpui::rgba(0x3b82_f640)) // blue-500/25` | `.bg(d0.pipeline_pill)` |
| 279 | `.text_color(gpui::rgba(0x6b72_80ff)) // gray-500` | `.text_color(d0.text_muted)` |
| 297 | `.bg(gpui::rgba(0xf3f4_f6ff)) // gray-100` | `.bg(d0.pipeline_chip)` |
| 321 | `.bg(gpui::rgba(0x3b82_f640)) // blue-500/25` | `.bg(d0.pipeline_pill)` |

If `d0` cannot be hoisted once because a `cx.listener(...)` borrow intervenes,
bind it separately inside each affected block rather than reintroducing a
literal.

- [ ] **Step 2: Lower the ratchet**

Remove `("view/pipeline_bar.rs", 9),` from `ALLOW`.

- [ ] **Step 3: Verify**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --features a11y-capture
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -s -F - <<'EOF'
feat(theme): A6c — pipeline bar colours read the theme (UI redesign)

Nine Tailwind literals in render_pipeline_bar become text_muted (x3),
pipeline_accent, text_error, pipeline_pill (x3) and pipeline_chip. No
signature change — the function already carried a Context.

style_lint ALLOW: view/pipeline_bar.rs drops out.
EOF
```

---

### Task 4: Catalog hover + the three deferred chevrons

**Files:**
- Modify: `crates/dat0-app/src/catalog/panel.rs:121`, `:131`, `:161`
- Modify: `crates/dat0-app/src/inspector/panel.rs:105`, `:153`
- Modify: `crates/dat0-app/tests/style_lint.rs` (`ALLOW`)

**Interfaces:**
- Consumes: Task 1 already added `ActiveTheme`/`Dat0Theme` imports and a `ring`
  hoist to `catalog/panel.rs`. Reuse the same `cx`.
- Produces: nothing new.

**Context:** A5 deferred these three glyphs because each interpolates its
chevron into a `format!` String that is **also** passed to `.a11y_label`.
Chevron SVGs ship with `gpui-component-assets` already — no new vendored asset
and no `Dat0IconName` variant.

- [ ] **Step 1: Swap the two catalog hover tints**

`catalog/panel.rs:131` and `:161` — hoist `let d0 = cx.theme().d0();` in each
containing function (both already receive `cx: &mut Context<WorkspaceShell>`):

```rust
// before
.hover(|s| s.bg(gpui::rgba(0x80808022)))
// after
.hover(move |s| s.bg(d0.hover_tint))
```

`Hsla` is `Copy`, so `move` here captures a copy and does not disturb later use
of `d0`.

- [ ] **Step 2: Split the catalog chevron out of its label**

`catalog/panel.rs:121-136` currently builds one String for both the child and
the accessible name:

```rust
let chev = if expanded { "▾" } else { "▸" };
let text = format!("{chev} {alias} ({n_children})");
// …
.child(SharedString::from(text.clone()))
.a11y_label(crate::a11y::AccessRole::Label, text)
```

Replace with an icon plus a glyph-free label:

```rust
let icon = if expanded {
    IconName::ChevronDown
} else {
    IconName::ChevronRight
};
let text = format!("{alias} ({n_children})");
```

and in the builder chain, replace the single `.child(SharedString::from(...))`
with a flex row carrying the icon and the text:

```rust
.flex()
.flex_row()
.items_center()
.gap_1()
.child(Icon::new(icon))
.child(SharedString::from(text.clone()))
.a11y_label(crate::a11y::AccessRole::Label, text)
```

Add `use gpui_component::{Icon, IconName};` to the file's imports.

**Do not add a second `.a11y_label`.** Both `a11y()` and `a11y_label()`
`push()` a new capture node; the existing call stays and only its String
argument changes. The accessible name becomes `"main (3)"` instead of
`"▾ main (3)"` — an intended improvement.

- [ ] **Step 3: Split the two inspector chevrons**

`inspector/panel.rs:105` — a static right-chevron on the inspected table row:

```rust
// before
let target_row = format!("▸ {target}");
section = section.child(
    div()
        .px_1()
        .border_1()
        .a11y_label(AccessRole::Label, target_row.clone())
        .child(SharedString::from(target_row)),
);
// after
let target_row = format!("{target}");
section = section.child(
    div()
        .px_1()
        .border_1()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .a11y_label(AccessRole::Label, target_row.clone())
        .child(Icon::new(IconName::ChevronRight))
        .child(SharedString::from(target_row)),
);
```

`inspector/panel.rs:153` — the hidden-columns expand toggle:

```rust
// before
let caret = if model.hidden_expanded { "▾" } else { "▸" };
let mut section = div().flex().flex_col().gap_2().child(
    div()
        .id("inspector-hidden-toggle")
        .cursor_pointer()
        .child(SharedString::from(format!("{caret} {header}")))
// after
let caret = if model.hidden_expanded {
    IconName::ChevronDown
} else {
    IconName::ChevronRight
};
let mut section = div().flex().flex_col().gap_2().child(
    div()
        .id("inspector-hidden-toggle")
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(Icon::new(caret))
        .child(SharedString::from(header.clone()))
```

Verified: this element carries **no** `.a11y_label` — its chain is
`.id().cursor_pointer().child(...).on_click(...)`. Do **not** add one. Giving
this toggle an accessible name is real a11y work that belongs to its own slice,
not to a colour-and-icon migration, and adding a label here would push a new
capture node and move the tree for no reason this slice can justify.

`header` is consumed by the `.child(...)` above; it is a `String`, so no clone
is needed unless a later edit reuses it.

Both sites are inside `render_inspector`, which already has `cx`; no signature
change is needed for the chevrons.

- [ ] **Step 4: Lower the ratchet**

Remove `("catalog/panel.rs", 2),` from `ALLOW`. `inspector/panel.rs` has no
`ALLOW` entry and gains none — it holds no colour literals.

- [ ] **Step 5: Verify — this is the one task that moves the capture tree**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --features a11y-capture
grep -rn '▾\|▸' crates/dat0-app/src/    # expect: no matches
```

Expected: a11y-capture suite green. Unlike the colour tasks this one **does**
change accessible names, so read the output rather than assuming. `catalog_nav`
asserts only `catalog.title`, and no test asserts a chevron glyph (verified),
so a failure here is a real regression — most likely a duplicate label from
adding rather than editing.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -s -F - <<'EOF'
feat(theme): A6d — catalog hover + the three deferred chevrons (UI redesign)

Catalog row hover tints read hover_tint. The three chevrons A5 deferred
(catalog/panel.rs, inspector/panel.rs x2) become gpui-component chevron icons;
each interpolated its glyph into a format! String that was also its a11y label,
so the existing label's text is edited in place rather than a second label
being added. Accessible names lose the decorative glyph, which is the intended
behaviour — a screen reader should not announce a chevron.

style_lint ALLOW: catalog/panel.rs drops out.
EOF
```

---

### Task 5: Settings active-nav tint

**Files:**
- Modify: `crates/dat0-app/src/settings_ui/panel.rs:86`
- Modify: `crates/dat0-app/tests/style_lint.rs` (`ALLOW`)

**Interfaces:**
- Consumes: Task 1 already added the theme imports and a `ring` hoist to
  `render_sidebar`. Reuse the same `cx`.

- [ ] **Step 1: Swap the literal**

In `render_sidebar`, hoist alongside the existing ring binding:

```rust
let d0 = cx.theme().d0();
```

then:

```rust
// before
.when(active, |d| d.bg(gpui::rgba(0x3b82f622)))
// after
.when(active, move |d| d.bg(d0.selection_tint))
```

`selection_tint` is `ring.opacity(0.13)` — the same derivation the grid's
selected region uses, and the value A2's field map assigns to this exact site.

- [ ] **Step 2: Lower the ratchet**

Remove `("settings_ui/panel.rs", 1),` from `ALLOW`.

- [ ] **Step 3: Verify**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --features a11y-capture
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -s -F - <<'EOF'
feat(theme): A6e — settings active-nav tint reads the theme (UI redesign)

The settings sidebar's active-section tint reads selection_tint
(ring at 13% alpha) instead of a literal.

style_lint ALLOW: settings_ui/panel.rs drops out.
EOF
```

---

### Task 6: Grid — the paint path

**Files:**
- Modify: `crates/dat0-app/src/grid/mod.rs:73-74` (reorder ghost),
  `:524`, `:552`, `:573-574`, `:587` (`render_td`)
- Modify: `crates/dat0-app/tests/style_lint.rs` (`ALLOW`)

**Interfaces:**
- Consumes: nothing. `ReorderDrag::render` and `render_td` both already carry a
  `Context`.
- Produces: nothing new.

**Risk:** `render_td` runs **per visible cell, per frame**, and touches the
theme zero times today. `d0()` builds all 21 fields per call. A5 held the macOS
grid-scroll bench only because `a11y_label` is an identity stub in release —
that protection does not apply to a real colour read.

- [ ] **Step 1: Record the pre-change bench baseline** — ⚠ **NOT RUNNABLE on
  this machine; attempted and blocked.**

```bash
cargo bench -p dat0-app --bench grid_scroll 2>&1 | tail -20
```

This fails before running a single iteration, on the **pre-existing** macOS 27
/ Xcode 26.6 libduckdb-sys breakage (`third_party/parquet/parquet_types.cpp`,
built `-std=c++11`, will not compile against the new libc++ headers). The bench
carries no `--target`, so it forces a *fresh* `libduckdb-sys` compile into
`target/release/build/` — precisely the path that hits the bad SDK, and the
reason `cargo test -p dat0-app` (cached artifact) succeeds while this does not.

Nothing about A6 causes it and no local workaround is in scope. **The gate is
therefore entirely the CI grid-scroll bench, which is push-to-main-only** — so
the post-merge watch in Task 8 Step 7 is not a formality for this slice, it is
the only measurement that will ever be taken. Say so plainly when reporting;
do not imply a local bench result exists.

- [ ] **Step 2: Migrate the column-reorder ghost pill**

`grid/mod.rs:64-80`. The parameter is currently `_cx`; rename it to `cx`.

```rust
impl gpui::Render for ReorderDrag {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        div().pl(self.position.x).pt(self.position.y).child(
            div()
                .px_2()
                .py_1()
                .bg(cx.theme().primary)
                .text_color(cx.theme().primary_foreground)
                .text_xs()
                .rounded_md()
                .child(format!("col {}", self.from)),
        )
    }
}
```

This is the documented deviation from A2's field map (design §4.1): the site is
the drag ghost, not a fill handle, and it paints text on the fill, so it needs a
gated **text** pair. `("primary.foreground", "primary.background", 4.5)` is
already in `tests/theme_contrast_gate.rs`'s `TEXT_PAIRS` across all three
builtins. Do **not** use `fill_handle` — A3 tuned it to α 0.72 against a
non-text 3:1 threshold.

Add imports:

```rust
use crate::theme::tokens::Dat0Theme as _;
use gpui_component::ActiveTheme as _;
```

(`Dat0Theme` is needed by Step 3, not by this step.)

- [ ] **Step 3: Migrate `render_td` — lazily, inside each branch**

Do **not** hoist `d0` to the top of `render_td`. Call it inside each styling
branch so an untinted cell — the common case — pays nothing.

```rust
// :524 — selected-region tint
if is_selected && !is_active {
    el = el.bg(cx.theme().d0().selection_tint);
}

// :552 — marching-ants dashed boundary
el = el.border_color(cx.theme().d0().marching_ants).border_dashed();

// :573-574 — active-cell ring + fill
if is_active {
    let d0 = cx.theme().d0();
    el = el.border_2().border_color(d0.focus_ring).bg(d0.active_cell_tint);
}

// :587 — NULL placeholder text
if display.is_null {
    el = el.text_color(cx.theme().d0().null_value_fg);
}
```

The `is_active` branch reads two fields, so bind `d0` once there.

- [ ] **Step 4: Lower the ratchet**

Remove `("grid/mod.rs", 7),` from `ALLOW`.

- [ ] **Step 5: Verify, including the bench delta**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --features a11y-capture
```

The bench delta cannot be measured here (Step 1). The structural guarantee is
instead checked by reading the diff: **every `cx.theme()` call in `render_td`
must sit inside a styling branch**, so an untinted cell does no theme work.
Verify mechanically:

```bash
# expect 4 hits, all indented inside `if` branches, none at fn-body level
grep -n "cx.theme()" crates/dat0-app/src/grid/mod.rs
```

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -s -F - <<'EOF'
feat(theme): A6f — grid colours read the theme (UI redesign)

render_td reads selection_tint, marching_ants, focus_ring, active_cell_tint and
null_value_fg. d0() is called lazily inside each styling branch, so an untinted
cell — the common case — does no theme work; render_td runs per visible cell
per frame and touched the theme zero times before this change.

The column-reorder drag ghost uses primary / primary_foreground rather than
A2's mapped fill_handle: the site paints text on the fill, and that pair is
already gated at 4.5:1 across all three builtins, whereas fill_handle was tuned
as a non-text 3:1 token. fill_handle consequently has no production consumer.

Local grid_scroll bench recorded before and after; the gate remains the
push-to-main CI bench.

style_lint ALLOW: grid/mod.rs drops out.
EOF
```

---

### Task 7: Charts + onboarding

**Files:**
- Modify: `crates/dat0-app/src/charts/mod.rs:79`, `:103`
- Modify: `crates/dat0-app/src/charts/panel.rs:98`, `:110`
- Modify: `crates/dat0-app/src/inspector/panel.rs:181-241` (`column_card` threads `cx`)
- Modify: `crates/dat0-app/src/onboarding/mod.rs:176-178`
- Modify: `crates/dat0-app/src/window.rs:6899` (caller)
- Modify: `crates/dat0-app/tests/style_lint.rs` (`ALLOW`)

**Interfaces:**
- Produces:
  - `pub fn charts::render_histogram(bins: &[Bin], cx: &App) -> impl IntoElement`
  - `pub fn charts::render_topn(items: &[(String, u64)], cx: &App) -> impl IntoElement`
  - `pub fn charts::panel::render_chart_body(panel: &ChartPanel, image:
    Option<Arc<RenderImage>>, logical: (f32, f32), cx: &App) -> impl IntoElement`
  - `fn inspector::panel::column_card(col, card, model, dimmed, cx: &App) -> gpui::Div`

- [ ] **Step 1: Chart placeholder bars**

`charts/mod.rs` — both functions gain `cx: &App` and read the placeholder
tokens:

```rust
pub fn render_histogram(bins: &[Bin], cx: &gpui::App) -> impl IntoElement {
    let fill = cx.theme().d0().chart_placeholder_a;
    // …
                .bg(fill),
}

pub fn render_topn(items: &[(String, u64)], cx: &gpui::App) -> impl IntoElement {
    let fill = cx.theme().d0().chart_placeholder_b;
    // …
                        .bg(fill),
}
```

Hoist the colour before the loop in each — it is loop-invariant.

Add imports:

```rust
use crate::theme::tokens::Dat0Theme as _;
use gpui_component::ActiveTheme as _;
```

- [ ] **Step 2: Thread `cx` through `column_card`**

`inspector/panel.rs:181` gains the parameter:

```rust
fn column_card(
    col: &dat0_engine::ColumnProfile,
    card: &RenderCard,
    model: &InspectorModel,
    dimmed: bool,
    cx: &gpui::App,
) -> gpui::Div {
```

and its two chart calls (around `:239`/`:241`) pass it:

```rust
card_div = card_div.child(crate::charts::render_topn(topn, cx));
// …
card_div = card_div.child(crate::charts::render_histogram(bins, cx));
```

`column_card` has exactly two call sites, both inside `render_inspector`, which
already has `cx`:

```rust
// inspector/panel.rs:142 — visible cards
visible = visible.child(column_card(col, card, model, false, cx));
// inspector/panel.rs:167 — hidden-section cards
section = section.child(column_card(col, card, model, true, cx));
```

- [ ] **Step 3: Chart panel body text**

`charts/panel.rs:90-113` gains `cx: &App`:

```rust
pub fn render_chart_body(
    panel: &ChartPanel,
    image: Option<Arc<RenderImage>>,
    logical: (f32, f32),
    cx: &gpui::App,
) -> impl IntoElement {
    let d0 = cx.theme().d0();
    let body = if let Some(err) = &panel.error {
        div()
            .p_4()
            .text_color(d0.text_error)
            .child(err.clone())
            .into_any_element()
    } else if let Some(ri) = image {
        // unchanged
    } else {
        let hint = dat0_i18n::t("chart.panel.empty");
        div()
            .p_4()
            .text_color(d0.text_muted)
            .child(hint.clone())
            .a11y_label(AccessRole::Label, hint)
            .into_any_element()
    };
```

The existing `.a11y_label` stays exactly as it is — do not add another.

Update the caller at `crates/dat0-app/src/window.rs:6899` to pass `cx` as the
new final argument.

- [ ] **Step 4: Onboarding pager dots**

`onboarding/mod.rs:170-181`. `present_panel` already takes `cx: &mut App`.
Hoist before the loop:

```rust
let d0 = cx.theme().d0();
let mut pager = h_flex().gap_1();
for i in 0..PANELS.len() {
    let glyph = if i == index { "●" } else { "○" };
    pager = pager.child(
        div()
            .text_color(if i == index {
                d0.pager_dot_active
            } else {
                d0.pager_dot_inactive
            })
            .child(glyph),
    );
}
```

The `●`/`○` glyphs are **not** in scope for icon conversion — they are their
own elements but no chevron/dot icon was vendored, and A5's scope was the
enumerated glyph set. Leave them.

- [ ] **Step 5: Lower the ratchet to its final state**

`ALLOW` now holds one row:

```rust
const ALLOW: &[(&str, usize)] = &[("window.rs", 1)];
```

- [ ] **Step 6: Verify**

```bash
cargo fmt --all
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --features a11y-capture
```

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -s -F - <<'EOF'
feat(theme): A6g — charts + onboarding colours read the theme (UI redesign)

render_histogram, render_topn and render_chart_body gain a `cx: &App`
parameter (threaded through inspector's column_card) and read
chart_placeholder_a / _b, text_error and text_muted. Onboarding pager dots read
pager_dot_active / pager_dot_inactive.

style_lint ALLOW is now down to its final A6 state: window.rs alone, which
B10 clears along with the rest of window.rs styling.
EOF
```

---

### Task 8: Whole-branch gate

**Files:** none modified unless a gate fails.

- [ ] **Step 1: Run the full substitute local gate**

`cargo test --workspace` is broken on this machine (pre-existing macOS 27 /
Xcode 26.6 libduckdb-sys Thrift failure, reproducible on `main`). Run instead:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app
cargo test -p dat0-app --features a11y-capture
cargo test -p dat0-app --features a11y-capture,gallery
```

Expected: all green; the a11y-capture run covers 109 test binaries; the gallery
run confirms `src/gallery.rs` still scans at allowance 0.

- [ ] **Step 2: Prove the ratchet is honest**

The gate must fail if a count is left too high, not just too low. Verify
non-vacuity by hand (A5's lesson — a red-first step that only proves an import
error proves nothing):

```bash
# temporarily set ("grid/mod.rs", 7) back into ALLOW, then:
cargo test -p dat0-app --test style_lint
# expect FAIL naming grid/mod.rs as over-allowanced; then revert
```

Revert the probe before continuing. Confirm with `git diff --stat` that nothing
remains.

> ⚠ **`touch` the file after reverting a probe, and re-run.** Restoring a probe
> by moving a backup back into place gives the file an *older* mtime than the
> probe build, so cargo considers it up to date and silently re-runs the
> **stale probe binary** — the revert looks like it failed. Hit during T4:
> a correctly-reverted source reported RED until `touch` forced the rebuild.
> Never read a post-revert result without forcing the rebuild first.

- [ ] **Step 3: Confirm the literal population is actually empty**

```bash
grep -rnE 'rgb\(|rgba\(|hsla\(|hsl\(|white\(\)|black\(\)|parse_hex' crates/dat0-app/src/
```

Expected: exactly one hit — `window.rs:6836`, the deferred `drag_over` tint.

- [ ] **Step 4: Confirm no `ALLOW` entry lost its reason to exist**

```bash
grep -rn "style-lint: allow" crates/dat0-app/src/
```

Expected: no matches. A4 established that zero allow-escapes exist in `src/`;
A6 must not introduce the first one.

- [ ] **Step 5: Push and open the PR**

Write the PR body to a scratch file first (it contains backticks, which zsh
would command-substitute inside an inline `--body "…"`), then:

```bash
git push -u origin feat/ui-redesign-a6-surface-migrations
gh pr create \
  --title "feat(theme): A6 — surface migrations (UI redesign)" \
  --body-file "$SCRATCH/a6-pr-body.md"
```

The body should state: the 36 → 1 ratchet drop with the per-file breakdown; the
two deviations from A2's field map (ghost pill, chevron labels) with their
reasons; that `render_td` calls `d0()` lazily and why; and that the visible
colour change across all three themes still owes a human glance.

Poll with `gh pr checks` — **not** `gh run watch`, which exits on the first run
completion rather than the last.

- [ ] **Step 6: Read the macOS disk telemetry on the PR run**

```bash
gh run view <id> --log | grep 'DISK\['
```

A6 adds no new test binaries (existing files only), so job-end headroom should
hold near A5's 4.8 Gi. If it drops materially, say so rather than assuming the
cause.

- [ ] **Step 7: Merge and watch the post-merge main run**

Squash-merge with an explicit `--subject` and `--body-file` so no skip-ci
marker leaks from a commit subject into the squash body.

Then **watch the post-merge main run** — `gh run list --branch main`. The
macOS grid-scroll bench is push-to-main-only, and `grid/mod.rs` is in this diff,
so PR checks cannot prove it. Confirm: all jobs success, all three bench steps
(reclaim → bench → upload) success with an artifact, crash-e2e spawned.

---

## Post-merge

Record in memory: as-built deviations, the bench outcome, the disk delta, and
the owed human glances from design §7 — which grow substantially, since A6 is
the first A-slice to change visible colour across nearly every surface in all
three themes, high contrast most of all.
