//! Live theme switching.
//!
//! Port of `dat0-app/tests/theme_live_switch.rs`. The guarantee is the same and
//! the machinery is a tenth of the size: switching theme must repaint the whole
//! app at once, from one document, with no half-swapped frame and nothing left
//! over from the theme before.
//!
//! Under GPUI that took `Theme::switch` → `set_global` → `apply_config` on the
//! widget library's global → `refresh_windows`, and the two failure modes were
//! (a) a sparse config leaking shadcn defaults into the gaps and (b) the brand
//! sidecar and the palette swapping in different frames — a light ground under
//! the dark brand's `#f5a623`, which measures 1.97:1.
//!
//! Both are now unrepresentable *by construction*: one `ThemeTokens` struct
//! holds every value including the brand, and `ThemeStyle` renders exactly
//! `tokens.css_vars()` into one `<style>` element. Unrepresentable is a claim
//! about today's code, so it is asserted rather than trusted — the tests below
//! compare the emitted `:root` block against the builtin document byte for
//! byte, which is the strongest form of "nothing leaked and nothing lagged".

mod support;

use dioxus::prelude::*;
use support::Harness;
use support::dom::NodeKey;

use dat0_core::settings::store::SettingsStore;
use dat0_core::theme::tokens::{BUILTIN_IDS, ThemeTokens, builtin};
use dat0_ui::theme::{Theme, ThemeStyle};

#[derive(Clone, PartialEq, Default, Props)]
struct DriverProps {
    /// A `settings.toml` to read `theme.id` from. `None` is the cold-start
    /// case: no file, no persisted choice.
    #[props(default)]
    settings: Option<std::path::PathBuf>,
}

/// The shell's half of the contract: provide the theme, render the style
/// element, and offer one switch button per builtin.
#[component]
fn Driver(props: DriverProps) -> Element {
    let store = props.settings.clone().map(SettingsStore::with_path);
    let mut theme = Theme::provide(store.as_ref());

    rsx! {
        div {
            ThemeStyle {}
            span { "data-a11y-id": "probe-id", "{theme.tokens().id}" }

            for id in BUILTIN_IDS {
                button {
                    key: "{id}",
                    "data-a11y-id": "switch-{id}",
                    onclick: move |_| theme.set(id),
                    "{id}"
                }
            }
            button {
                "data-a11y-id": "switch-unknown",
                onclick: move |_| theme.set("does-not-exist"),
                "?"
            }
        }
    }
}

fn mount(props: DriverProps) -> Harness {
    Harness::new(Driver, props)
}

/// The `<style id="d0-theme">` node the whole app is painted from.
fn style_node(h: &Harness) -> NodeKey {
    h.dom()
        .walk()
        .into_iter()
        .find(|k| h.dom().get(*k).attr("id") == Some("d0-theme"))
        .expect("the theme style element must be in the tree")
}

/// Its content: the `:root{…}` declaration block.
fn root_block(h: &Harness) -> String {
    h.attr(style_node(h), "dangerous_inner_html")
        .expect("the style element carries the token block")
}

fn active_id(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("probe-id").expect("the id probe"))
}

/// The round trip: dark → light → high-contrast → dark.
///
/// At every step the emitted block must be *exactly* the block that theme's own
/// document produces. Byte equality is the full-coverage anti-leak assertion in
/// its strongest form: a value that came from anywhere else — a previous theme,
/// a default, a partially applied write — is a difference.
#[test]
fn switching_repaints_the_whole_app_from_one_document() {
    let mut h = mount(DriverProps::default());

    for id in ["dark", "light", "high-contrast", "dark"] {
        h.click(&format!("switch-{id}"));

        let tokens = builtin(id).expect("builtin");
        assert_eq!(active_id(&h), id);
        assert_eq!(
            root_block(&h),
            tokens.css_vars(),
            "{id}: the rendered :root block is not this theme's document"
        );
        assert!(
            root_block(&h).contains(&format!(
                "color-scheme:{}",
                if id == "dark" { "dark" } else { "light" }
            )),
            "{id}: the block does not declare its colour scheme"
        );
    }
}

