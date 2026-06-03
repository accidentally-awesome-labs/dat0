# gpui-component API surface notes (P3b T0 spike)

This document is the authoritative reference for the `gpui-component` widget
surfaces used by P3b (T2–T13). Tasks MUST defer to this file when plan snippets
contradict the actual API.

- **Verification date:** 2026-05-25
- **Verifier:** P3b.T0 spike (read-only inspection of vendored source in
  `~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/0f0ab35/`)
- **gpui-component pinned commit:** `0f0ab35233212f8f3277028995caf0c41e13ee6c`
  (tag `v0.5.1`, recorded in `docs/upstream-watch.md`)
- **gpui version:** `=0.2.2` (crates.io), publish commit
  `08d95ad9d31f616a43dacda8416568d658dca6ae`
- **Cross-references:**
  - `docs/internal/gpui-api-notes.md` — gpui v0.2.2 core surface, Dialog/Sheet
    primitives (§0.5b), file drop (§0.9), globals (§0.A — appended by this
    spike).
  - `docs/internal/gpui-table-api-notes.md` — P3a.T0 spike of `TableDelegate`.
    This file re-verifies + extends.

---

## 1. Table (re-verification of P3a.T0 spike against the same pinned commit)

The `TableDelegate` trait and `Column` type documented in
`gpui-table-api-notes.md` were re-checked against
`~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/0f0ab35/crates/ui/src/table/`
on 2026-05-25. **No drift detected** at the pinned commit. The P3a spike
remains canonical for `TableDelegate`; the answers below cite the same source.

### 1.1 Constructor signatures (verbatim)

`TableState::new` — `crates/ui/src/table/state.rs:120`:

```rust
pub fn new(delegate: D, _: &mut Window, cx: &mut Context<Self>) -> Self
```

Construction shape (parent view stores `Entity<TableState<D>>`):

```rust
let table_state = cx.new(|cx| TableState::new(delegate, window, cx));
```

`Table::new` — `crates/ui/src/table/mod.rs:65`:

```rust
pub fn new(state: &Entity<TableState<D>>) -> Self
```

`Table<D>` is `#[derive(IntoElement)]` (one-shot render element); `TableState<D>`
holds persistent state on the parent view.

### 1.2 Delegate trait — `GridDataSource` already implements this for P3a

Source: `crates/ui/src/table/delegate.rs` — verbatim trait body documented in
`gpui-table-api-notes.md` §1.

**Required to implement (no default body):**
- `fn columns_count(&self, cx: &App) -> usize`
- `fn rows_count(&self, cx: &App) -> usize`
- `fn column(&self, col_ix: usize, cx: &App) -> &Column`
- `fn render_td(&mut self, row_ix: usize, col_ix: usize, window: &mut Window, cx: &mut Context<TableState<Self>>) -> impl IntoElement`

**Useful overrides for P3b T7 (Table mount):**
- `fn loading(&self, cx: &App) -> bool` — drives the built-in skeleton
- `fn is_eof(&self, cx: &App) -> bool` — return `!self.loading` so paged loads aren't re-entrant (see P3a notes §4.5 for the counter-intuitive polarity)
- `fn load_more(&mut self, ...)` — kicks off background paged fetch
- `fn visible_rows_changed(&mut self, visible_range: Range<usize>, ...)` — prefetch hook

**Trait bounds:** `TableDelegate: Sized + 'static`. **No `Send`/`Sync`.** The
delegate runs on the GPUI main thread; an `Arc<DuckDBEngine>` field is fine.
(Re-verified — P3a notes §4.1 still authoritative.)

### 1.3 Theme is implicit via `cx.theme()` (NO explicit prop)

Source: `crates/ui/src/table/state.rs` — `cx.theme()` is called throughout the
default render (sample lines `:643`, `:683`, `:684`, `:769`, `:774`, `:775`,
`:782`, `:838`, `:891`, `:892`):

```rust
el.bg(cx.theme().table_active)
.bg(cx.theme().table_row_border)
.text_color(cx.theme().table_head_foreground)
.border_color(cx.theme().border)
```

`cx.theme()` is the `ActiveTheme` trait method defined at
`crates/ui/src/theme/mod.rs:28-37`:

