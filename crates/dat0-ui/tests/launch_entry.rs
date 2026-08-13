//! The process entry point's shape.
//!
//! Renamed from the GPUI build's `window_smoke.rs`: the entry moved out of a
//! `window` module into [`dat0_ui::launch`], and its third parameter changed
//! from a `MainLoop` (the GPUI main-thread closure pump) to the
//! `ActionRegistry` the menu and palette dispatch through. The guarantee is
//! unchanged — `main.rs` is one line, so anything that binds the boot sequence
//! must be pinned here or nothing pins it.

#[test]
fn the_crate_exposes_run_app_with_the_boot_signature() {
    let _: fn(
        dat0_core::app_lock::AppLock,
        Vec<std::path::PathBuf>,
        dat0_core::actions::registry::ActionRegistry,
    ) -> anyhow::Result<()> = dat0_ui::launch::run_app;
}

#[test]
fn the_binary_entry_point_is_the_launch_module() {
    // `main.rs` delegates to this and nothing else; a signature change here is
    // a change to what `dat0` does when you double-click it.
    let _: fn() -> anyhow::Result<()> = dat0_ui::launch::main;
}
