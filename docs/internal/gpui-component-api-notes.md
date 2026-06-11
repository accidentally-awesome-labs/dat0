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

⚠️ The `.child(...)` body-text call requires `use gpui::ParentElement;` (or `as _`) in scope — without it this fails to compile with `E0599: no method named child`. See §7.2/§7.5(a) for the verified detail.

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

---

## 7. Dialog-from-App + window activation (P7b T0 spike)

- **Verification date:** 2026-06-11
- **Verifier:** P7b.T0 spike (read-only inspection of pinned source + a deleted
  throwaway external-consumer example).
- **gpui-component pinned commit:** `0f0ab35233212f8f3277028995caf0c41e13ee6c`
  (tag `v0.5.1`).
- **gpui version:** `=0.2.2` (crates.io); source at
  `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-0.2.2/`.

De-risk spike for P7b T6 (sync-drive conflict + same-machine modal) and T8
(bring-to-front). Three GPUI calls not yet exercised anywhere in dat0:
(1) reaching a `&mut Window` from an `&mut App` action context, (2) wiring a
two-button confirm `Dialog` with body text, (3) raising a specific window to the
foreground. The core unknown: dat0's call site `open_workspace_at(cx: &mut App,
folder: PathBuf)` (`crates/dat0-app/src/window.rs:105`) holds only `&mut App` —
no `Window` — yet `open_dialog` is a method on `Window`.

**Method (mirrors §6 P5a precedent):** a throwaway
`crates/dat0-app/examples/p7b_spike.rs` compiled as an **external** consumer of
`gpui` + `gpui-component` (the exact visibility boundary the shipped code faces).
`cargo build -p dat0-app --example p7b_spike` **succeeded (clean compile + link;
`target/debug/examples/p7b_spike` produced)** after one fix the proof caught
(see §7.5(a)). The example was then DELETED (not committed). A headless agent
cannot click buttons, so **runtime button-fire (`on_ok`/`on_cancel` actually
firing on click) is owed to manual UAT** — exactly as §6(a) recorded visual
colour confirmation as owed. This spike proves only that the API paths compile
and link.

### 7.1 STEP 1 — reaching `&mut Window` from `&mut App` (PRIMARY path confirmed)

`App` exposes `active_window`, and `AnyWindowHandle::update` yields a `&mut
Window` inside its closure. Verbatim signatures:

`App::active_window` — gpui `src/app.rs:936`:

```rust
/// Returns a handle to the window that is currently focused at the platform level, if one exists.
pub fn active_window(&self) -> Option<AnyWindowHandle> {
    self.platform.active_window()
}
```

`AnyWindowHandle::update` — gpui `src/window.rs:4818`:

```rust
/// Updates the state of the root view of this window.
/// This will fail if the window has been closed.
pub fn update<C, R>(
    self,
    cx: &mut C,
    update: impl FnOnce(AnyView, &mut Window, &mut App) -> R,
) -> Result<R>
where
    C: AppContext,
```

**Exact param/return facts (do NOT trust the target-shape guess; these are the
confirmed forms):**

- The first closure param is **`AnyView`** (the type-erased root view) — NOT a
  typed `_root_view`. dat0 does not need it, so bind it `_root: AnyView` (or
  `_`).
- `update` returns **`Result<R>`** (the window may have closed). In an action
  path that cannot propagate, swallow it with `let _ = handle.update(...)`. (Use
  `?` only where the caller returns `Result`.)
- The `C: AppContext` bound is satisfied by `App` — `impl AppContext for App` at
  gpui `src/app.rs:2106` (`update_window` impl at `src/app.rs:2183`). So passing
  `cx: &mut App` straight through compiles.

### 7.2 STEP 2 — two-button confirm Dialog with body text

The trait carrying `open_dialog` is **`WindowExt`** (re-exported as
`gpui_component::WindowExt`, `crates/ui/src/lib.rs:85`), implemented for
`Window`. NOTE: it is `WindowExt`, not `ContextModal` as the task brief
guessed. Signature — `crates/ui/src/root.rs:45`:

```rust
/// Opens a Dialog.
fn open_dialog<F>(&mut self, cx: &mut App, build: F)
where
    F: Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static;
