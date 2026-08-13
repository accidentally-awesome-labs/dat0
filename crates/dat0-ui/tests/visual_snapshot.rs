//! Tier 1 of the visual suite: every scene, SSR'd and snapshotted.
//!
//! Pure Rust, hermetic, no display, runs in CI. It catches every markup, class
//! and inline-style change in `crates/dat0-ui/src/components/` — the whole
//! class of regression that the headless harness cannot see, because that one
//! asserts *named* attributes and this one asserts the entire tree.
//!
//! ```text
//! cargo nextest run -p dat0-ui --test visual_snapshot
//! INSTA_UPDATE=always cargo test -p dat0-ui --test visual_snapshot   # accept
//! ```
//!
//! **Reading the diff is the check.** A snapshot suite whose diffs nobody reads
//! only proves the renderer is deterministic. `normalise` puts one tag per line
//! for exactly that reason.
//!
//! Tier 2 — real geometry in a real window — is `examples/visual_probe.rs`, and
//! it is not a CI job because hosted runners have no display.

use dioxus::prelude::*;

use dat0_core::theme::builtin;
use dat0_core::theme::tokens::BUILTIN_IDS;
use dat0_ui::visual::{Fixtures, Handle, SCENES, Scene, SceneHost, SceneHostProps, normalise};

/// Every scene, snapshotted — and every scene's `theme_sensitive` flag,
/// measured.
///
/// Each scene is rendered in all three builtins whatever its flag says, because
/// the flag is a claim about the markup and this is the only place that can
/// check it. A scene that claims to be theme-blind and is not would otherwise
/// have two of its three renderings go unsnapshotted forever.
///
/// `#[serial]` because `Fixtures::build` pins `DAT0_CONFIG_DIR`, which is
/// process-global — the same rule every suite in this crate that mounts the
/// shell already lives under.
#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn every_scene_renders() -> anyhow::Result<()> {
    let fx = Handle::new(Fixtures::build().await?);

    for scene in SCENES {
        let rendered: Vec<(&str, String)> = BUILTIN_IDS
            .iter()
            .map(|theme| (*theme, render(&fx, scene, theme)))
            .collect();

        for (theme, html) in &rendered {
            assert!(
                !html.is_empty(),
                "{} rendered nothing at all in {theme}",
                scene.id
            );
        }

        let differs = rendered.iter().any(|(_, h)| *h != rendered[0].1);
        assert_eq!(
            differs,
            scene.theme_sensitive,
            "{}: theme_sensitive is {} but the markup {} across themes. \
             Flip the flag in src/visual/mod.rs — a theme-blind scene must not \
             be snapshotted three times, and a theme-sensitive one must not be \
             snapshotted once.",
            scene.id,
            scene.theme_sensitive,
            if differs {
                "DOES differ"
            } else {
                "does NOT differ"
            },
        );

        if scene.theme_sensitive {
            for (theme, html) in &rendered {
                insta::assert_snapshot!(format!("{}__{theme}", scene.stem()), html);
            }
        } else {
            insta::assert_snapshot!(scene.stem(), rendered[0].1);
        }
    }
    Ok(())
}

/// The themes themselves.
///
/// The scene snapshots deliberately exclude `<style id="d0-theme">` —
/// `ThemeStyle` emits it outside the scene root — so this is where a token set
/// is actually pinned. Three small snapshots beat re-recording every scene
/// three times to catch a changed hex value.
#[test]
fn every_builtin_theme_resolves() {
    for id in BUILTIN_IDS {
        let vars = builtin(id).expect("a builtin theme").css_vars();
        insta::assert_snapshot!(format!("theme-vars__{id}"), vars);
    }
}

fn render(fx: &Handle, scene: &Scene, theme: &str) -> String {
    let mut dom = VirtualDom::new_with_props(
        SceneHost,
        SceneHostProps {
            fx: fx.clone(),
            id: scene.id.to_string(),
            theme: theme.to_string(),
        },
    );
    dom.rebuild_in_place();
    normalise(fx.scrub(dioxus_ssr::render(&dom)))
}
