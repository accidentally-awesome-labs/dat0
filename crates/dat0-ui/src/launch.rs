//! Process boot and window creation.
//!
//! The ordering here is load-bearing and reproduces `window/boot.rs::run_app`'s
//! sequence. Two steps in particular are not stylistic:
//!
//! 1. **`--version` / `--help` short-circuit before logging.** `init_logging`
//!    writes an INFO banner to stdout, and `release.yml`'s Linux smoke test
//!    greps the *first* stdout line inside a bare `ubuntu:24.04` container. The
//!    banner used to be it. `tests/cli_version.rs` measures this.
//! 2. **The tokio runtime is entered around the event loop, not inside it.**
//!    Every `tokio::spawn` / `spawn_blocking` in the component tree resolves
//!    against that context; without it they panic at the first query.

use std::path::PathBuf;
use std::sync::Arc;

use dioxus::desktop::tao::dpi::LogicalSize;
use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;

use dat0_core::app_lock::{AppLock, OpenWindowMessage};
use dat0_core::events::{AppEvent, AppEvents};

/// Initial window size. Matches the GPUI build's.
const WINDOW_SIZE: (f64, f64) = (1280.0, 800.0);

/// The macOS traffic-light inset, matching the 44px titlebar's optical centre.
/// `components::shell` reserves 88px on the left for them.
#[cfg(target_os = "macos")]
const TRAFFIC_LIGHT_INSET: (f64, f64) = (12.0, 18.0);

/// Process entry. Returns the exit code.
pub fn main() -> anyhow::Result<()> {
    // The FIRST statement, so the cold-launch scenario measures from real
    // process start rather than from whenever something first asked.
    let _ = dat0_core::perf::PROCESS_START.set(std::time::Instant::now());

    let raw: Vec<String> = std::env::args().collect();

    // See the module docs: this must precede `init_logging`.
    if let Some(cmd @ (dat0_core::cli::PackageCmd::Version | dat0_core::cli::PackageCmd::Help)) =
        dat0_core::cli::parse(&raw)
    {
        std::process::exit(dat0_core::cli::run(cmd));
    }

    dat0_core::boot::init_logging()?;
    let ctx = dat0_core::boot::AppContext::boot()?;
    dat0_core::globals::install_recents(Arc::clone(&ctx.recents));

    let state_dir = dat0_core::platform::data_dir()?;
    dat0_core::globals::install_state_root(state_dir.clone());
    let cli_paths: Vec<PathBuf> = std::env::args().skip(1).map(Into::into).collect();

    // Headless package front-door: if argv names a package subcommand, run it
    // without a window or an AppLock and exit with its code. A bare launch or a
    // dropped path returns `None` and falls through to the GUI.
    if let Some(cmd) = dat0_core::cli::parse(&raw) {
        std::process::exit(dat0_core::cli::run(cmd));
    }

    let lock = match AppLock::try_acquire(&state_dir)? {
        Some(l) => l,
        None => {
            // Another instance owns the lock: hand it the paths and exit.
            AppLock::forward_open_window(&state_dir, OpenWindowMessage { paths: cli_paths })?;
            return Ok(());
        }
    };

    let registry = dat0_core::actions::registry::ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&registry)
        .expect("built-in actions must register without conflict");

    let _crash_guard = dat0_core::boot::CrashGuard::arm(&state_dir)?;

    tracing::info!("dat0 starting");
    let result = run_app(lock, cli_paths, registry);
    drop(_crash_guard); // explicit: clear the marker on a clean shutdown
    result
}

/// Own the event loop.
pub fn run_app(
    lock: AppLock,
    cli_paths: Vec<PathBuf>,
    registry: dat0_core::actions::registry::ActionRegistry,
) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let (events, rx) = AppEvents::channel();

    // The single-instance server: a second launch forwards its paths here
    // rather than starting a second process.
    {
        let events = events.clone();
        runtime.spawn(async move {
            // A failed listener means a second launch silently opens nothing
            // instead of a window, so it is logged rather than dropped — but it
            // must not take the running instance down.
            if let Err(e) = lock
                .serve(move |msg: OpenWindowMessage| {
                    events.send(AppEvent::OpenWindow { paths: msg.paths });
                })
                .await
            {
                tracing::error!("single-instance listener stopped: {e:#}");
            }
        });
    }

    // Entered around the event loop so every `tokio::spawn` inside a component
    // resolves. See the module docs.
    let _guard = runtime.enter();

    dioxus::LaunchBuilder::desktop()
        .with_cfg(config())
        .with_context(Boot {
            events,
            rx: Arc::new(parking_lot::Mutex::new(Some(rx))),
            registry,
            cli_paths: Arc::new(parking_lot::Mutex::new(cli_paths)),
        })
        .launch(crate::components::App);

    Ok(())
}

