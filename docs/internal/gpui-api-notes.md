# GPUI API Verification Notes (P1.T0 spike)

This document is the canonical reference for the GPUI / gpui-component API surface used by P1. Subsequent tasks (T2 GPUI window, T14 macOS menu, T16 Settings panel, T17 Error/dialog primitives) MUST defer to this file when plan snippets contradict the actual API.

- **Verification date:** 2026-04-26
- **Verifier:** P1.T0 spike (read-only inspection of GitHub source at pinned SHAs)

---

## 0.1 Pinned commits

| Component | Tag | Date pinned | SHA (full 40-char) | Source / pin form |
|---|---|---|---|---|
| `gpui-component` (longbridge) | `v0.5.1` | 2026-02-05 | `0f0ab35233212f8f3277028995caf0c41e13ee6c` | git tag in `longbridge/gpui-component` |
| `gpui` (Zed) | `=0.2.2` (crates.io) | 2025-10-22 | `08d95ad9d31f616a43dacda8416568d658dca6ae` | crates.io publish commit in `zed-industries/zed`; commit message: "chore: Bump gpui to 0.2.2 (#40856)" |
| `gpui-macros` (Zed) | `=0.2.2` (crates.io) | 2025-10-22 | `08d95ad9d31f616a43dacda8416568d658dca6ae` | crates.io; same publish commit as `gpui` |

The literal `Cargo.toml` form for the Zed pins (exact-version, per the upstream-watch policy) is:

```toml
[dependencies]
gpui = "=0.2.2"
gpui-macros = "=0.2.2"
```

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

## 0.5b Dialog / Modal / Sheet primitives (T17 reference)

T17 implements error/dialog UX (modal, toast, banner). The canonical name in gpui-component v0.5.1 is **`Dialog`** — there is no separate `Modal` type; a "modal" in dat0's UX vocabulary maps to `Dialog` here. A separate **`Sheet`** primitive exists (slide-in panel from a window edge). Notifications (toasts) live in `gpui_component::notification` and are pushed via `WindowExt::push_notification`.

All Dialog / Sheet / Notification rendering depends on the window's root view being a `gpui_component::Root` — see §0.2 #2. Without the `Root::new(view, window, cx)` wrapper, calls to `window.open_dialog(...)`, `window.open_sheet(...)`, and `window.push_notification(...)` have no overlay layer to render into.

### `Dialog` struct (`crates/ui/src/dialog.rs:76`)

```rust
pub struct Dialog {
    style: StyleRefinement,
    title: Option<AnyElement>,
    footer: Option<FooterFn>,
    content: Div,
    width: Pixels,
    max_width: Option<Pixels>,
    margin_top: Option<Pixels>,
    on_close: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>,
    on_ok: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static>>,
    on_cancel: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static>,
    button_props: DialogButtonProps,
    close_button: bool,
    overlay: bool,
    overlay_closable: bool,
    keyboard: bool,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) layer_ix: usize,
    pub(crate) overlay_visible: bool,
}
```

### `Dialog` constructor + builder methods (`crates/ui/src/dialog.rs`)

Verbatim public signatures, with file:line references:

```rust
// dialog.rs:112
pub fn new(_: &mut Window, cx: &mut App) -> Self

// dialog.rs:130
pub fn title(mut self, title: impl IntoElement) -> Self

// dialog.rs:134
pub fn footer<E, F>(mut self, footer: F) -> Self
//   (where F builds the footer; full where-clause at the source)

// dialog.rs:151
pub fn confirm(self) -> Self

// dialog.rs:157
pub fn alert(self) -> Self

// dialog.rs:162
pub fn button_props(mut self, button_props: DialogButtonProps) -> Self

// dialog.rs:166
pub fn on_close(mut self, on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self

// dialog.rs:174
pub fn on_ok(mut self, on_ok: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static) -> Self

// dialog.rs:181
pub fn on_cancel(mut self, on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static) -> Self

// dialog.rs:188
pub fn close_button(mut self, close_button: bool) -> Self

// dialog.rs:192
pub fn margin_top(mut self, margin_top: Pixels) -> Self

// dialog.rs:196
pub fn w(mut self, width: Pixels) -> Self

// dialog.rs:202
pub fn width(mut self, width: Pixels) -> Self

// dialog.rs:206
pub fn max_w(mut self, max_width: Pixels) -> Self

// dialog.rs:210
pub fn overlay(mut self, overlay: bool) -> Self

// dialog.rs:214
pub fn overlay_closable(mut self, overlay_closable: bool) -> Self

// dialog.rs:220
pub fn keyboard(mut self, keyboard: bool) -> Self
```

Notes:
- `Dialog::new` requires both `&mut Window` and `&mut App`. It is **not** a free constructor — it depends on a live `Window` to allocate a `FocusHandle`.
- `confirm()` / `alert()` are preset variants (set `button_props` and behavior flags); use `confirm()` for OK/Cancel modals, `alert()` for OK-only.
- `on_ok` / `on_cancel` callbacks return `bool` — returning `false` aborts the close so validation can keep the dialog open.
- Children/content are added through `ParentElement` via `.child(...)` / `.children(...)` (Dialog implements `ParentElement` and `Styled`).

### `DialogButtonProps` builder (`crates/ui/src/dialog.rs:35`)