```rust
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}
impl ActiveTheme for App {
    #[inline(always)]
    fn theme(&self) -> &Theme { Theme::global(self) }
}
```

Which dispatches to the global lookup at `crates/ui/src/theme/mod.rs:103-105`:

```rust
pub fn global(cx: &App) -> &Theme { cx.global::<Theme>() }
```

`Theme` implements `Global` (`crates/ui/src/theme/mod.rs:98`):
`impl Global for Theme {}`. **There is no theme prop, no theme parameter, no
theme builder method on `Table`.** Theming flows entirely through the gpui
`Global` map; T7 just constructs `Table::new(&state)` and the widget pulls
colours via `cx.theme()`.

`gpui_component::init(cx)` (`crates/ui/src/theme/mod.rs:21-26`) seeds the
default `Theme` global on app init. Without this call no `Theme` global
exists, and the first `cx.theme()` panics in `App::global` (see
gpui-api-notes §0.A).

### 1.4 Cell render entry point

`fn render_td(...) -> impl IntoElement` (no default; see P3a notes §4.4 for the
row-major call pattern). dat0's existing `crates/dat0-app/src/data_grid/renderers.rs`
mounts here; T7 will wire the existing `GridDataSource::render_td` to a
`Table::new(&state).stripe(true).bordered(true)` in `WorkspaceShell::render`.

---

## 2. Drawer / Modal / Sheet (T11 — Import wizard drawer)

The plan asks: "Does a Drawer-style primitive exist?" Answer: **yes — `Sheet`
is the gpui-component name for what dat0 calls a drawer.** No separate `Drawer`
type. `Dialog` exists for true modals. Both are documented verbatim in
`gpui-api-notes.md` §0.5b (already on disk from P1.T0); this section re-cites
the bits T11 will actually use.

### 2.1 Sheet — slide-in panel from a window edge (drawer-equivalent)

Source: `crates/ui/src/sheet.rs`. Verbatim public API (file:line refs from
`gpui-api-notes.md` §0.5b, re-verified 2026-05-25):

```rust
// sheet.rs:28 — struct
#[derive(IntoElement)]
pub struct Sheet { ... }

// sheet.rs:44
pub fn new(_: &mut Window, cx: &mut App) -> Self
// sheet.rs:61
pub fn title(mut self, title: impl IntoElement) -> Self
// sheet.rs:67
pub fn footer(mut self, footer: impl IntoElement) -> Self
// sheet.rs:72
pub fn size(mut self, size: impl Into<DefiniteLength>) -> Self
// sheet.rs:80
pub fn margin_top(mut self, top: Pixels) -> Self
// sheet.rs:86
pub fn resizable(mut self, resizable: bool) -> Self
// sheet.rs:92
pub fn overlay(mut self, overlay: bool) -> Self
// sheet.rs:98
pub fn overlay_closable(mut self, overlay_closable: bool) -> Self
// sheet.rs:104
pub fn on_close(mut self, on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self
```

### 2.2 Opening a Sheet — `WindowExt::open_sheet_at`

Source: `crates/ui/src/root.rs:32` (`WindowExt` trait). The fluent open
methods:

```rust
fn open_sheet<F>(&mut self, cx: &mut App, build: F)
where F: Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static;

fn open_sheet_at<F>(&mut self, placement: Placement, cx: &mut App, build: F)
where F: Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static;

fn has_active_sheet(&mut self, cx: &mut App) -> bool;
fn close_sheet(&mut self, cx: &mut App);
```

**For T11:** the spec §3.7 says "Drawer slides from the **top** of the
workspace shell." The placement enum at
`crates/ui/src/geometry.rs:10` is:

```rust
pub enum Placement { Top, Bottom, Left, Right }
```

So the T11 invocation is:

```rust
window.open_sheet_at(Placement::Top, cx, move |sheet, _w, _cx| {
    sheet.title("Import wizard")
        .size(px(360.))
        .resizable(false)
        .child( /* delimiter dropdown + preview */ )
});
```

The default placement for plain `open_sheet` is `Placement::Right` with size
`350px` (verified — `sheet.rs` `Default` impl); T11 must use `open_sheet_at`
to anchor to the top edge.

### 2.3 Required render-layer wiring