/// Every token reaches the document on every theme, and reaches it once.
///
/// The GPUI original proved this indirectly, by picking one key the shadcn
/// defaults would have supplied (`secondary`) and asserting it differed. With
/// the whole token set in one struct the direct assertion is available: every
/// `--d0-*` name is declared, with this theme's value, exactly once.
#[test]
fn no_token_is_left_at_a_previous_themes_value() {
    let mut h = mount(DriverProps::default());

    for id in BUILTIN_IDS {
        h.click(&format!("switch-{id}"));
        let block = root_block(&h);
        let tokens = builtin(id).unwrap();

        for (name, value) in tokens.pairs() {
            let declaration = format!("{name}:{value};");
            let count = block.matches(&format!("{name}:")).count();
            let expected = if !tokens.shadow && name.starts_with("--d0-shadow-") {
                // High contrast overrides both shadow tokens to `none` after
                // declaring them, so the file value is present *and* replaced.
                2
            } else {
                1
            };
            assert_eq!(count, expected, "{id}: {name} declared {count} times");
            assert!(
                block.contains(&declaration),
                "{id}: {name} is not this theme's value ({value})"
            );
        }
    }
}

/// A switch is one signal write, not a re-mount.
///
/// This is what replaced `refresh_windows`: the old path re-applied a widget
/// library config and asked every window to rebuild, which is why a theme
/// change could show a frame of mixed palette. Here the style element's text
/// changes and the node itself — and therefore everything around it — stays put.
#[test]
fn a_switch_rewrites_the_style_element_without_remounting_it() {
    let mut h = mount(DriverProps::default());
    let before = style_node(&h);
    let before_id = h.dom().element_id_of(before);
    let before_block = root_block(&h);

    h.click("switch-dark");

    let after = style_node(&h);
    assert_eq!(
        before, after,
        "the style element was replaced, not rewritten"
    );
    assert_eq!(
        before_id,
        h.dom().element_id_of(after),
        "the style element was rebound to a new ElementId"
    );
    assert_ne!(before_block, root_block(&h), "nothing actually changed");
}

/// The brand and the palette are one document, so they cannot lag each other.
///
/// The failure this names is specific and was real: light's ground painted
/// under dark's amber-as-text, `#f5a623` on `#fcfcfb`, 1.97:1 — exactly the
/// pair light's darker `amber_text` exists to avoid. Storing the brand in the
/// same struct as the palette is what makes a half-swap unrepresentable; this
/// asserts it stayed that way across the full round trip.
#[test]
fn the_brand_can_never_lag_the_palette() {
    let mut h = mount(DriverProps::default());
    let dark = builtin("dark").unwrap();

    for id in ["dark", "light", "high-contrast", "dark"] {
        h.click(&format!("switch-{id}"));
        let block = root_block(&h);
        let tokens = builtin(id).unwrap();

        assert!(block.contains(&format!("--d0-canvas:{};", tokens.canvas)));
        assert!(block.contains(&format!("--d0-amber-text:{};", tokens.amber_text)));
        if id != "dark" {
            assert!(
                !block.contains(&format!("--d0-amber-text:{};", dark.amber_text)),
                "{id}: the ground moved but amber-as-text is still dark's"
            );
        }
    }
}

/// An unknown id lands on the default, and the default is light.
///
/// A behaviour change worth pinning: the GPUI build defaulted *and* fell back
/// to dark. `DEFAULT_ID` is now light (S9), and a fallback must not silently
/// leave the previous theme in place either — that would make a bad id look
/// like a successful switch.
#[test]
fn an_unknown_id_falls_back_to_the_default_not_to_the_previous_theme() {
    let mut h = mount(DriverProps::default());

    h.click("switch-dark");
    assert_eq!(active_id(&h), "dark");

    h.click("switch-unknown");
    assert_eq!(active_id(&h), "light");
    assert_eq!(root_block(&h), builtin("light").unwrap().css_vars());
}