```rust
pub struct DialogButtonProps {
    ok_text: Option<SharedString>,
    ok_variant: ButtonVariant,
    cancel_text: Option<SharedString>,
    cancel_variant: ButtonVariant,
}

// dialog.rs:46
pub fn ok_text(mut self, ok_text: impl Into<SharedString>) -> Self
// dialog.rs:50
pub fn ok_variant(mut self, ok_variant: ButtonVariant) -> Self
// dialog.rs:54
pub fn cancel_text(mut self, cancel_text: impl Into<SharedString>) -> Self
// dialog.rs:58
pub fn cancel_variant(mut self, cancel_variant: ButtonVariant) -> Self
```

### How dialogs are opened — `WindowExt` trait (`crates/ui/src/root.rs:32`)

The opening API is on `WindowExt`, which is implemented for `gpui::Window` (re-exported as `gpui_component::WindowExt`). Verbatim trait signatures:

```rust
// root.rs:32
pub trait WindowExt: Sized {
    // root.rs:34
    fn open_sheet<F>(&mut self, cx: &mut App, build: F)
    where
        F: Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static;

    // root.rs:39
    fn open_sheet_at<F>(&mut self, placement: Placement, cx: &mut App, build: F)
    where
        F: Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static;

    // root.rs:44
    fn has_active_sheet(&mut self, cx: &mut App) -> bool;

    // root.rs:47
    fn close_sheet(&mut self, cx: &mut App);

    // root.rs:50
    fn open_dialog<F>(&mut self, cx: &mut App, build: F)
    where
        F: Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static;

    // root.rs:55
    fn has_active_dialog(&mut self, cx: &mut App) -> bool;

    // root.rs:58
    fn close_dialog(&mut self, cx: &mut App);

    // root.rs:61
    fn close_all_dialogs(&mut self, cx: &mut App);

    // root.rs:64
    fn push_notification(&mut self, note: impl Into<Notification>, cx: &mut App);

    // root.rs:67
    fn remove_notification<T: Sized + 'static>(&mut self, cx: &mut App);
}
```

Verbatim invocation from the `dialog_overlay` example (`examples/dialog_overlay/src/main.rs:10-14`):

```rust
fn show_dialog(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.title("Test dialog").child("Hello from dialog!")
    });
}
```

And the corresponding sheet open from the same file (`examples/dialog_overlay/src/main.rs:16-20`):

```rust
fn show_drawer(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
    window.open_sheet(cx, move |drawer, _, _| {
        drawer.title("Test Drawer").child("Hello from Drawer!")
    });
}
```

### Render-time wiring (`examples/dialog_overlay/src/main.rs:76-77`)

The view's `Render` impl must include the dialog and sheet layers as children — otherwise `Root` has the active dialog state but nothing draws it:

```rust
.children(Root::render_dialog_layer(window, cx))
.children(Root::render_sheet_layer(window, cx))
```

These are public methods on `gpui_component::Root` (`crates/ui/src/root.rs:309` and `:278` respectively). For T17, dat0's top-level view (the one wrapped by `Root::new`) must call both at the end of its render tree.

### `Sheet` struct (`crates/ui/src/sheet.rs:28`) — exists; verbatim definition

```rust
#[derive(IntoElement)]
pub struct Sheet {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) placement: Placement,
    pub(crate) size: DefiniteLength,
    resizable: bool,
    on_close: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>,
    title: Option<AnyElement>,
    footer: Option<AnyElement>,
    content: Div,
    margin_top: Pixels,
    overlay: bool,
    overlay_closable: bool,
}
```

`Sheet` builder methods (verbatim signatures, file:line):

```rust
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

`Sheet` defaults to `Placement::Right` with `350px` size and a top margin equal to `TITLE_BAR_HEIGHT`. Use `WindowExt::open_sheet_at(Placement::Bottom, ...)` etc. to slide from a different edge.

### Notes for T17 (banner / toast)

- **Modal** as a distinct type: does **not** exist. Use `Dialog` with `.overlay(true).overlay_closable(false)` for a hard-modal style, or `.confirm()` / `.alert()` presets.
- **Toast / notification**: use `WindowExt::push_notification(note, cx)`. The notification type lives in `gpui_component::notification` (declared as a public module at `crates/ui/src/lib.rs`). Toasts render into the same `Root` overlay layer; do not require a separate render-layer call beyond what `Root` already wires.
- **Banner**: gpui-component does not ship a dedicated banner primitive. T17 implements one as a styled `div` placed inline above main content (no overlay needed).
- **Cross-reference**: the `Root::new(view, window, cx)` wrapper is mandatory (§0.2 #2). T17 must verify T2 has shipped the `Root` wrapper before any dialog/toast work begins; otherwise the open_* calls succeed silently but render nothing.

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

---

## 0.9 File drop events (v0.2.2)

- **Verification date:** 2026-05-16
- **Verifier:** P3a.T0 spike (read-only inspection of `~/.cargo/registry/src/index.crates.io-*/gpui-0.2.2/src/`)
- **Source files inspected:** `interactive.rs` (ExternalPaths, FileDropEvent), `window.rs` (dispatch_event), `platform/mac/window.rs` (Cocoa drag callbacks)

### `ExternalPaths` — the file payload type (verbatim, `interactive.rs:496`)

```rust
/// A collection of paths from the platform, such as from a file drop.
#[derive(Debug, Clone, Default)]
pub struct ExternalPaths(pub(crate) SmallVec<[PathBuf; 2]>);