```

The build closure receives `(Dialog, &mut Window, &mut App)` and **returns the
built `Dialog`** (fluent style). `close_dialog(&mut self, cx: &mut App)` is the
matching close (`root.rs:53`); the confirm/cancel buttons auto-close (see
return-`bool` below), so T6 rarely needs `close_dialog` directly.

`Dialog` builder (all on the value returned through the closure):

- `.title(impl IntoElement)` — `crates/ui/src/dialog.rs:138`.
- `.confirm()` — OK+Cancel footer; also sets `overlay_closable(false)` +
  `close_button(false)` — `dialog.rs:168`. `.alert()` is OK-only — `dialog.rs:177`.
- `.button_props(DialogButtonProps::default().ok_text(..).cancel_text(..))` —
  `dialog.rs:184`; `DialogButtonProps` at `dialog.rs:34` (reach it as
  `gpui_component::dialog::DialogButtonProps`); `ok_text`/`cancel_text` at
  `dialog.rs:54`/`:66`. Default OK text is `OK`, cancel is `Cancel`.
- **Body text is set via `ParentElement::child(impl IntoElement)`** — `Dialog`
  implements `ParentElement` (`dialog.rs:279-283`, forwarding to an inner
  `content: Div`). Confirmed in the upstream example
  `crates/story/src/dialog_story.rs:274`: `.child("Are you sure to submit?")`.
  There is **no** `.content(...)` / `.body(...)` method — it is `.child(...)`.
  ⚠️ **The proof caught this:** calling `.child(...)` requires the
  `gpui::ParentElement` trait to be **in scope** (`use gpui::ParentElement as
  _;`), else `E0599: no method named child`. The shipped T6 module must import
  it. (Most dat0 render modules already do; the conflict-dialog helper must too.)
- `.on_ok(impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static)` —
  `dialog.rs:203`. **Returns `bool`: `true` = close the dialog after running;
  `false` = keep it open.** For T6 the "Open anyway" branch runs the follow-up
  (`open_workspace_proceed(...)`) and returns `true`.
- `.on_cancel(impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static)` —
  `dialog.rs:214`. Same `bool` close-semantics; the abort branch returns `true`.
- A `move` closure that runs a follow-up action captures fine: the closures are
  `Fn` (stored as `Rc<dyn Fn(...) -> bool>` — `dialog.rs:90-91`), so any captured
  data must be `Clone`/copy-per-call-safe (in the spike `title`/`body` `String`s
  are `.clone()`d into the build closure, which is itself `Fn`).

Upstream reference call (`crates/story/src/dialog_story.rs:269-289`, verbatim
shape): `window.open_dialog(cx, move |dialog, _, _| { dialog.confirm()
.child("Are you sure to submit?").on_ok(|_, window, cx| { …; true })
.on_cancel(|_, window, cx| { …; true }) });`.

### 7.3 STEP 3 — bring a window to the foreground (PRIMARY path; per-window raise EXISTS)

A real per-window raise exists; no fallback needed for T8.

`Window::activate_window` — gpui `src/window.rs:4112`:

```rust
/// Focus the current window and bring it to the foreground at the platform level.
pub fn activate_window(&self) {
    self.platform_window.activate();
}
```

`App::activate` (app-level foreground, already used by dat0 at
`crates/dat0-app/src/window.rs:840` — `cx.activate(true)`) — gpui `src/app.rs:979`:

```rust
/// Instructs the platform to activate the application by bringing it to the foreground.
pub fn activate(&self, ignoring_other_apps: bool) {
    self.platform.activate(ignoring_other_apps);
}
```

**T8 ([Focus existing] → raise the already-open same-machine window) uses
BOTH:** reach the target window via `handle.update(...)` and call
`window.activate_window()` (raises that specific window), plus `cx.activate(true)`
(brings the dat0 app to the foreground at the OS level). The fallback the task
brief described ("no per-window API → re-run `cx.activate(true)` + log") is
**NOT** triggered: per-window activation is available at this pin.

⚠️ **Limitation for T8:** `cx.active_window()` returns the window *currently
focused at the platform level*, which is not necessarily the workspace window T8
wants to raise. T8 must obtain the **correct** target handle — dat0 already
tracks open workspace windows in `window_registry` (see
`open_workspace_at` → `reg.lock().find_by_workspace(&folder)` at
`crates/dat0-app/src/window.rs:121-126`). T8 should store the per-window
`AnyWindowHandle` (or `WindowHandle<Root>`, downcastable via
`AnyWindowHandle::downcast`, gpui `src/window.rs:4804`) in that registry at
window creation and call `.update(cx, |_, window, _| window.activate_window())`
on the **registry-resolved** handle — NOT blindly on `active_window()`. The
spike only proves the *call* compiles from `active_window()`; choosing the right
handle is T8 design, not an API gap.

### 7.4 Confirmed compiling pattern (PRIMARY path — copy into T6/T8)

This is the exact form the throwaway example compiled+linked as an external
consumer (imports included — the `ParentElement` import is load-bearing per
§7.2):

```rust
use gpui::{App, AnyView, ParentElement as _, Window};
use gpui_component::WindowExt as _;
use gpui_component::dialog::{Dialog, DialogButtonProps};