/// Cold start with no settings at all resolves to the default.
///
/// The GPUI equivalent was `switch_without_component_global_still_installs_facade`:
/// a context with no widget-library global still had to produce a working
/// theme. There are no globals now; the equivalent hole is a window built
/// before any settings store exists, and it must produce a themed window
/// rather than an unpainted one.
///
/// **Known S9 gap, deliberately not asserted here.** With a store *present* and
/// no user choice in it, `get_string("theme.id")` does not answer `None` — it
/// answers `"dark"`, because `settings::schema::Theme::default()` still carries
/// the pre-redesign default. So the shipped app boots dark, not light. The flip
/// is a one-line change in `dat0-core`, and it is blocked until Phase 7 only
/// because `dat0-app/tests/settings_window.rs:632` pins the old value ("fresh
/// store must default theme.id=dark") and that crate has to stay green. Nothing
/// below asserts dark: pinning a value the design says is wrong would turn this
/// file into the reason it never gets fixed.
#[test]
fn a_window_with_no_settings_still_gets_the_default_theme() {
    let h = mount(DriverProps::default());
    assert_eq!(active_id(&h), "light");
    assert_eq!(root_block(&h), builtin("light").unwrap().css_vars());
}

/// An unreadable settings file is not a crash and not an unpainted window.
#[test]
fn an_unreadable_settings_file_still_produces_a_theme() {
    let h = mount(DriverProps {
        settings: Some(std::path::PathBuf::from("/definitely/not/a/settings.toml")),
    });
    let id = active_id(&h);
    assert!(
        dat0_core::theme::tokens::BUILTIN_IDS.contains(&id.as_str()),
        "an unreadable settings file resolved to {id:?}, which is not a builtin"
    );
    assert_eq!(root_block(&h), builtin(&id).unwrap().css_vars());
}

/// A persisted `theme.id` wins over the default.
///
/// The other half of S9's promise: light became the default, and anyone who had
/// chosen dark keeps dark.
#[test]
fn a_persisted_theme_id_wins_over_the_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.toml");
    let store = SettingsStore::with_path(path.clone());
    store.set("theme.id", "dark").expect("persist theme.id");

    let h = mount(DriverProps {
        settings: Some(path.clone()),
    });
    assert_eq!(active_id(&h), "dark");
    assert_eq!(root_block(&h), builtin("dark").unwrap().css_vars());

    // And a persisted id that no longer names a builtin degrades to the
    // default rather than to an unpainted window.
    store
        .set("theme.id", "solarized")
        .expect("persist a stale id");
    let h = mount(DriverProps {
        settings: Some(path),
    });
    assert_eq!(active_id(&h), "light");
}

/// The stylesheet is fetched, not inlined — and from the one URL builder.
///
/// `app.css` is a real file the editor can lint and the webview caches once per
/// window; only the token block, which changes, is inlined. A hand-rolled href
/// that the asset handler does not match is a window with no styles at all.
#[test]
fn the_static_stylesheet_is_linked_over_the_asset_protocol() {
    let h = mount(DriverProps::default());
    let link = h
        .dom()
        .walk()
        .into_iter()
        .find(|k| h.dom().get(*k).tag() == Some("link"))
        .expect("the stylesheet link");

    assert_eq!(h.attr(link, "rel").as_deref(), Some("stylesheet"));
    assert_eq!(
        h.attr(link, "href").as_deref(),
        Some(dat0_ui::protocol::url("app.css").as_str())
    );
    assert!(
        dat0_ui::protocol::Embedded::get("app.css").is_some(),
        "the linked stylesheet is not in the embed"
    );
}

/// The type the whole module hangs off is a plain data struct — no signal, no
/// context, no toolkit — so a theme can be resolved and inspected anywhere.
#[test]
fn tokens_are_ordinary_data() {
    let a: ThemeTokens = builtin("light").unwrap();
    let b = builtin("light").unwrap();
    assert_eq!(a, b, "two reads of one builtin must agree");
    assert_ne!(a, builtin("dark").unwrap());
}