Without rendering the sheet layer, `open_sheet*` silently has no effect. Source:
`crates/ui/src/root.rs:278` — `Root::render_sheet_layer(window, cx)`. Wiring
pattern (from `examples/dialog_overlay/src/main.rs:76-77`):

```rust
// Inside the Root-wrapped view's render impl:
.children(Root::render_dialog_layer(window, cx))
.children(Root::render_sheet_layer(window, cx))
```

**Cross-reference:** T7 mounts the Table inside `WorkspaceShell`. The
`WorkspaceShell` is already wrapped in `Root` (P3a T1 wired this). T11 must
ensure `WorkspaceShell::render` (or its `Root`-wrapped parent) calls
`.children(Root::render_sheet_layer(window, cx))` before the import wizard
will draw. If it does not already, T11 is also responsible for adding the call.

### 2.4 Dialog (T8/T10 confirmation prompts — already documented)

`Dialog` is the canonical modal type. There is **no** `Modal` type. P3b uses
`Dialog` indirectly for any Sample-Data confirm prompts (T8) and the Discard
confirmation in Recovery (T10).

Full signature listing already in `gpui-api-notes.md` §0.5b — re-verified
2026-05-25, no drift. Key calls:

```rust
window.open_dialog(cx, move |dialog, _, _| {
    dialog.title("Discard orphan?").confirm()
        .on_ok(move |_, _, _| { /* delete */; true })
        .on_cancel(move |_, _, _| true)
        .child("This cannot be undone.")
});
```

### 2.5 Fallback (NOT needed for P3b)

Plan asked for the fallback if no drawer primitive exists. Since `Sheet`
exists, the fallback (hand-rolled overlay + `gpui::Animation`) is **not
needed**. No code task in P3b should implement a custom drawer.

---

## 3. Fuzzy list / Picker / Command palette source (T12)

Plan asks: "Does a fuzzy-list primitive exist?" Answer: **yes — `List` +
`ListDelegate` is the picker primitive, with built-in `perform_search` hook
and search input.** No separate `Picker` or `CommandPalette` type.
`Select` (combobox) also exists for the simpler dropdown case (e.g., T13
delimiter dropdown).

### 3.1 `List` + `ListDelegate` — the fuzzy-search primitive

Source: `crates/ui/src/list/delegate.rs`. Verbatim trait body:

```rust
// crates/ui/src/list/delegate.rs:10
#[allow(unused)]
pub trait ListDelegate: Sized + 'static {
    type Item: Selectable + IntoElement;

    /// When Query Input change, this method will be called.
    /// You can perform search here.
    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        Task::ready(())
    }

    fn sections_count(&self, cx: &App) -> usize { 1 }

    fn items_count(&self, section: usize, cx: &App) -> usize;

    /// Render the item at the given index.
    /// Return None will skip the item.
    /// NOTE: Every item should have same height.
    fn render_item(
        &mut self,
        ix: IndexPath,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item>;

    fn render_section_header(&mut self, section: usize, ...) -> Option<impl IntoElement> { None::<AnyElement> }
    fn render_section_footer(&mut self, section: usize, ...) -> Option<impl IntoElement> { None::<AnyElement> }
    fn render_empty(&mut self, ...) -> impl IntoElement { /* default Inbox icon */ }
    fn render_initial(&mut self, ...) -> Option<AnyElement> { None }
    fn loading(&self, _cx: &App) -> bool { false }
    fn render_loading(&mut self, ...) -> impl IntoElement { Loading }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    );

    fn confirm(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {}
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {}

    fn is_eof(&self, cx: &App) -> bool { true }
    fn load_more_threshold(&self) -> usize { 20 }
    fn load_more(&mut self, ...) {}
}
```

### 3.2 Required methods (no default body)

- `type Item: Selectable + IntoElement;`
- `fn items_count(&self, section: usize, cx: &App) -> usize`
- `fn render_item(&mut self, ix: IndexPath, ...) -> Option<Self::Item>`
- `fn set_selected_index(&mut self, ix: Option<IndexPath>, ...)`

`render_item` returns `Option<Self::Item>` (not `impl IntoElement` directly);
the associated type binds the item element type. **All items must have the
same height** per the doc comment.

### 3.3 Constructors — `ListState::new` + `List::new`