impl ExternalPaths {
    /// Convert this collection of paths into a slice.
    pub fn paths(&self) -> &[PathBuf] {
        &self.0
    }
}
```

`ExternalPaths` is **not** `Vec<PathBuf>` directly. The inner field is `pub(crate)` — callers must use `.paths()` to get a `&[PathBuf]` slice.

### `FileDropEvent` enum (verbatim, `interactive.rs:515`)

```rust
/// A file drop event from the platform, generated when files are dragged and dropped onto the window.
#[derive(Debug, Clone)]
pub enum FileDropEvent {
    /// The files have entered the window.
    Entered {
        position: Point<Pixels>,
        paths: ExternalPaths,
    },
    /// The files are being dragged over the window.
    Pending {
        position: Point<Pixels>,
    },
    /// The files have been dropped onto the window.
    Submit {
        position: Point<Pixels>,
    },
    /// The user has stopped dragging the files over the window.
    Exited,
}
```

`FileDropEvent` implements `InputEvent` and `MouseEvent`:

```rust
impl InputEvent for FileDropEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::FileDrop(self)
    }
}
impl MouseEvent for FileDropEvent {}
```

### How GPUI v0.2.2 translates file drop to drag-and-drop (verbatim, `window.rs:3622`)

```rust
// Translate dragging and dropping of external files from the operating system
// to internal drag and drop events.
PlatformInput::FileDrop(file_drop) => match file_drop {
    FileDropEvent::Entered { position, paths } => {
        self.mouse_position = position;
        if cx.active_drag.is_none() {
            cx.active_drag = Some(AnyDrag {
                value: Arc::new(paths.clone()),
                view: cx.new(|_| paths).into(),
                cursor_offset: position,
                cursor_style: None,
            });
        }
        PlatformInput::MouseMove(MouseMoveEvent { ... })
    }
    FileDropEvent::Pending { position } => {
        PlatformInput::MouseMove(MouseMoveEvent { ... })
    }
    FileDropEvent::Submit { position } => {
        cx.activate(true);
        PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, ... })
    }
    FileDropEvent::Exited => {
        cx.active_drag.take();
        PlatformInput::FileDrop(FileDropEvent::Exited)
    }
}
```

**Key finding:** GPUI v0.2.2 translates `FileDropEvent::Entered` into a `MouseMove` event with the `ExternalPaths` value stored as `cx.active_drag`. `FileDropEvent::Submit` becomes a `MouseUp` (Left button). The framework thus reuses the **drag-and-drop element API** (`on_drop<T>`) for file drops — there is no separate "on_file_drop" window-level handler.

### Consumer pattern — `on_drop<ExternalPaths>` on a `div`

To receive dropped files, attach `.on_drop::<ExternalPaths>(...)` to any element that should be a drop target. The callback signature (from `div.rs:462`):

```rust
pub fn on_drop<T: 'static>(
    &mut self,
    listener: impl Fn(&T, &mut Window, &mut App) + 'static,
)
```

In dat0's context, using the fluent `InteractiveElement::on_drop` form (from `div.rs:976`):

```rust
fn on_drop<T: 'static>(
    mut self,
    listener: impl Fn(&T, &mut Window, &mut App) + 'static,
) -> Self
```

Example wiring inside a `Render` impl (illustrative, not in a `.rs` file):

```rust
div()
    .size_full()
    .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(gpui::rgba(0x00_88_ff_22)))
    .on_drop::<ExternalPaths>(cx.listener(|view, paths: &ExternalPaths, _window, cx| {
        for path in paths.paths() {
            view.handle_dropped_file(path.clone(), cx);
        }
    }))
```

`cx.listener` (from `Context::listener`) binds `&mut Self` into the closure. The `drag_over` call provides a hover-highlight while files are dragged over the target.

### Extracting paths from `ExternalPaths`

`ExternalPaths.paths()` returns `&[PathBuf]`. There is no `into_vec()` method; clone each `PathBuf` if ownership is needed:

```rust
let owned: Vec<PathBuf> = paths.paths().to_vec();
```

### Drop handler thread — confirmed main thread

On macOS, `FileDropEvent` originates from Cocoa's `NSDraggingDestination` protocol methods (`draggingEntered`, `draggingUpdated`, `performDragOperation`, etc.) in `platform/mac/window.rs`. These callbacks dispatch via `send_new_event` which calls the window's `event_callback` directly on the calling thread — the macOS App Kit main thread.

GPUI's `Window::dispatch_event` (the function that translates `FileDrop` to internal drag events) is always called on the main thread. The `on_drop` callback therefore runs on **the GPUI main thread** — the same thread where all rendering and `Context`/`Window` mutation happen. No cross-thread synchronization is needed inside the callback.

### Summary for P3a T5 (`FileDropHandler`)

| Question | Answer |
|---|---|
| API entry point | `div().on_drop::<ExternalPaths>(...)` |
| Paths accessor | `ExternalPaths::paths() -> &[PathBuf]` |
| Payload type | `ExternalPaths` (wraps `SmallVec<[PathBuf; 2]>`), NOT `Vec<PathBuf>` |
| Handler thread | GPUI main thread (confirmed — Cocoa callbacks are main-thread) |
| Hover-highlight | `div().drag_over::<ExternalPaths>(...)` |
| Requires `Root` wrapper? | No — file drop is pure element-level, no overlay layer needed |

---

## 0.A Globals + cross-thread dispatch (v0.2.2)

- **Verification date:** 2026-05-25
- **Verifier:** P3b.T0 spike (read-only inspection of
  `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-0.2.2/src/`)
- **Source files inspected:** `global.rs`, `app.rs`, `gpui.rs` (`BorrowAppContext`),
  `app/async_context.rs`, `app/test_context.rs`,
  `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/futures-channel-0.3.32/src/mpsc/mod.rs`.
- **Why:** P3b §3.1 `MainThreadDispatcher` (closes PD-010) is a futures-mpsc →
  `cx.spawn` bridge, and P3b §3.12 theme live-switch reads `Theme: Global`.
  Both surfaces must be verified before T1 / T13 / T7 code lands.

### 0.A.1 `Global` trait (verbatim, `global.rs:22`)

```rust
/// A marker trait for types that can be stored in GPUI's global state.
pub trait Global: 'static {
    // This trait is intentionally left empty, by virtue of being a marker trait.
}
```

**The only bound is `'static`. No `Send`, no `Sync`, no `Default`.** Required
derives: none for the trait itself, though `Theme` derives `Debug + Clone +
Serialize + Deserialize + JsonSchema` (gpui-component side, not a GPUI
requirement). `set_global` consumes the value by move, so the implementor
chooses what derives suit storage.

