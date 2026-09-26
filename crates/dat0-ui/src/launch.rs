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
use dat0_core::events::{AppEvent, AppEvents, Opening};

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

    // What the last run left behind, now that this is the only instance:
    // every scratch directory belongs to a window that is gone. Those holding
    // nothing to recover go; the rest are counted in one banner, which the
    // first window shows whenever it arrives. Off the main thread, so a long
    // list of old sessions does not hold the first frame: a window opening
    // meanwhile counts as open before its directory exists, so neither step
    // touches it. Recent workspaces are where an interrupted Save Workspace
    // is found.
    {
        let scratch = state_dir.join("scratch");
        let inspect = state_dir.join("inspect");
        let scan = std::thread::Builder::new()
            .name("dat0-recovery-scan".into())
            .spawn(move || {
                let swept = dat0_core::recovery_scan::sweep_scratch(&scratch);
                if swept > 0 {
                    tracing::info!(swept, "removed scratch sessions with nothing to recover");
                }
                let swept = dat0_core::recovery_scan::sweep_inspect(&inspect);
                if swept > 0 {
                    tracing::info!(swept, "removed packages closed windows had extracted");
                }
                let recents = dat0_core::globals::recents_snapshot();
                let _ = dat0_core::recovery_scan::recovery_scan_emit(&scratch, &recents);
            });
        // Worth a line in the log, not a failed launch: the next one scans.
        if let Err(e) = scan {
            tracing::warn!(error = %e, "could not start the recovery scan");
        }
    }

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

    let boot = Boot::new(registry, cli_paths);

    // The single-instance server: a second launch forwards its paths here
    // rather than starting a second process.
    {
        let events = boot.events.clone();
        runtime.spawn(async move {
            // A failed listener means a second launch silently opens nothing
            // instead of a window, so it is logged rather than dropped — but it
            // must not take the running instance down.
            if let Err(e) = lock
                .serve(move |msg: OpenWindowMessage| {
                    events.send(AppEvent::OpenWindow(Opening::files(msg.paths)));
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
        .with_context(boot)
        .launch(crate::components::App);

    Ok(())
}

/// Everything the root component needs that cannot be recreated inside it.
#[derive(Clone)]
pub struct Boot {
    /// The process bus: what belongs to no workbench window. A second launch
    /// forwards its paths here, and the settings window posts its cross-window
    /// controls here. Commands raised in a workbench window go on that
    /// window's own bus instead (`components::use_window_bus`).
    pub events: AppEvents,
    /// The process bus's receiver, held by one window at a time through a
    /// [`ProcessBusLease`].
    pub rx: Arc<parking_lot::Mutex<Option<dat0_core::events::AppEventRx>>>,
    /// Woken when a lease is dropped, so a waiting window takes the bus over.
    pub bus_free: Arc<tokio::sync::Notify>,
    /// The open workbench windows, and the one the menu bar acts on.
    pub windows: WindowRegistry,
    pub registry: dat0_core::actions::registry::ActionRegistry,
    /// What the window mounting this `Boot` opens on: the command line's paths
    /// for the first window, or what `open_window` was asked for.
    ///
    /// Take-once: `Boot` is cloned into every window, and a second window that
    /// also opened the same paths would duplicate every tab the user asked for
    /// once.
    pub opening: Arc<parking_lot::Mutex<Option<Opening>>>,
}

impl Boot {
    /// A boot around a fresh process bus, with no window open yet.
    pub fn new(
        registry: dat0_core::actions::registry::ActionRegistry,
        cli_paths: Vec<PathBuf>,
    ) -> Self {
        let (events, rx) = AppEvents::channel();
        Self {
            events,
            rx: Arc::new(parking_lot::Mutex::new(Some(rx))),
            bus_free: Arc::new(tokio::sync::Notify::new()),
            windows: WindowRegistry::default(),
            registry,
            opening: Arc::new(parking_lot::Mutex::new(Some(Opening::files(cli_paths)))),
        }
    }

    /// What this window opens on, once. Every later caller gets a fresh
    /// scratch window with no files.
    pub fn take_opening(&self) -> Opening {
        self.opening
            .lock()
            .take()
            .unwrap_or_else(|| Opening::files(Vec::new()))
    }
}

/// The open workbench windows, and the one a command from no particular
/// window acts on.
///
/// The menu bar raises commands from no particular window: macOS has one bar
/// per process, and dioxus hands each menu event to every window's handler on
/// every platform. The settings window's controls and a second launch's paths
/// belong to no workbench window either. All of them go to the workbench
/// window focused last or, before any has been focused, the one opened last
/// (PD-027).
///
/// Focus is recorded when it changes rather than asked for when a click
/// lands: an open GTK menu holds a keyboard grab, so the window a menu belongs
/// to can read as unfocused at the moment its item is chosen.
#[derive(Clone, Default)]
pub struct WindowRegistry(Arc<parking_lot::Mutex<Windows>>);

#[derive(Default)]
struct Windows {
    /// Each open window's own bus, in the order the windows opened.
    open: Vec<(uuid::Uuid, AppEvents)>,
    focused: Option<uuid::Uuid>,
    /// The workspace folder each workspace window holds.
    roots: Vec<(uuid::Uuid, PathBuf)>,
}

impl Windows {
    fn target(&self) -> Option<uuid::Uuid> {
        self.focused
            .filter(|f| self.bus(*f).is_some())
            .or_else(|| self.open.last().map(|(id, _)| *id))
    }

    fn bus(&self, window: uuid::Uuid) -> Option<&AppEvents> {
        self.open
            .iter()
            .find(|(id, _)| *id == window)
            .map(|(_, bus)| bus)
    }
}

impl WindowRegistry {
    /// `window` opened, and `bus` performs commands on it.
    pub fn opened(&self, window: uuid::Uuid, bus: AppEvents) {
        let mut w = self.0.lock();
        w.open.retain(|(id, _)| *id != window);
        w.open.push((window, bus));
    }

    /// `window` closed.
    pub fn closed(&self, window: uuid::Uuid) {
        let mut w = self.0.lock();
        w.open.retain(|(id, _)| *id != window);
        w.roots.retain(|(id, _)| *id != window);
        if w.focused == Some(window) {
            w.focused = None;
        }
    }

    /// `window` holds the workspace in `root`.
    pub fn holds(&self, window: uuid::Uuid, root: PathBuf) {
        let mut w = self.0.lock();
        w.roots.retain(|(id, _)| *id != window);
        w.roots.push((window, root));
    }

    /// The open window holding the workspace in `root`, if any.
    pub fn holding(&self, root: &std::path::Path) -> Option<uuid::Uuid> {
        let w = self.0.lock();
        w.roots
            .iter()
            .find(|(id, r)| r == root && w.bus(*id).is_some())
            .map(|(id, _)| *id)
    }

    /// `window` was focused.
    pub fn focused(&self, window: uuid::Uuid) {
        self.0.lock().focused = Some(window);
    }

    /// The window a command from no particular window acts on, if any is open.
    pub fn target(&self) -> Option<uuid::Uuid> {
        self.0.lock().target()
    }

    /// Post `ev` on `window`'s bus, or on the target's when `window` is
    /// `None`. False when that window is not open.
    pub fn send(&self, window: Option<uuid::Uuid>, ev: AppEvent) -> bool {
        let bus = {
            let w = self.0.lock();
            window
                .or_else(|| w.target())
                .and_then(|id| w.bus(id).cloned())
        };
        match bus {
            Some(bus) => {
                bus.send(ev);
                true
            }
            None => false,
        }
    }

    /// Post an event on every open window's bus.
    pub fn broadcast(&self, ev: impl Fn() -> AppEvent) {
        let buses: Vec<AppEvents> = self.0.lock().open.iter().map(|(_, b)| b.clone()).collect();
        for bus in buses {
            bus.send(ev());
        }
    }
}

/// One window's hold on the process bus.
///
/// Exactly one window drains the process bus at a time. The first window used
/// to take the receiver for good, so closing it left a second launch's
/// forwarded paths with nobody to open them (PD-027). A lease hands the
/// receiver back when it is dropped — which is what happens to the holding
/// window's drain task when the window closes — and wakes one waiting window
/// to take it over.
pub struct ProcessBusLease {
    rx: Option<dat0_core::events::AppEventRx>,
    boot: Boot,
}

impl ProcessBusLease {
    /// The bus, if no other window holds it.
    pub fn take(boot: &Boot) -> Option<Self> {
        let rx = boot.rx.lock().take()?;
        Some(Self {
            rx: Some(rx),
            boot: boot.clone(),
        })
    }

    /// The next process event, or `None` once every sender is gone.
    pub async fn next(&mut self) -> Option<AppEvent> {
        use futures::StreamExt as _;
        self.rx.as_mut()?.next().await
    }
}

impl Drop for ProcessBusLease {
    fn drop(&mut self) {
        if let Some(rx) = self.rx.take() {
            *self.boot.rx.lock() = Some(rx);
            // `notify_one` keeps a permit if nobody is waiting yet, so a
            // window that starts waiting a moment later is not missed.
            self.boot.bus_free.notify_one();
        }
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

/// Open an additional window on `opening`. Returns its `tao` id.
///
/// Each window gets its own `VirtualDom`, which is what makes multi-window work
/// at all here — the thing Blitz cannot do today (`DioxusNativeApplication`
/// holds a single `pending_window`).
pub async fn open_window(
    boot: Boot,
    opening: Opening,
) -> Option<dioxus::desktop::tao::window::WindowId> {
    // A session open in a window stays in that one: a second window on the
    // same database would open a second engine over it. The recovery panel
    // leaves open sessions out; this holds when its list is out of date.
    if let Opening::Recover { dir } = &opening
        && dat0_core::globals::is_live_scratch_dir(dir)
    {
        tracing::info!(dir = %dir.display(), "recover: that session is open already");
        return None;
    }
    // The new window's `Boot` carries the opening as its own take-once slot,
    // so "open these files in a new window" and "open the files this process
    // was launched with" are the same code path in the child.
    let boot = Boot {
        opening: Arc::new(parking_lot::Mutex::new(Some(opening))),
        ..boot
    };
    let dom = VirtualDom::new(crate::components::App).with_root_context(boot);
    let pending = dioxus::desktop::window().new_window(dom, config()).await;
    Some(pending.window.id())
}

/// Bring this window to the front, unminimised. Nothing without a window
/// system.
pub fn raise() {
    if !has_desktop() {
        return;
    }
    let window = dioxus::desktop::window();
    window.set_minimized(false);
    window.set_visible(true);
    window.set_focus();
}

/// The process bus, from anywhere in a window's tree. `None` where no [`Boot`]
/// was provided, as in the headless harness.
pub fn process_bus() -> Option<AppEvents> {
    try_consume_context::<Boot>().map(|boot| boot.events)
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

    fn test_boot() -> Boot {
        Boot::new(
            dat0_core::actions::registry::ActionRegistry::new(),
            Vec::new(),
        )
    }

    #[test]
    fn one_window_holds_the_process_bus_at_a_time() {
        let boot = test_boot();
        let lease = ProcessBusLease::take(&boot).expect("the first window gets the bus");
        assert!(
            ProcessBusLease::take(&boot).is_none(),
            "and nobody else does"
        );
        drop(lease);
        assert!(
            ProcessBusLease::take(&boot).is_some(),
            "a closed window's lease goes back for the next one"
        );
    }

    #[tokio::test]
    async fn a_waiting_window_takes_the_bus_over_when_its_holder_closes() {
        let boot = test_boot();
        let lease = ProcessBusLease::take(&boot).unwrap();

        // The second window: waits, then takes the bus and reads from it.
        let waiter = {
            let boot = boot.clone();
            tokio::spawn(async move {
                loop {
                    if let Some(mut lease) = ProcessBusLease::take(&boot) {
                        return lease.next().await;
                    }
                    boot.bus_free.notified().await;
                }
            })
        };
        tokio::task::yield_now().await;
        drop(lease); // the first window closes

        boot.events
            .send(AppEvent::OpenWindow(Opening::files(Vec::new())));
        let got = tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await
            .expect("the waiting window was woken")
            .unwrap();
        assert!(matches!(got, Some(AppEvent::OpenWindow(_))), "{got:?}");
    }

    fn window() -> (uuid::Uuid, AppEvents, dat0_core::events::AppEventRx) {
        let (bus, rx) = AppEvents::channel();
        (uuid::Uuid::now_v7(), bus, rx)
    }

    #[test]
    fn commands_from_no_window_go_to_the_window_focused_last() {
        let reg = WindowRegistry::default();
        assert_eq!(reg.target(), None, "no window, no target");

        let (a, a_bus, _a_rx) = window();
        let (b, b_bus, _b_rx) = window();
        reg.opened(a, a_bus);
        reg.opened(b, b_bus);
        assert_eq!(reg.target(), Some(b), "before any focus, the newest window");

        reg.focused(a);
        assert_eq!(reg.target(), Some(a));
        reg.focused(uuid::Uuid::now_v7()); // a window the registry never saw
        assert_eq!(reg.target(), Some(b), "an unknown window is no target");

        reg.focused(a);
        reg.closed(a);
        assert_eq!(reg.target(), Some(b), "the focused window closed");
        reg.closed(b);
        assert_eq!(reg.target(), None);
    }

    #[test]
    fn an_event_reaches_the_window_it_names_or_else_the_target() {
        let reg = WindowRegistry::default();
        let (a, a_bus, mut a_rx) = window();
        let (b, b_bus, mut b_rx) = window();
        reg.opened(a, a_bus);
        reg.opened(b, b_bus);
        reg.focused(b);

        let run = || AppEvent::RunAction {
            id: "sidebar.toggle",
            window: None,
        };
        assert!(reg.send(Some(a), run()));
        assert!(matches!(a_rx.try_recv(), Ok(AppEvent::RunAction { .. })));
        assert!(b_rx.try_recv().is_err(), "only the named window");

        assert!(reg.send(None, run()));
        assert!(matches!(b_rx.try_recv(), Ok(AppEvent::RunAction { .. })));
        assert!(a_rx.try_recv().is_err(), "only the target");

        reg.closed(a);
        assert!(!reg.send(Some(a), run()), "a closed window is not sent to");

        reg.opened(a, AppEvents::channel().0);
        reg.broadcast(|| AppEvent::ThemeChanged { id: "dark".into() });
        assert!(matches!(b_rx.try_recv(), Ok(AppEvent::ThemeChanged { .. })));
    }

    #[test]
    fn a_workspace_is_held_by_its_window_while_the_window_is_open() {
        let reg = WindowRegistry::default();
        let root = PathBuf::from("/work/sales");
        let (a, a_bus, _a_rx) = window();
        assert_eq!(reg.holding(&root), None);
        reg.holds(a, root.clone());
        assert_eq!(reg.holding(&root), None, "not until the window is open");
        reg.opened(a, a_bus);
        assert_eq!(reg.holding(&root), Some(a));
        assert_eq!(reg.holding(std::path::Path::new("/work/other")), None);
        reg.closed(a);
        assert_eq!(reg.holding(&root), None, "and not after it closes");
    }

    #[test]
    fn a_malformed_colour_falls_back_rather_than_panicking() {
        assert_eq!(parse_hex("not a colour"), None);
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#ffffff"), Some((0xff, 0xff, 0xff, 0xff)));
    }
}
