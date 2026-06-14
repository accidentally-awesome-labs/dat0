use anyhow::Result;
use dat0_app::app_lock::{AppLock, OpenWindowMessage};
use dat0_app::main_bridge::MainThreadDispatcher;

fn main() -> Result<()> {
    dat0_app::boot::init_logging()?;
    let ctx = dat0_app::boot::AppContext::boot()?;
    // P7a T9: publish the canonical recents store as a process-wide singleton so
    // the workspace open/save flows push into the same instance the app shares
    // (mirrors how the ActionRegistry singleton is installed below). `ctx` is
    // held for the lifetime of `main` (telemetry guard etc.), so the store lives
    // as long as the app.
    dat0_app::window_registry::install_recents(std::sync::Arc::clone(&ctx.recents));
    let state_dir = dat0_app::platform::data_dir()?;
    let cli_paths: Vec<std::path::PathBuf> = std::env::args().skip(1).map(Into::into).collect();

    // P8 T4: headless package front-door. If argv names a package subcommand
    // (export/unpack/inspect/replay/diff), run it WITHOUT GPUI or AppLock and
    // exit with its process code. A bare launch or a dropped file path returns
    // None and falls through to the GUI below. `std::process::exit` returns `!`,
    // so this short-circuit type-checks inside `main`'s `Result`.
    let raw: Vec<String> = std::env::args().collect();
    if let Some(cmd) = dat0_app::cli::parse(&raw) {
        std::process::exit(dat0_app::cli::run(cmd));
    }

    let lock = match AppLock::try_acquire(&state_dir)? {
        Some(l) => l,
        None => {
            // Another instance is running — forward the open-window request and exit.
            AppLock::forward_open_window(&state_dir, OpenWindowMessage { paths: cli_paths })?;
            return Ok(());
        }
    };

    // PD-010 closure: capture the dispatcher BEFORE Application::run so the
    // UDS handler (and any tokio task) can post closures onto the GPUI main
    // thread. The matching `MainLoop` is consumed inside `cx.spawn` from
    // `run_app`. See `crates/dat0-app/src/main_bridge.rs` for design notes.
    let (dispatcher, main_loop) = MainThreadDispatcher::new();
    dat0_app::window_registry::install_dispatcher(dispatcher);

    // P3b T3: build and publish the `ActionRegistry` singleton with all
    // seven built-in actions so the command palette (T6), Banner action
    // resolution (T2), and built-in dispatch closures can look up by
    // stable id. Built-ins that depend on `state_root` / `WindowRegistry`
    // resolve those via singletons installed inside `run_app`.
    let registry = dat0_app::actions::registry::ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&registry)
        .expect("built-in actions must register without conflict");
    dat0_app::window_registry::install_action_registry(registry);

    tracing::info!("dat0 starting");
    dat0_app::run_app(lock, cli_paths, main_loop)
}