Convenience traits also in `global.rs`:

```rust
// global.rs:30
pub trait ReadGlobal { fn global(cx: &App) -> &Self; }
impl<T: Global> ReadGlobal for T { ... }

// global.rs:44
pub trait UpdateGlobal {
    fn update_global<C, F, R>(cx: &mut C, update: F) -> R
    where C: BorrowAppContext, F: FnOnce(&mut Self, &mut C) -> R;

    fn set_global<C>(cx: &mut C, global: Self)
    where C: BorrowAppContext;
}
impl<T: Global> UpdateGlobal for T { ... }
```

### 0.A.2 `App` accessors (verbatim, `app.rs`)

```rust
// app.rs:1450 — does a global of this type exist?
pub fn has_global<G: Global>(&self) -> bool

// app.rs:1455 — read-only; PANICS if missing
#[track_caller]
pub fn global<G: Global>(&self) -> &G

// app.rs:1465 — read-only; None if missing
pub fn try_global<G: Global>(&self) -> Option<&G>

// app.rs:1472 — mutable ref; pushes a NotifyGlobalObservers effect; PANICS if missing
#[track_caller]
pub fn global_mut<G: Global>(&mut self) -> &mut G

// app.rs:1486 — like global_mut but creates a default value if missing (G: Default required)
pub fn default_global<G: Global + Default>(&mut self) -> &mut G

// app.rs:1497 — set or replace; pushes a NotifyGlobalObservers effect
pub fn set_global<G: Global>(&mut self, global: G)

// app.rs:1510 — remove and return; pushes a NotifyGlobalObservers effect; PANICS if missing
pub fn remove_global<G: Global>(&mut self) -> G

// app.rs:1522 — subscribe to changes; returns Subscription that must be kept alive
pub fn observe_global<G: Global>(
    &mut self,
    mut f: impl FnMut(&mut Self) + 'static,
) -> Subscription
```

`observe_global` callback signature is `FnMut(&mut App) + 'static`. The
`Subscription` returned must be stored (typically as a field on the observing
view) — drop it and the observer is unregistered.

### 0.A.3 `BorrowAppContext::update_global` (verbatim, `gpui.rs:241`)

```rust
pub trait BorrowAppContext {
    fn set_global<T: Global>(&mut self, global: T);
    fn update_global<G, R>(&mut self, f: impl FnOnce(&mut G, &mut Self) -> R) -> R
    where G: Global;
    fn update_default_global<G, R>(&mut self, f: impl FnOnce(&mut G, &mut Self) -> R) -> R
    where G: Global + Default;
}

impl<C> BorrowAppContext for C where C: BorrowMut<App> {
    fn set_global<G: Global>(&mut self, global: G) {
        self.borrow_mut().set_global(global)
    }
    #[track_caller]
    fn update_global<G, R>(&mut self, f: impl FnOnce(&mut G, &mut Self) -> R) -> R
    where G: Global,
    {
        let mut global = self.borrow_mut().lease_global::<G>();   // moves out of map
        let result = f(&mut global, self);
        self.borrow_mut().end_global_lease(global);              // moves back; pushes NotifyGlobalObservers
        result
    }
    fn update_default_global<G, R>(&mut self, f: impl FnOnce(&mut G, &mut Self) -> R) -> R
    where G: Global + Default,
    {
        self.borrow_mut().default_global::<G>();
        self.update_global(f)
    }
}
```

`update_global` works by **leasing the global onto the stack**, running `f`
with the leased value + `&mut C`, then putting it back via `end_global_lease`
(which pushes `Effect::NotifyGlobalObservers`). Inside `f` the global is **not
in the map** — a second `update_global<G, _>` for the same type during `f`
will panic in `lease_global` with "no global registered of type ..." (see
`app.rs:1538`). T13 must not recursively update the `Theme` global.

### 0.A.4 Storage scope — app-scoped (NOT window-scoped)

Source: `app.rs:552`:

```rust
pub(crate) globals_by_type: FxHashMap<TypeId, Box<dyn Any>>,
```

The map lives on `App`, which is the application-wide context. **One global
per `(App, TypeId)`** — shared across every window opened by that `App`.