/// Everything the root component needs that cannot be recreated inside it.
#[derive(Clone)]
pub struct Boot {
    pub events: AppEvents,
    /// Taken exactly once, by the root's drain task.
    pub rx: Arc<parking_lot::Mutex<Option<dat0_core::events::AppEventRx>>>,
    pub registry: dat0_core::actions::registry::ActionRegistry,
    /// Paths from the command line, opened by the FIRST window only.
    ///
    /// Take-once for the same reason `rx` is: `Boot` is cloned into every
    /// window, and a second window that also opened them would duplicate every
    /// tab the user asked for once.
    pub cli_paths: Arc<parking_lot::Mutex<Vec<PathBuf>>>,
}

impl Boot {
    /// The CLI paths, once. Every later caller gets an empty vec.
    pub fn take_cli_paths(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *self.cli_paths.lock())
    }
}

/// The window configuration.
///
/// **`.with_menu(...)` must come after `.with_window(...)`**: `Config::with_window`
/// clears the menu when `decorations == false` (`dioxus-desktop/src/config.rs`),
/// so building them the other way round silently ships no menu bar.
pub fn config() -> Config {
    Config::new()
        .with_window(window_builder())
        .with_menu(crate::menu::build())
        .with_background_color(background_color())
}

/// The window's ground colour, so the first frame is not a white flash on a
/// dark theme. Read from the tokens rather than hard-coded.
fn background_color() -> (u8, u8, u8, u8) {
    let tokens = dat0_core::theme::builtin_or_default(dat0_core::theme::DEFAULT_ID);
    parse_hex(&tokens.canvas).unwrap_or((0xff, 0xff, 0xff, 0xff))
}

fn parse_hex(s: &str) -> Option<(u8, u8, u8, u8)> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    Some((
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
        0xff,
    ))
}

/// The `tao` window.
///
/// On macOS the titlebar is transparent with a full-size content view, because
/// dat0 draws its own 44px titlebar: the wordmark, the workspace name and the
/// live-source pill all live in the same bar as the traffic lights.
fn window_builder() -> WindowBuilder {
    let b = WindowBuilder::new()
        .with_title("dat0")
        .with_inner_size(LogicalSize::new(WINDOW_SIZE.0, WINDOW_SIZE.1))
        .with_min_inner_size(LogicalSize::new(720.0, 480.0));

    #[cfg(target_os = "macos")]
    {
        use dioxus::desktop::tao::dpi::LogicalPosition;
        use dioxus::desktop::tao::platform::macos::WindowBuilderExtMacOS;
        b.with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
            .with_traffic_light_inset(LogicalPosition::new(
                TRAFFIC_LIGHT_INSET.0,
                TRAFFIC_LIGHT_INSET.1,
            ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        b
    }
}

/// Open an additional window. Returns its `tao` id.
///
/// Each window gets its own `VirtualDom`, which is what makes multi-window work
/// at all here — the thing Blitz cannot do today (`DioxusNativeApplication`
/// holds a single `pending_window`).
pub async fn open_window(
    boot: Boot,
    paths: Vec<PathBuf>,
) -> Option<dioxus::desktop::tao::window::WindowId> {
    // The new window's `Boot` carries the paths as its own take-once CLI slot,
    // so "open these files in a new window" and "open the files this process
    // was launched with" are the same code path in the child.
    let boot = Boot {
        cli_paths: Arc::new(parking_lot::Mutex::new(paths)),
        ..boot
    };
    let dom = VirtualDom::new(crate::components::App).with_root_context(boot);
    let pending = dioxus::desktop::window().new_window(dom, config()).await;
    Some(pending.window.id())
}

/// Whether this tree is running inside a real desktop window.
///
/// False in the headless component harness, where there is no webview, no
/// window handle and no native dialog to show. The commands that need one —
/// file pickers, the settings window — check this and log instead of panicking
/// deep inside `dioxus::desktop::window()`.
///
/// This is not a test hook: a build with no window system genuinely cannot show
/// a file dialog, and saying so is better than aborting the process.
pub fn has_desktop() -> bool {
    dioxus::prelude::try_consume_context::<std::rc::Rc<dioxus::desktop::DesktopService>>().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_background_comes_from_the_default_theme() {
        let tokens = dat0_core::theme::builtin_or_default(dat0_core::theme::DEFAULT_ID);
        let want = parse_hex(&tokens.canvas).expect("canvas is a 6-digit hex");
        assert_eq!(background_color(), want);
        // Light is the default, so the first frame must not be dark.
        assert!(want.0 > 0xf0 && want.1 > 0xf0, "{want:?}");
    }

    #[test]
    fn a_malformed_colour_falls_back_rather_than_panicking() {
        assert_eq!(parse_hex("not a colour"), None);
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#ffffff"), Some((0xff, 0xff, 0xff, 0xff)));
    }
}