// T6: open_workspace_at(cx: &mut App, …) reaches a &mut Window and opens a
// real two-button confirm Dialog with a body string.
fn open_conflict_dialog(cx: &mut App, title: String, body: String) {
    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, move |_root: AnyView, window: &mut Window, cx| {
            window.open_dialog(cx, move |dialog: Dialog, _w, _cx| {
                dialog
                    .title(title.clone())
                    .confirm()
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text("Open anyway")
                            .cancel_text("Cancel"),
                    )
                    .child(body.clone()) // body text — needs ParentElement in scope
                    .on_ok(move |_ev, _window, _cx| {
                        // T6: call open_workspace_proceed(...) here; return true to close
                        true
                    })
                    .on_cancel(move |_ev, _window, _cx| {
                        true // abort; true closes the dialog
                    })
            });
        });
    }
}

// T8: raise the already-open same-machine window + app to the foreground.
fn focus_existing_window(cx: &mut App) {
    if let Some(handle) = cx.active_window() { // T8: prefer the registry-resolved handle
        let _ = handle.update(cx, |_root: AnyView, window: &mut Window, _cx| {
            window.activate_window();
        });
    }
    cx.activate(true);
}
```

### 7.5 Spike findings / corrections

**(a) The compile proof caught a real defect.** First build failed with
`E0599: no method named child found for struct Dialog` — body text via
`.child(...)` requires `use gpui::ParentElement;` in scope. A prose-only snippet
would have shipped that bug into T6. Fixed by adding `ParentElement as _` to the
imports; rebuild clean. (This is precisely the class of private-visibility /
missing-trait problem the external-consumer example exists to surface.)

**(b) Trait name correction.** The dispatching trait is **`WindowExt`**
(`gpui_component::WindowExt`, `root.rs:27`/`:45`), not `ContextModal` as the task
brief's "verified facts" guessed. `open_dialog`/`close_dialog` live on it.

**(c) Closure-param/return-type corrections vs the target-shape guess.**
`handle.update`'s first closure param is `AnyView` (not a typed root view) and
the call returns `Result<R>` (needs `let _ =`/`?`). The `open_dialog` build
closure is `Fn(Dialog, &mut Window, &mut App) -> Dialog` (returns the Dialog,
fluent), and `on_ok`/`on_cancel` are `Fn(&ClickEvent, &mut Window, &mut App) ->
bool` (true = close).

**(d) Runtime click owed to UAT.** Headless build cannot fire `on_ok`/`on_cancel`
on a real click; that behavioural confirmation is deferred to manual UAT, same
as §6(a)'s colour check.

### 7.6 DECISION GATE

**CHOSEN: PRIMARY PATH — real gpui-component `Dialog` from the `&mut App` action
context.** Decisive evidence: the external-consumer example
`cargo build -p dat0-app --example p7b_spike` **compiled and linked** using only
public APIs — `cx.active_window()` (gpui `app.rs:936`) → `handle.update(cx, |_:
AnyView, window: &mut Window, cx| …)` (gpui `window.rs:4818`,
`impl AppContext for App` at `app.rs:2106`) reaches a `&mut Window`, on which
`WindowExt::open_dialog` (gpui-component `root.rs:45`) opens a `Dialog` with
`.title`/`.confirm`/`.button_props`/`.child`/`.on_ok`/`.on_cancel`
(`dialog.rs:138/168/184/279/203/214`). Per-window raise
`Window::activate_window()` (gpui `window.rs:4112`) is likewise reachable.

Therefore:

- **T6** `open_conflict_dialog` / `open_same_machine_dialog` use the real
  `Dialog` via the §7.4 pattern. Remember `use gpui::ParentElement;` for body
  text. Note: the plan's T6 Step 2 scaffold imports `gpui_component::ContextModal as _` — that trait name is wrong (see §7.5(b)); use `gpui_component::WindowExt as _` instead.
- **T8** uses `Window::activate_window()` on the **registry-resolved** window
  handle (§7.3 limitation) plus `cx.activate(true)`.

**FALLBACK PATH (NOT taken):** an in-shell modal overlay — a
`WorkspaceShell`-rendered blocking layer keyed on a `pending_conflict:
Option<…>` field. This would only have been needed if `open_dialog` were
unreachable from the action context. Since the PRIMARY path compiles+links,
downstream T6/T7 do **not** need the in-shell overlay. Record kept only so a
future gpui-component bump that breaks the PRIMARY path has a documented Plan B.

**Owed to manual UAT (cannot be verified headless):** that clicking OK actually
fires `on_ok` (→ proceeds) and Cancel fires `on_cancel` (→ aborts), and that
`activate_window()` visibly raises the right window on macOS.