Confirmation: there is no `globals_by_type` field on `Window`. Grepped
`gpui-0.2.2/src/window.rs` — no occurrence of `globals_by_type` outside `app.rs`.

For dat0:
- **`Theme: Global`** is shared across all windows in one `App`. Switching
  theme in one window (via `cx.update_global::<Theme, _>(...)`) immediately
  notifies observers in **every** window. Spec §3.12 "Theme live-switch" is
  correct in assuming this propagation.
- **`MainThreadDispatcher` (spec §3.1)** if registered as a `Global`, is also
  app-scoped — one dispatcher per `App`, used by every window. The plan
  registers it once in `crates/dat0-app/src/main.rs` before `Application::run`
  returns; consistent.

### 0.A.5 Notification semantics — automatic, deferred, deduplicated

Source: `app.rs:1180-1196`:

```rust
pub(crate) fn push_effect(&mut self, effect: Effect) {
    match &effect {
        Effect::Notify { emitter } => {
            if !self.pending_notifications.insert(*emitter) { return; }
        }
        Effect::NotifyGlobalObservers { global_type } => {
            if !self.pending_global_notifications.insert(*global_type) { return; }
        }
        _ => {}
    };
    self.pending_effects.push_back(effect);
}
```

Effects are deduplicated by type during one update cycle. The flush itself
runs at the end of every `App::update` (`app.rs:770-777`):

```rust
pub(crate) fn finish_update(&mut self) {
    if !self.flushing_effects && self.pending_updates == 1 {
        self.flushing_effects = true;
        self.flush_effects();
        self.flushing_effects = false;
    }
    self.pending_updates -= 1;
}
```

And the global-observer call site (`app.rs:1330-1335`):

```rust
fn apply_notify_global_observers_effect(&mut self, type_id: TypeId) {
    self.pending_global_notifications.remove(&type_id);
    self.global_observers
        .clone()
        .retain(&type_id, |observer| observer(self));
}
```

**Answer to plan question Step 5.4:** `update_global` / `set_global` /
`global_mut` / `remove_global` / `default_global` all push
`Effect::NotifyGlobalObservers`. **No explicit `cx.notify()` call is required.**
The flush runs automatically at the end of the surrounding `App::update`
(typically the closure passed to `App::spawn`, `Window::update`, or any of the
`Context::update_entity` family — all of these wrap in `start_update` /
`finish_update`). Observers registered via `observe_global<G>` fire on the
same tick.

**Implication for T13 (theme live-switch):** writing
`cx.update_global::<Theme, _>(|theme, _| { theme.mode = new_mode; })` is
sufficient. No extra `cx.notify()`, no manual refresh — the
`observe_global<Theme>` subscriptions inside every widget that depends on the
theme fire automatically, and every dirty window re-renders.

### 0.A.6 `cx.spawn` — exact closure shape (verbatim, `app.rs:1417`)

```rust
#[track_caller]
pub fn spawn<AsyncFn, R>(&self, f: AsyncFn) -> Task<R>
where
    AsyncFn: AsyncFnOnce(&mut AsyncApp) -> R + 'static,
    R: 'static,
{
    if self.quitting {
        debug_panic!("Can't spawn on main thread after on_app_quit")
    };
    let mut cx = self.to_async();
    self.foreground_executor.spawn(async move { f(&mut cx).await })
}
```

The bound is **`AsyncFnOnce(&mut AsyncApp) -> R`** — Rust's "async closure"
trait (stable since edition 2024). The verified call shape (from
`crates/gpui-component/examples/hello_world/src/main.rs:30-39`):

```rust
cx.spawn(async move |cx| {
    cx.open_window(WindowOptions::default(), |window, cx| {
        let view = cx.new(|_| Example);
        cx.new(|cx| Root::new(view, window, cx))
    })?;
    Ok::<_, anyhow::Error>(())
})
.detach();
```

**Use the `async move |cx| { ... }` form**, NOT `|cx| async move { ... }`.
The latter is the older "closure-returning-a-future" pattern; the gpui-0.2.2
`spawn` bound (`AsyncFnOnce`, not `FnOnce(&mut AsyncApp) -> Future`) accepts
both with edition 2024 trait coercions, but the gpui examples uniformly use
`async move |cx|` — match that for consistency.

The closure receives `&mut AsyncApp` (not `&mut App`). Reaching the real
`App` requires `cx.update(|app: &mut App| ...)` inside the future:

```rust
// crates/gpui-0.2.2/src/app/async_context.rs:142-146
pub fn update<R>(&self, f: impl FnOnce(&mut App) -> R) -> Result<R> {
    let app = self.app.upgrade().context("app was released")?;
    let mut lock = app.borrow_mut();   // <-- BorrowMut on the App's RefCell
    Ok(lock.update(f))
}
```

**`AsyncApp::update` returns `Result<R>`** (errors when the underlying `App`
has been dropped, e.g., during shutdown). The `?` operator inside the spawn
future propagates the error; pair with `.detach()` (or capture the `Task` and
ignore on shutdown).

### 0.A.7 `cx.update(|cx| ...)` thread-safety — confirmed unsafe off-main-thread

The key citations for PD-010:

```rust
// crates/gpui-0.2.2/src/app.rs:58-63 — AppCell is RefCell<App>
#[doc(hidden)]
pub struct AppCell {
    app: RefCell<App>,
}

// crates/gpui-0.2.2/src/app.rs:78-84 — borrow_mut panic site
#[doc(hidden)]
#[track_caller]
pub fn borrow_mut(&self) -> AppRefMut<'_> {
    if option_env!("TRACK_THREAD_BORROWS").is_some() {
        let thread_id = std::thread::current().id();
        eprintln!("borrowed {thread_id:?}");
    }
    AppRefMut(self.app.borrow_mut())   // <-- this is the RefCell::borrow_mut() call
}

// crates/gpui-0.2.2/src/app/async_context.rs:17-21
#[derive(Clone)]
pub struct AsyncApp {
    pub(crate) app: Weak<AppCell>,   // <-- Weak<Rc<RefCell<App>>>; Rc is !Send
    pub(crate) background_executor: BackgroundExecutor,
    pub(crate) foreground_executor: ForegroundExecutor,
}
```

`AppCell` wraps `RefCell<App>`. `AsyncApp` holds `Weak<AppCell>` (`Weak<Rc<...>>`).
**`Rc` is `!Send`**, so `Weak<AppCell>` is `!Send`. There is no `unsafe impl Send`
on `AppCell`, `App`, or `AsyncApp` (grepped). Therefore `AsyncApp` cannot
*physically* cross a thread boundary in safe Rust.

However: if a `Send`-able `AsyncApp` clone leaks (e.g., via `unsafe` or
through a `'static` future that somehow ends up polled off-main), calling
`AsyncApp::update` reaches `lock.borrow_mut()` on the `RefCell`. That call
**panics** when the cell is already borrowed elsewhere (the Cocoa event loop
holds a borrow during dispatch). On older rustc / debug builds, the panic is
deterministic; on release builds the same code path can produce UB if the
panic is caught and the borrow tracker is bypassed. PD-010 is the
authoritative diagnosis.

**P3b §3.1 design conclusion:** `MainThreadDispatcher` does NOT clone or carry
the `AsyncApp` across threads. It uses a `futures::channel::mpsc::Sender`
(which IS `Send`) to post a `Box<dyn FnOnce(&mut App) + Send>` closure. The
receiver side is consumed inside a `cx.spawn` future that stays on the
foreground executor:

```rust
// Sketch (pseudo-code for T1):
let (tx, mut rx) = futures::channel::mpsc::channel::<Box<dyn FnOnce(&mut App) + Send>>(64);
cx.set_global(MainThreadDispatcher { tx });

cx.spawn(async move |cx| {
    use futures::StreamExt;
    while let Some(closure) = rx.next().await {
        cx.update(|app| closure(app))?;   // safe — runs on foreground executor
    }
    Ok::<_, anyhow::Error>(())
}).detach();
```

The off-main-thread caller (tokio task, UDS handler, etc.) only ever touches
`tx.try_send(Box::new(move |app| ...))` — `Sender::try_send` does not borrow
the `App` and is thread-safe.

### 0.A.8 `futures::channel::mpsc` — verbatim semantics

Source: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/futures-channel-0.3.32/src/mpsc/mod.rs`.

```rust
// mpsc/mod.rs:385 — bounded channel
pub fn channel<T>(buffer: usize) -> (Sender<T>, Receiver<T>)

// mpsc/mod.rs:420 — unbounded channel
pub fn unbounded<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>)
```

`Sender` / `UnboundedSender` are `Send + Sync` (no `unsafe impl` needed — the
inner state is `Arc<…>` with atomic counters). `Receiver` / `UnboundedReceiver`
implement `Stream` (`mpsc/mod.rs:1126` and `:1320` respectively):

```rust
// mpsc/mod.rs:1126
impl<T> Stream for Receiver<T> {
    type Item = T;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> { ... }
}
// mpsc/mod.rs:1320
impl<T> Stream for UnboundedReceiver<T> { ... }
```

`Stream::Item = T` (not `Result<T, _>`). Consume with `StreamExt::next`:

```rust
use futures::StreamExt;
while let Some(item) = rx.next().await { /* ... */ }
```

**Drop behavior (verbatim, `mpsc/mod.rs:969-988`):**

```rust
impl<T> Drop for UnboundedSenderInner<T> {
    fn drop(&mut self) {
        let prev = self.inner.num_senders.fetch_sub(1, SeqCst);
        if prev == 1 {
            self.close_channel();
        }
    }
}
impl<T> Drop for BoundedSenderInner<T> {
    fn drop(&mut self) {
        let prev = self.inner.num_senders.fetch_sub(1, SeqCst);
        if prev == 1 {
            self.close_channel();
        }
    }
}
```

When the **last** sender drops, `close_channel` is called. The receiver's
`next_message` then sees `state.is_closed()` and returns `Poll::Ready(None)`
(`mpsc/mod.rs:1283-1289`):

```rust
None => {
    let state = decode_state(inner.state.load(SeqCst));
    if state.is_closed() {
        self.inner = None;
        Poll::Ready(None)
    } else {
        Poll::Pending
    }
}
```

**Answer to plan question Step 6.3:** Yes, the receiver-side `while let
Some(...)` loop exits cleanly when the last `Sender` is dropped. The
`Receiver::poll_next` returns `Poll::Ready(None)`, `StreamExt::next` yields
`None`, and the loop terminates. **Cloning a sender increments `num_senders`**;
the channel only closes when every clone has dropped. For T1, the
`MainThreadDispatcher` should store the `Sender` clone in the `Global`, and
the app shutdown path can drop the global (via `cx.remove_global`) to signal
the receiver loop to exit.

### 0.A.9 `App::test()` and safe test seams (Step 6.4 — CRITICAL question)

**Question:** Does `gpui::App::test()` (or any safe test seam yielding a usable
`&mut gpui::App`) exist?

**Answer:** **PARTIAL — yes, but behind a feature flag dat0 does not currently
enable.**

Source: `crates/gpui-0.2.2/src/app.rs:30-31`:

```rust
#[cfg(any(test, feature = "test-support"))]
pub use test_context::*;
```

And `crates/gpui-0.2.2/src/app/test_context.rs:122-149`:

```rust
impl TestAppContext {
    /// Creates a new `TestAppContext`. Usually you can rely on `#[gpui::test]`
    /// to do this for you.
    pub fn build(dispatcher: TestDispatcher, fn_name: Option<&'static str>) -> Self {
        let arc_dispatcher = Arc::new(dispatcher.clone());
        let background_executor = BackgroundExecutor::new(arc_dispatcher.clone());
        let foreground_executor = ForegroundExecutor::new(arc_dispatcher);
        let platform = TestPlatform::new(background_executor.clone(), foreground_executor.clone());
        let asset_source = Arc::new(());
        let http_client = http_client::FakeHttpClient::with_404_response();
        let text_system = Arc::new(TextSystem::new(platform.text_system()));

        Self {
            app: App::new_app(platform.clone(), asset_source, http_client),
            background_executor,
            foreground_executor,
            dispatcher,
            test_platform: platform,
            text_system,
            fn_name,
            on_quit: Rc::new(RefCell::new(Vec::default())),
        }
    }