Source: `crates/ui/src/list/list.rs:95`:

```rust
pub fn new(delegate: D, window: &mut Window, cx: &mut Context<Self>) -> Self
```

`ListState::new` automatically constructs an internal `InputState` for the
search box (`crates/ui/src/list/list.rs:96-97`):

```rust
let query_input = cx.new(|cx| InputState::new(window, cx)
    .placeholder(t!("List.search_placeholder")));
```

Built-in subscription wires `perform_search` to input changes
(`list.rs:99-100`):

```rust
let _query_input_subscription =
    cx.subscribe_in(&query_input, window, Self::on_query_input_event);
```

Searchable mode must be opted into (default is OFF):

```rust
// crates/ui/src/list/list.rs:126-129
pub fn searchable(mut self, searchable: bool) -> Self {
    self.searchable = searchable;
    self
}
```

Render element:

```rust
// crates/ui/src/list/list.rs:670-688
pub struct List<D: ListDelegate + 'static> { ... }
impl<D: ListDelegate + 'static> List<D> {
    pub fn new(state: &Entity<ListState<D>>) -> Self { ... }
}
```

### 3.4 Item rendering — closure-free, trait-impl-based

`render_item` is a method on the delegate, **not** a closure passed at
construction. The item type is bound by the associated type
`type Item: Selectable + IntoElement`. dat0's `ActionDescriptor` already
implements `IntoElement` candidates via `gpui-component`'s `ListItem`
(`crates/ui/src/list/list_item.rs`); T12 wraps each action in a
`ListItem::new(...)` returned from `render_item`.

The `Selectable` trait (re-export via `gpui_component::Selectable`) is the
single-method "is this element currently selected?" trait used by the widget
to draw the highlight.

### 3.5 Keybindings — ↑/↓/Enter/Esc are wired automatically

`crates/ui/src/select.rs:23-34` and parallel registrations in `list/mod.rs` bind:

- `up` → `SelectUp`
- `down` → `SelectDown`
- `enter` → `Confirm { secondary: false }`
- `secondary-enter` → `Confirm { secondary: true }`
- `escape` → `Cancel`

These call `set_selected_index` / `confirm` / `cancel` on the delegate. **T12
does not implement key handling manually** — the widget owns it.

### 3.6 `Select` (combobox) — for T13 delimiter dropdown

Source: `crates/ui/src/select.rs`. For a simple dropdown (one-line, no search
input above the list) use `Select` instead of `List`. The `SelectItem` trait
(at `select.rs:37-65`) is a value-bearing variant with `title()`, `value()`,
and `matches(query)` methods. `SelectDelegate` (analogous to `ListDelegate`)
is the data-source trait.

`SelectState::new` — `crates/ui/src/select.rs:547`:

```rust
pub fn new(...) -> Self // exact arg list at the source
```

`Select::new` — `crates/ui/src/select.rs:924`:

```rust
pub fn new(state: &Entity<SelectState<D>>) -> Self
```

For T13 (delimiter dropdown with 4-5 options) `Select` is a better fit than
`List`; `List` is reserved for the command palette (T12) where the query input
is required.

### 3.7 Fallback (NOT needed for P3b)

Plan asked for the fallback (raw `TextInput` + scored `Vec<&ActionDescriptor>`
+ manual key handling). Since `List` + `ListDelegate` exists with
`perform_search` already wired, **no fallback is needed**. T12 uses `List`.

### 3.8 What `perform_search` does NOT do

`perform_search` is called when the query input changes, but the **scoring
function is up to the delegate**. gpui-component does not ship a fuzzy-match
crate or scorer. T12 must implement the score function inside `perform_search`
(e.g., simple substring `.to_lowercase().contains(&q.to_lowercase())` or pull
in `nucleo-matcher` / `sublime_fuzzy`). dat0's `ActionRegistry` (spec §3.3)
owns the action list; `perform_search` reads from it, ranks against `query`,
and stores the ranked indices for `render_item` to consume.

---

## 4. Caveats / drift risks

