use anyhow::Result;
use dat0_app::app_lock::{AppLock, OpenWindowMessage};
use dat0_app::main_bridge::MainThreadDispatcher;

fn main() -> Result<()> {
    dat0_app::boot::init_logging()?;
    let _ctx = dat0_app::boot::AppContext::boot()?;
    let state_dir = dat0_app::platform::data_dir()?;
    let cli_paths: Vec<std::path::PathBuf> = std::env::args().skip(1).map(Into::into).collect();
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