    /// Create a single TestAppContext, for non-multi-client tests
    pub fn single() -> Self {
        let dispatcher = TestDispatcher::new(StdRng::seed_from_u64(0));
        Self::build(dispatcher, None)
    }
}
```

So `TestAppContext::single()` is the safe seam. **However**, it is
`#[cfg(any(test, feature = "test-support"))]` — gated by the `test-support`
feature on the `gpui` crate. dat0's workspace `Cargo.toml` currently declares
`gpui = "=0.2.2"` **without** the `test-support` feature (grepped — no
occurrence of `test-support` or `test_support` in `Cargo.toml` /
`crates/*/Cargo.toml` as of 2026-05-25).

`TestAppContext::single()` itself takes no arguments and constructs a fully
synthetic `App` over a `TestPlatform`. Its `app` field is
`pub Rc<AppCell>` — a unit test can call
`cx.app.borrow_mut()` to get a `&mut App` directly, or use the
`BorrowAppContext` / `AppContext` impls on `TestAppContext` for the higher-level
update API. Unit tests using this seam:

```rust
#[gpui::test]
fn my_test(cx: &mut gpui::TestAppContext) {
    cx.update(|app| {
        app.set_global(MyGlobal::default());
        // ...
    });
}
```

**For P3b T1 (`MainThreadDispatcher` + tests):** the `unsafe { std::mem::zeroed() }`
shim referenced in the T1 plan can be replaced by `TestAppContext` **provided
dat0 enables `gpui/test-support` as a `[dev-dependencies]` feature** in
`crates/dat0-app/Cargo.toml`. The recommended snippet:

```toml
# crates/dat0-app/Cargo.toml
[dev-dependencies]
gpui = { workspace = true, features = ["test-support"] }
```

This adds a dev-only dep on the test seam without affecting the release build.
T1 should make this dep flip part of its acceptance, then replace the shim
with `TestAppContext::single()`.

### 0.A.10 Summary table (P3b T1 / T7 / T13 quick reference)

| Question (plan Step) | Verified answer | Source citation |
|---|---|---|
| Step 5.1: `set_global<T: Global>` exact signature | `pub fn set_global<G: Global>(&mut self, global: G)` | `app.rs:1497` |
| Step 5.1: `update_global<T, R>` exact signature | `fn update_global<G, R>(&mut self, f: impl FnOnce(&mut G, &mut Self) -> R) -> R where G: Global` | `gpui.rs:245`, impl `gpui.rs:263` |
| Step 5.1: `observe_global<T>` exact signature | `pub fn observe_global<G: Global>(&mut self, mut f: impl FnMut(&mut Self) + 'static) -> Subscription` | `app.rs:1522` |
| Step 5.1: `try_global<T>` exact signature | `pub fn try_global<G: Global>(&self) -> Option<&G>` | `app.rs:1466` |
| Step 5.2: `Global` trait bound | `Global: 'static` only — NO `Send`/`Sync` | `global.rs:22` |
| Step 5.3: app-scoped or window-scoped? | App-scoped — `App::globals_by_type: FxHashMap<TypeId, Box<dyn Any>>` | `app.rs:552` |
| Step 5.4: notify automatically? | Yes — `set_global` / `update_global` / `global_mut` / `default_global` / `remove_global` all push `Effect::NotifyGlobalObservers`; flushed at end of `App::update` | `app.rs:1476,1488,1499,1512,1552`; flush at `app.rs:1330-1335` |
| Step 6.1: exact `cx.spawn` closure shape | `cx.spawn(async move \|cx\| { ... })` — `AsyncFnOnce(&mut AsyncApp) -> R + 'static` | `app.rs:1417-1430`; canonical example `examples/hello_world/src/main.rs:30` |
| Step 6.2: `cx.update` panic from non-main? | Yes — `AsyncApp::update` calls `app.borrow_mut()` on `RefCell<App>`; `AppCell = RefCell<App>` (`app.rs:61`); `Weak<AppCell>` is `!Send` so the panic is normally prevented by the type system, but `unsafe`-ly sending a clone triggers `RefCell::borrow_mut` panic from off-main | `async_context.rs:142-146`; `app.rs:61-95` |
| Step 6.3: sender drop → receiver exits? | Yes — `Drop for {Unbounded,Bounded}SenderInner` decrements `num_senders`; at zero calls `close_channel`; `Receiver::next_message` then returns `Poll::Ready(None)` | `futures-channel-0.3.32/src/mpsc/mod.rs:969-988, 1283-1289, 1320-1330` |
| Step 6.4: safe `&mut App` test seam? | **PARTIAL** — `gpui::TestAppContext::single()` exists, gated by `cfg(any(test, feature = "test-support"))`; dat0 must opt into `gpui/test-support` as a dev-dep feature | `app.rs:30-31`; `app/test_context.rs:122-149` |