1. **Theme is global — there is no "theme prop" on any widget.** T7 (Table
   mount) and T13 (theme live-switch) both depend on the `Theme: Global`
   model. Any code path that mutates `Theme` via `cx.set_global` or
   `cx.update_global` triggers an automatic re-render of every widget that
   reads `cx.theme()` — this is what makes "live theme switch" cheap. See
   `gpui-api-notes.md` §0.A (appended by this spike) for the exact notification
   semantics.

2. **`gpui_component::init(cx)` must run before any widget is constructed.**
   Without it, `Theme::global(cx)` panics. dat0's `crates/dat0-app/src/window.rs`
   already calls this on the gpui app's `run` closure (P1.T0 verified — see
   `gpui-api-notes.md` §0.2 #1). T7/T11/T12 do NOT need to re-call it; they
   inherit the existing init.

3. **`Root::new(view, window, cx)` wrapper is mandatory for Sheet/Dialog
   overlays.** P3a T1 wired this for `WorkspaceShell`. T11 (Import wizard
   drawer / Sheet) and T8 (Sample-Data Dialog) inherit it; no re-wrapping
   required. **But:** the parent view's `render` must also call
   `.children(Root::render_sheet_layer(window, cx))` and
   `.children(Root::render_dialog_layer(window, cx))` at the end of its render
   tree. If P3a `WorkspaceShell::render` does not, T7/T11/T8 are also responsible
   for adding those calls — verify before adding the Sheet/Dialog code.

4. **`List` uses `IndexPath`, not `usize`.** dat0 currently keys actions by
   plain `usize` (or `ActionId`). The delegate's `render_item(ix: IndexPath, ...)`
   receives `IndexPath { section, row }`. For a single-section command palette
   (the P3b T12 case) `ix.row` is the effective row index; section is always 0.
   `IndexPath` is in `crates/ui/src/index_path.rs`.

5. **`List::Item: Selectable` is a required associated-type bound.** Implementers
   must either use `gpui_component::list::ListItem` (which already implements
   `Selectable`) or implement `Selectable` directly on a custom element type.
   `Selectable` is a single-method trait at `crates/ui/src/lib.rs` (re-exported
   from a sub-module); inspect it before designing a custom item element.

6. **`Sheet` is named differently from how dat0 talks about drawers.** Spec
   §3.7 uses "drawer"; gpui-component uses "Sheet". No drift, just a naming
   alias. T11 imports `gpui_component::Sheet` (re-exported via
   `crates/ui/src/lib.rs:89` — implicit `pub use sheet::*` would be present
   if `sheet.rs` re-exported; verify the exact re-export form in T11's code).
   Verified: `crates/ui/src/lib.rs` does not have an explicit `pub use sheet::*`
   line, so `Sheet` is referenced as `gpui_component::sheet::Sheet`.

7. **No `Modal` type.** Anywhere the spec or plan says "Modal", read "Dialog".
   No drift, just a naming alias.

8. **`perform_search` returns `Task<()>`.** It can be implemented async (e.g.,
   spawn an `Arc<RwLock<ActionRegistry>>` read). For the small (5–50) action
   set in P3b T12, a synchronous implementation that returns `Task::ready(())`
   after computing the ranking inline is fine.

---

## 5. Source pointers (for re-verification)

At pinned commit `0f0ab35233212f8f3277028995caf0c41e13ee6c`:

- `crates/ui/src/table/{delegate,mod,state,column}.rs` — Table primitives
- `crates/ui/src/list/{delegate,list,list_item}.rs` — List + searchable picker
- `crates/ui/src/select.rs` — Select (combobox)
- `crates/ui/src/sheet.rs` — Sheet (drawer)
- `crates/ui/src/dialog.rs` — Dialog (modal)
- `crates/ui/src/root.rs` — `Root::new`, `WindowExt`, `render_sheet_layer`,
  `render_dialog_layer`
- `crates/ui/src/theme/mod.rs` — `Theme`, `ActiveTheme`, `Theme::global`,
  `impl Global for Theme`
- `crates/ui/src/geometry.rs:10` — `Placement` enum

Re-verification protocol: when bumping `gpui-component` past `v0.5.1`, refetch
each file above and diff against the verbatim snippets in this doc + the P3a
`gpui-table-api-notes.md`. Update `docs/upstream-watch.md` "Current verified
pins" row.

---

## 6. P5a T0 spike (2026-06-02)

De-risk spike for the SQL Console. Method: a throwaway `examples/p5a_spike.rs`
(deleted at end of T0) that compiles as an **external** consumer of
`dat0-app`/`gpui-component` — the exact visibility boundary the shipped
`crates/dat0-app/src/...` code will face. Findings:

**(a) Runtime SQL highlight via `LanguageRegistry` — API compiles/links cleanly.**
The runtime-registration path is valid and ABI-correct:
`LanguageConfig::new("sql", tree_sitter_sequel::LANGUAGE.into(), vec![],
tree_sitter_sequel::HIGHLIGHTS_QUERY, "", "")` +
`LanguageRegistry::singleton().register("sql", &cfg)` +
`InputState::new(..).code_editor("sql")`.
`cargo build -p dat0-app --example p5a_spike` succeeded (clean compile + link).
This mirrors gpui-component's own reference exactly (`crates/story/examples/editor.rs:35`
registers `tree_sitter_navi::LANGUAGE.into()` / `HIGHLIGHTS_QUERY` the same way;
`crates/ui/src/highlighter/registry.rs:515` unit test uses
`tree_sitter_json::LANGUAGE.into()`). Public paths confirmed:
`gpui_component::highlighter::{LanguageConfig, LanguageRegistry}` and
`gpui_component::input::{Input, InputState}`.
**Visual color confirmation owed to manual UAT (headless agent cannot view GUI)** —
i.e. "are keywords/identifiers actually colored on screen" is a human-run check;
this spike only proves the API path compiles and links.
=> Design decision 7 (fallback to plain `code_editor`, highlight moved to P5b) is
NOT triggered. Highlight stays in P5a.

