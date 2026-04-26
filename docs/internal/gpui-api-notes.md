# GPUI API Verification Notes (P1.T0 spike)

This document is the canonical reference for the GPUI / gpui-component API surface used by P1. Subsequent tasks (T2 GPUI window, T14 macOS menu, T16 Settings panel, T17 Error/dialog primitives) MUST defer to this file when plan snippets contradict the actual API.

- **Verification date:** 2026-04-26
- **Verifier:** P1.T0 spike (read-only inspection of GitHub source at pinned SHAs)

---

## 0.1 Pinned commits

| Component | Tag | Date pinned | SHA (full 40-char) | Source / pin form |
|---|---|---|---|---|
| `gpui-component` (longbridge) | `v0.5.1` | 2026-02-05 | `0f0ab35233212f8f3277028995caf0c41e13ee6c` | git tag in `longbridge/gpui-component` |
| `gpui` (Zed) | `v0.2.2` (crates.io) | 2025-10-22 | `08d95ad9d31f616a43dacda8416568d658dca6ae` | crates.io publish commit in `zed-industries/zed`; commit message: "chore: Bump gpui to 0.2.2 (#40856)" |
| `gpui-macros` (Zed) | `v0.2.2` (crates.io) | 2025-10-22 | (same as above) | crates.io |

### Plan-contradicting finding (important)

The P1 plan and the upstream-watch table both implied `gpui-component` pins `gpui` to a **Zed git SHA**. **This is no longer true.** As of `gpui-component` v0.5.1, the workspace `Cargo.toml` declares:

```toml
[workspace.dependencies]
gpui = "0.2.2"
gpui-macros = "0.2.2"
```