### 0.A.11 — T1 manual UAT path for visual second-launch

Cross-references: PD-010 (closed by T1), §0.A.7 (the `RefCell`/`!Send`
diagnosis), §0.A.9 (`TestAppContext` safe seam). This subsection documents
the **manual** UAT recipe the `#[ignore]`d integration test points to.

**Why one of the T1 integration tests is `#[ignore]`d.** The visual
second-launch assertion — "second `cargo run -p dat0-app` invocation makes
a new window appear in the running instance" — needs a live GPUI event
loop pinned to the platform main thread (Cocoa `NSApplication`, X11/Wayland
connection, etc.). `cargo test` runs each `#[test]` (and `#[tokio::test]`)
on a worker thread the harness chose, *not* on the process's platform
main thread, and gpui's `Application::run` is not safe to start from a
worker thread inside a `#[tokio::test]` harness:

- `Application::run` takes over the calling thread for the platform event
  loop (it never returns until the app quits — `app.rs:174` and per §0.2).
- gpui 0.2.2 has no headless / no-window variant of `Application::run`;
  the only off-thread seam is `TestAppContext::single()` (§0.A.9), which
  supplies a `&mut App` but no event loop and no window backend.
- Even if the event loop were started on a worker, the `RefCell<App>`
  borrow rules (§0.A.7) make it racy to drive UI work from outside that
  thread.

The integration test `second_launch_spawns_visual_window` in
`crates/dat0-app/tests/single_instance.rs` is therefore marked
`#[ignore]` with a reason string and a body that
`unimplemented!()`s — its purpose is to be a discoverable anchor for the
manual UAT recipe below, not to run under `cargo test`.

**Compensating automated coverage.** The same UDS → dispatcher → main-thread
plumbing is exercised by the sibling test
`second_launch_forwards_and_dispatches_to_main_thread` in the same file.
That test substitutes an `AtomicUsize` increment for the
`WindowRegistry::open()` call inside the dispatched closure, then drives
the dispatcher's drain against a real `&mut gpui::App` supplied by
`TestAppContext::single()` (the safe seam established in §0.A.9). The
assertion "closure ran exactly once after the UDS forward" is the same
shape as "window count grew by one after the UDS forward" — the only
delta is what the closure does once it's on the main thread, which is the
piece the manual UAT verifies visually.

**Manual UAT recipe (visual second-launch).** Run from two separate
shells with the workspace as cwd:

```bash
# Terminal A — start the first instance
cargo run -p dat0-app
# (leave it running; a window appears)

# Terminal B — simulate a second launch
cargo run -p dat0-app
# (this process must exit ~immediately after forwarding via UDS;
#  it must NOT open its own window)
```

Pass criteria:

1. Terminal A's existing instance grows from one window to two visible
   windows. The new window is empty (no file paths were forwarded) and
   focusable independently of the first.
2. Terminal B's process exits with status 0 within ~1 s of launch.
3. Terminal A's logs (with `RUST_LOG=dat0=debug`) show a
   `single_instance: forwarded open-window message` line followed by a
   `window_registry: opened window` line on the main thread.

Failure modes to watch for:

- Terminal B spawns its own window (single-instance check broke).
- Terminal A panics with a `BorrowMutError` (PD-010 regression — the
  dispatcher is being bypassed and `AsyncApp::update` is being called
  off-main).
- Second window never appears but no panic (UDS message reached the
  handler but the dispatcher closure never drained — likely the
  `MainThreadDispatcher` global wasn't installed in `main.rs`).

**Revisit in T13 retro.** T13 (post-implementation retrospective) should
either (a) replace this manual UAT with an automated end-to-end test if
a headless gpui seam becomes available upstream, or (b) explicitly accept
the manual path as the long-term shape for this assertion and move the
recipe into `docs/ci.md` alongside other manual smokes. Track in the T13
deliverable as "P3b T1 manual UAT — promote or accept".

- T7 manual UAT: `cargo run -p dat0-app` against an empty `$STATE`
  (`rm -rf "$HOME/Library/Application Support/dat0"` on macOS) and an
  empty recents file; verify the two-column empty-state hero ("Drop a
  file to start" left, samples picker right) appears on the first
  window. Toggle by registering a recent (drop any CSV) and relaunching
  — right column should now read "Recents…" instead.