**(b) Selection accessor — NONE public; selection-override deferred, cursor-only run.**
Neither selection probe compiles from outside the crate:
- `selected_range` (field): `error[E0616]: field selected_range of struct InputState
  is private` — declared `pub(super) selected_range: Selection` at
  `crates/ui/src/input/state.rs:273`.
- `selected_text()` (method): `error[E0624]: method selected_text is private` —
  declared `pub(super) fn selected_text(&self) -> RopeSlice<'_>` at
  `crates/ui/src/input/state.rs:1837`.
The ONLY public state accessors are `cursor() -> usize` (state.rs:1498, the
control probe — compiled OK), `cursor_position() -> Position` (state.rs:802),
`text() -> &Rope` (state.rs:797), and `value() -> SharedString` (state.rs:787).
=> T6/T11 "run the selected text" is NOT achievable through public API at rev
`0f0ab35`. Selection-override is deferred; the run path is **cursor-based**
(use `cursor()` + `text()` to find the statement under the cursor). If true
selection-run is later required, options are: upstream a `pub` getter, or read
the full `text()` and run the whole buffer / statement-at-cursor.

**(c) `tree-sitter-sequel` version pinned.**
Requirement string in `crates/dat0-app/Cargo.toml`: `tree-sitter-sequel = "0.3.8"`
— byte-identical to gpui-component's own declaration (rev `0f0ab35`,
`crates/ui/Cargo.toml:123`: `tree-sitter-sequel = { version = "0.3.8", optional = true }`).
Both caret requirements unify to a **single** resolved copy in `Cargo.lock`:
`tree-sitter-sequel v0.3.11` (checksum `9d198ad3...`), riding the shared
`tree-sitter v0.25.10` core (gpui-component declares `tree-sitter = "0.25.4"`).
Single unified copy => `LANGUAGE`/`HIGHLIGHTS_QUERY` come from an ABI-identical
build, so the `tree_sitter::Language` handed to gpui-component's registry is the
one its highlighter expects. Transitive footprint is tiny (`tree-sitter-language`
runtime + `cc`/`jobserver`/`shlex`/`libc` build deps); no `arrow`, no bundle.

**(d) Do NOT enable gpui-component `tree-sitter-languages` (28-grammar bundle).**
gpui-component gates ~28 grammar deps behind its `tree-sitter-languages` feature
(`crates/ui/Cargo.toml:23`); enabling it has caused CI OOM/disk failures. dat0
deliberately stays off that feature and supplies the single SQL grammar directly
as a dat0-app dependency, registered at runtime. Under no circumstance enable the
bundle.