i.e., it consumes `gpui` as a published crate from **crates.io**, not as a git dependency of `zed-industries/zed`. The crate is published from `zed-industries/zed` (per `Cargo.toml`'s `repository = "https://github.com/zed-industries/zed"`), and the publish commit is `08d95ad9d31f616a43dacda8416568d658dca6ae`. So the pin policy still works, but the mechanism changed: dat0 will pin via `Cargo.toml` semver (`gpui = "=0.2.2"`) plus a git SHA recorded in `Cargo.lock`/this doc, NOT via `[patch.crates-io]` or git dependency.

This is a **good** development for dat0 — published crates are more stable to depend on than monorepo-internal git tracking. Tasks that referenced "the gpui SHA pinned by gpui-component" should now read "the gpui crates.io version pinned by gpui-component" plus the publish-commit SHA recorded here for auditability.

> Note: gpui-component v0.5.1 was released specifically to fix macOS `core-text` compilation errors that affected v0.5.0 (per the release notes). v0.5.1 is the correct floor for dat0 on macOS.

---

## 0.2 Window-open API surface (gpui v0.2.2 @ 08d95ad9)

All types are re-exported at the crate root: `gpui::Application`, `gpui::App`, `gpui::WindowOptions`, `gpui::WindowBounds`, `gpui::Bounds`, `gpui::TitlebarOptions`, `gpui::Render`, `gpui::Window`, `gpui::Context`, `gpui::IntoElement`, `gpui::div`. Source: `crates/gpui/src/gpui.rs` re-exports `app::*`, `element::*`, `geometry::*`, `platform::*`, `view::*`, `window::*`.

### `Application::run` — full signature (`crates/gpui/src/app.rs:174`)

```rust
pub fn run<F>(self, on_finish_launching: F)
where
    F: 'static + FnOnce(&mut App),
{ ... }
```

The closure takes `&mut App`, NOT `&mut AppContext` and NOT `&mut cx`. Bind it as `|cx: &mut App|`.

### `App::open_window` — full signature (`crates/gpui/src/app.rs:943`)

```rust
pub fn open_window<V: 'static + Render>(
    &mut self,
    options: crate::WindowOptions,
    build_root_view: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> anyhow::Result<WindowHandle<V>>
```

- The build closure receives `(&mut Window, &mut App)` — note: `&mut App` (the app), not `&mut Context<V>`. To create the entity, call `cx.new(|_| MyView { .. })` which yields `Entity<V>`.
- Returns `anyhow::Result<WindowHandle<V>>`. Existing examples `.unwrap()` it.

### `Render` trait — full signature (`crates/gpui/src/element.rs:131`)

```rust
pub trait Render: 'static + Sized {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
}
```

Confirmed: signature matches what the plan assumed: `(&mut self, &mut Window, &mut Context<Self>) -> impl IntoElement`. No drift.

### Other types

- **`WindowOptions`** (`crates/gpui/src/platform.rs:1089`): public fields include `window_bounds: Option<WindowBounds>`, `titlebar: Option<TitlebarOptions>`, `focus: bool`, `show: bool`, `kind: WindowKind`, `is_movable: bool`, `is_resizable: bool`, `is_minimizable: bool`, `display_id: Option<DisplayId>`, `window_background: WindowBackgroundAppearance`, `app_id: Option<String>`, `window_min_size: Option<Size<Pixels>>`, `window_decorations: Option<WindowDecorations>`, `tabbing_identifier: Option<String>`. Has `Default` impl.
- **`WindowBounds`** (`platform.rs:1188`): `enum { Windowed(Bounds<Pixels>), Maximized(Bounds<Pixels>), Fullscreen(Bounds<Pixels>) }`. Has `WindowBounds::centered(size: Size<Pixels>, cx: &App)` constructor.
- **`Bounds<Pixels>`** (`crates/gpui/src/geometry.rs:754`): `struct { origin: Point<T>, size: Size<T> }`. Has `Bounds::centered(display_id: Option<DisplayId>, size: Size<Pixels>, cx: &App)`.
- **`TitlebarOptions`** (`platform.rs:1248`): `struct { title: Option<SharedString>, appears_transparent: bool, traffic_light_position: Option<Point<Pixels>> }`. Has `Default`.

### Canonical `hello_world.rs` open-window pattern (verbatim from `crates/gpui/examples/hello_world.rs`)

```rust
use gpui::{
    App, Application, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};

struct HelloWorld { text: SharedString }

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().child(format!("Hello, {}!", &self.text))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(500.), px(500.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| HelloWorld { text: "World".into() })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
```

`use gpui::prelude::*;` is required to bring `Styled`, `IntoElement`, `ParentElement`, `FluentBuilder`, etc. methods (`flex`, `flex_col`, `child`, `bg`, `rgb`, `text_color`, `size_full`, `gap_3`, etc.) into scope.

`cx.activate(true)` brings the application to the foreground (especially relevant on macOS so the window isn't hidden behind other apps at launch).

### gpui-component-flavored open-window pattern (verbatim from `examples/hello_world/src/main.rs`)

```rust
use gpui::*;
use gpui_component::{button::*, *};

pub struct Example;
impl Render for Example {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().v_flex().gap_2().size_full().items_center().justify_center()
            .child("Hello, World!")
            .child(Button::new("ok").primary().label("Let's Go!").on_click(|_, _, _| println!("Clicked!")))
    }
}

fn main() {
    let app = Application::new();
    app.run(move |cx| {
        // This must be called before using any GPUI Component features.
        gpui_component::init(cx);

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|_| Example);
                // This first level on the window, should be a Root.
                cx.new(|cx| Root::new(view, window, cx))
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
```

**Key gpui-component requirements** (these are NOT optional for dat0's window):

1. Call `gpui_component::init(cx)` once inside the `Application::run` closure before opening any window — this initialises the theme, global state, and (in debug builds) the inspector.
2. The window's root view must be wrapped in `gpui_component::Root::new(view, window, cx)`. `Root` provides the overlay layer used by `Dialog`, `Sheet`, notifications, etc. Without it, dialogs cannot render.
3. The example uses `cx.spawn(async move |cx| { ... }).detach()` rather than calling `open_window` directly inline. This is a pattern preference, not a requirement — direct `cx.open_window(...)` on the synchronous path also works (the gpui-internal `hello_world.rs` does it directly).

For a custom title bar (used later in T2 / T14 styling), set `titlebar: Some(TitleBar::title_bar_options())` on `WindowOptions` and render `TitleBar::new()` as the first child of the root view; see `examples/window_title/src/main.rs`.

For asset bundling: `Application::new().with_assets(gpui_component_assets::Assets)` — signature is `pub fn with_assets(self, asset_source: impl AssetSource) -> Self` (`crates/gpui/src/app.rs:155`).

---

## 0.3 Menu API (gpui v0.2.2 @ 08d95ad9)

Source: `crates/gpui/src/platform/app_menu.rs`.

### `Menu` struct (`app_menu.rs:5`)

```rust
pub struct Menu {
    pub name: SharedString,
    pub items: Vec<MenuItem>,
}
```

Confirmed: matches the plan's assumption.

### `MenuItem` enum (`app_menu.rs:52`)

```rust
pub enum MenuItem {
    Separator,
    Submenu(Menu),
    SystemMenu(OsMenu),
    Action {
        name: SharedString,
        action: Box<dyn Action>,
        os_action: Option<OsAction>,
    },
}
```

### `MenuItem::action` signature (`app_menu.rs:96`)

```rust
pub fn action(name: impl Into<SharedString>, action: impl Action) -> Self
```

Note: `action: impl Action` (the trait), not `&dyn Action`; the action is taken by value and boxed internally.

Other constructors: `MenuItem::separator()`, `MenuItem::submenu(Menu)`, `MenuItem::os_submenu(name: impl Into<SharedString>, menu_type: SystemMenuType)`, `MenuItem::os_action(name, action, os_action: OsAction)`.

### `actions!` macro (`crates/gpui/src/action.rs:24`)

```rust
#[macro_export]
macro_rules! actions {
    ($namespace:path, [ $( $(#[$attr:meta])* $name:ident),* $(,)? ]) => {
        $(
            #[derive(::std::clone::Clone, ::std::cmp::PartialEq, ::std::default::Default, ::std::fmt::Debug, gpui::Action)]
            #[action(namespace = $namespace)]
            $(#[$attr])*
            pub struct $name;
        )*
    };
    ([ $( $(#[$attr:meta])* $name:ident),* $(,)? ]) => { ... };
}
```

Invocation: `actions!(set_menus, [Quit]);` generates a unit struct `pub struct Quit;` that derives `Clone + PartialEq + Default + Debug + gpui::Action`, registered under the namespace `set_menus`. Namespace argument may be omitted when not in Zed proper. For complex (non-unit) actions, use `#[derive(Action)]` directly.

### `set_menus` placement (`crates/gpui/src/app.rs:1840`)

```rust
pub fn set_menus(&self, menus: Vec<Menu>) {
    self.platform.set_menus(menus, &self.keymap.borrow());
}
```

It is a method on `App`, called inside the `Application::run` closure. There is **no** `cx.activate_menu(...)` — that name from the plan is incorrect. The verified pattern is:

```rust
Application::new().run(|cx: &mut App| {
    cx.activate(true);
    cx.on_action(quit);
    cx.set_menus(vec![Menu {
        name: "set_menus".into(),
        items: vec![
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Quit", Quit),
        ],
    }]);
    cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| SetMenus {})).unwrap();
});
actions!(set_menus, [Quit]);
fn quit(_: &Quit, cx: &mut App) { cx.quit(); }
```

(Verbatim from `crates/gpui/examples/set_menus.rs`.)

### `App::on_action` (`app.rs:1696`)

```rust
pub fn on_action<A: Action>(&mut self, listener: impl Fn(&A, &mut Self) + 'static)
```

Action handlers registered globally take `(&Action, &mut App)`. (Inside a `Render` impl, you'd use `cx.listener(...)` on an `Entity<Self>` instead.)

---

## 0.4 gpui-component Tailwind helpers

The Tailwind-flavored layout helpers split across two traits:

### `gpui::Styled` — core layout & sizing (Zed crate, `crates/gpui/src/styled.rs:21`)

Provides: `flex()`, `flex_row()`, `flex_col()`, `flex_1()`, `w_64()`, `h_full()`, `size_full()`, `size_8()`, `gap_2()`, `gap_3()`, `gap_4()`, `bg(...)`, `text_color(...)`, `text_xl()`, `border_1()`, `border_color(...)`, `rounded_md()`, `shadow_lg()`, `items_center()`, `justify_center()`, `p_5()`, `pt(...)`, `pb(...)`, `pl(...)`, `pr(...)`, etc. — the Tailwind-style methods that the plan attributes to "gpui-component" actually live in **gpui itself**.

### `gpui_component::StyledExt` — extension helpers (`crates/ui/src/styled.rs:58`)

```rust
pub trait StyledExt: Styled + Sized {
    fn h_flex(self) -> Self { self.flex().flex_row().items_center() }
    fn v_flex(self) -> Self { self.flex().flex_col() }
    fn refine_style(self, style: &StyleRefinement) -> Self;
    fn paddings<L>(self, paddings: impl Into<Edges<L>>) -> Self where L: ...;
    fn margins<L>(self, margins: impl Into<Edges<L>>) -> Self where L: ...;
    // plus: font_weight shortcuts, text helpers, etc.
}
```

Plus free functions `gpui_component::h_flex()` and `gpui_component::v_flex()` that return a pre-configured `Div`.

### Required `use` statements

Minimal:

```rust
use gpui::{prelude::*, div, px, rgb, size};   // Styled, ParentElement, IntoElement, FluentBuilder
use gpui_component::{StyledExt as _, h_flex, v_flex, Root, TitleBar};
```

In practice the gpui-component examples use the glob form:

```rust
use gpui::*;
use gpui_component::{button::*, *};
```

`gpui_component::*` re-exports `StyledExt`, `Root`, `TitleBar`, `WindowExt`, `h_flex`, `v_flex`, `window_border`, theming, etc. via `pub use styled::*; pub use theme::*; pub use title_bar::*;` (see `crates/ui/src/lib.rs`).

### `child(...)` accepts strings directly

`ParentElement::child` is defined as `fn child(mut self, child: impl IntoElement) -> Self` (`crates/gpui/src/element.rs:161`). The standard `IntoElement` impls in `crates/gpui/src/elements/text.rs` cover:

- `impl IntoElement for &'static str`
- `impl IntoElement for String`
- `impl IntoElement for SharedString`

So `.child("Hello")`, `.child(format!("Hello, {}!", name))`, and `.child(my_shared_string)` all compile **without** wrapping in `SharedString::from(...)`. (The `SharedString::from(...)` wrapping is only required for fields typed `SharedString` — e.g. `Menu { name: ..., }` — not for `child(...)`.)

### `when` / `when_some` (`crates/gpui/src/util.rs:13`)

```rust
pub trait FluentBuilder {
    fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self where Self: Sized;
    fn when_some<T>(self, option: Option<T>, then: impl FnOnce(Self, T) -> Self) -> Self where Self: Sized;
    // also: when_else, when_some_else, map
}
```

Blanket-impl `impl<T: IntoElement> FluentBuilder for T {}` (`element.rs:127`) — every element automatically has `when` / `when_some`. Brought in by `use gpui::prelude::*;`.

---

## 0.5 Settings-panel composition

dat0 has TWO viable references:

### Reference A — gpui-component's first-party `setting` module (`crates/ui/src/setting/`)

This is the **preferred** reference for T16. v0.5.1 ships a complete settings primitive set:

```rust
// crates/ui/src/setting/mod.rs
pub use fields::*;     // input field types (text, toggle, select, ...)
pub use group::*;      // SettingGroup
pub use item::*;       // SettingItem
pub use page::*;       // SettingPage
pub use settings::*;   // top-level Settings container
```

`SettingPage` API (`crates/ui/src/setting/page.rs`):

```rust
pub struct SettingPage { ... }
impl SettingPage {
    pub fn new(title: impl Into<SharedString>) -> Self;
    pub fn title(self, title: impl Into<SharedString>) -> Self;
    pub fn description(self, description: impl Into<SharedString>) -> Self;
    pub fn default_open(self, default_open: bool) -> Self;
    pub fn resettable(self, resettable: bool) -> Self;
    pub fn group(self, group: SettingGroup) -> Self;
    pub fn groups(self, groups: impl IntoIterator<Item = SettingGroup>) -> Self;
    // Render impl on line 93: -> impl IntoElement
}
```

This means T16 can compose: `Settings → SettingPage → SettingGroup → SettingItem → field` directly, instead of hand-rolling sidebar+content layout.

### Reference B — Zed's own `settings_ui` crate (`zed/crates/settings_ui/src/settings_ui.rs`)

3,484 LOC. The top-level `SettingsWindow` view (`Render` impl at line 2754) shows the canonical sidebar+content composition pattern. Boiled down:

```rust
impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        client_side_decorations(
            v_flex()
                .text_color(cx.theme().colors().text)
                .size_full()
                .children(self.title_bar.clone())
                .child(
                    div()
                        .id("settings-window")
                        .key_context("SettingsWindow")
                        .track_focus(&self.focus_handle)
                        // ... many on_action handlers
                        .flex().flex_row().flex_1().min_h_0()
                        .font(ui_font)
                        .bg(cx.theme().colors().background)
                        .text_color(cx.theme().colors().text)
                        .child(self.render_nav(window, cx))     // sidebar
                        .child(self.render_page(window, cx)),   // content
                ),
            window,
            cx,
        )
    }
}
```

Pattern summary: **outer `v_flex` (title bar + body), inner body is `div().flex().flex_row().flex_1().min_h_0()` with two children: nav (sidebar) on the left, page (content) on the right**. Use `.min_h_0()` on the row container so the inner panes can scroll independently.

For T16, the gpui-component `setting` module is a higher-level abstraction; Zed's pattern is the lower-level reference for the outer chrome/sidebar layout if dat0's settings UI grows beyond what `SettingPage` natively supports. dat0 should start with gpui-component's `setting` module and fall back to a hand-rolled `Sidebar` + content split (using `gpui_component::sidebar::Sidebar`, `crates/ui/src/sidebar/mod.rs:29`) only if needed.

`Sidebar` from gpui-component: `pub struct Sidebar<E: Collapsible + IntoElement + 'static>`, constructed via `Sidebar::new(side: Side)` / `Sidebar::left()` / `Sidebar::right()`, with `.header(...)`, `.footer(...)`, `.child(E)`, `.children(...)`, `.collapsible(bool)`, `.collapsed(bool)`. Width constants: `COLLAPSED_WIDTH = px(48.)`.

---

## 0.6 Gotchas / surprises

1. **gpui is now a published crates.io crate.** As of v0.2.0 (2025-10-09; PR #39835) and v0.2.1 (2025-10-14, PR #40158), `gpui` is published. Prior planning that assumed git-only consumption is stale. The pin model is now: `gpui = "=0.2.2"` in `Cargo.toml`, plus this doc records the publish-commit SHA for traceability.

2. **gpui-component requires `gpui_component::init(cx)` and a `Root::new(...)` wrapper.** Skipping either silently breaks dialogs, theming, and the inspector. T2 must implement both, not just call `cx.open_window(WindowOptions::default(), ...)` on a plain `Render`er.

3. **Tailwind helpers live in `gpui::Styled`, not gpui-component.** Every `flex`, `flex_col`, `gap_3`, `bg`, `rounded_md`, `border_1`, etc. method is on `gpui::Styled`. gpui-component only adds `StyledExt` extras (`h_flex`, `v_flex`, `paddings`, `margins`). Plan task snippets that say "import gpui-component for Tailwind-flavor helpers" should be read as "import `gpui::prelude::*;` for the helpers, plus `gpui_component::{h_flex, v_flex, StyledExt}` for the extras."

4. **No `cx.activate_menu(...)`.** The plan referenced this name; it does not exist. The actual method is `App::set_menus(&self, menus: Vec<Menu>)`. Call it inside the `Application::run` closure (i.e., on `&mut App`).

5. **`open_window` build closure receives `&mut App`, not `&mut Context<V>`.** Inside the closure use `cx.new(|_| MyView { .. })` to construct the entity. The plan's snippet `cx.new_view(|cx| ...)` is the older Zed-internal name; the published crates.io API uses `cx.new(|_| ...)`.

6. **`.child("text")` works directly.** No `SharedString::from(...)` required for the `child()` arg. (It IS required for `Menu { name: SharedString, ... }`-shaped public fields — `name: "Foo".into()` is fine.)

7. **`Bounds::centered` takes `(Option<DisplayId>, Size<Pixels>, &App)`.** The convenience `WindowBounds::centered(size, cx)` skips the display arg. Both exist.

8. **Action derive macro requires several traits.** The `actions!` macro auto-derives `Clone + PartialEq + Default + Debug + gpui::Action`. Hand-rolled action structs (e.g., struct fields) need at minimum `Clone + PartialEq + serde::Deserialize + schemars::JsonSchema + Action` (or use `#[action(no_json)]` to skip the JSON-derive ceremony). Source: `crates/gpui/src/action.rs:42-90`.

9. **gpui-component v0.5.1 specifically fixes a macOS `core-text` build failure** present in v0.5.0. Use v0.5.1 or later as the floor on macOS (per upstream release notes, 2026-02-05).

10. **Window initial draw quirk on Windows.** `App::open_window` performs an initial `window.draw(cx)` and discards the cleared layer to avoid an empty-tree assertion on DirectX 11. Not a dat0 concern, but worth knowing if macOS-only behavior diverges.

11. **The `gpui` crate uses Rust edition 2024** (`workspace.package.edition = "2024"` in gpui-component's Cargo.toml). dat0's `Cargo.toml` must match (or use 2021 with care). Edition 2024 is stable since Rust 1.85 (2025-02).

---

## 0.7 Pointers to source (for re-verification)

All paths are at the pinned SHAs.

- `gpui` examples: <https://github.com/zed-industries/zed/tree/08d95ad9d31f616a43dacda8416568d658dca6ae/crates/gpui/examples>
  - `hello_world.rs`, `set_menus.rs`, `window.rs`, `window_positioning.rs`, `window_shadow.rs`, `on_window_close_quit.rs`
- `gpui` source: `crates/gpui/src/{gpui,app,window,view,element,platform,styled,action,util,geometry}.rs` and `crates/gpui/src/platform/app_menu.rs`
- `gpui-component` examples: <https://github.com/longbridge/gpui-component/tree/0f0ab35233212f8f3277028995caf0c41e13ee6c/examples>
  - `hello_world`, `window_title`, `dialog_overlay`, `input`, `app_assets`
- `gpui-component` source: `crates/ui/src/{lib,styled,title_bar,dialog}.rs` and `crates/ui/src/{sidebar,setting,menu,dock}/`
- Zed `settings_ui`: `crates/settings_ui/src/settings_ui.rs` (line 2754 for the canonical `Render` impl)

---

## 0.8 Re-verification protocol

When bumping either pin:

1. Update the SHA + tag + date in the table at the top of this file.
2. Re-fetch the example files referenced in §0.2, §0.3, §0.4 and diff against the snippets above.
3. Re-fetch `crates/gpui/src/{app,window,element,platform/app_menu}.rs` and re-verify the API signatures in §0.2-§0.4.
4. Update §0.6 if any new gotcha appears or any existing one is resolved upstream.
5. Mirror the new SHAs into `docs/upstream-watch.md` "Current verified pins" subsection.
