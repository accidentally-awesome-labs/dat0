//! Does `document::eval` serialize scripts, and does a returned script's
//! channel keep working?
//!
//! Both questions decide the SQL console's architecture, and neither is
//! documented. The editor needs a **long-lived push channel** (CodeMirror emits
//! `change` / `cursor` / `run` whenever the user types) *and* the ability to
//! send commands. If scripts are serialized, a channel script that stays alive
//! blocks every command; if a returned script's channel dies, a short one
//! cannot be the channel.
//!
//! ```text
//! cd crates/dat0-ui && cargo run --example eval_probe
//! ```

use dioxus::prelude::*;

fn main() {
    dioxus::LaunchBuilder::desktop().launch(App);
}

/// Sleeps 1.5 s, then sends. If scripts are serialized, nothing else runs
/// meanwhile.
const SLOW: &str = r#"
await new Promise((r) => setTimeout(r, 1500));
dioxus.send("slow done");
await new Promise((r) => setTimeout(r, 0));
"#;

/// Returns immediately.
const FAST: &str = r#"
dioxus.send("fast done");
await new Promise((r) => setTimeout(r, 0));
"#;

/// Sends once, returns, then keeps pushing from a timer the script no longer
/// owns. Tests whether a finished script's channel still delivers.
const LINGER: &str = r#"
let n = 0;
const t = setInterval(() => {
  n++;
  try { dioxus.send("tick " + n); } catch (e) { clearInterval(t); }
  if (n >= 3) clearInterval(t);
}, 200);
dioxus.send("linger started");
await new Promise((r) => setTimeout(r, 0));
"#;

#[component]
fn App() -> Element {
    use_effect(move || {
        spawn(async move {
            let t0 = std::time::Instant::now();

            // Start the slow script but do not await it yet.
            let mut slow = document::eval(SLOW);
            // Then a fast one. If it comes back first, scripts run concurrently.
            let mut fast = document::eval(FAST);

            let fast_msg = fast.recv::<String>().await;
            let fast_at = t0.elapsed();
            let slow_msg = slow.recv::<String>().await;
            let slow_at = t0.elapsed();

            println!("--- dioxus eval semantics ---");
            println!("  fast   {fast_msg:?} at {fast_at:?}");
            println!("  slow   {slow_msg:?} at {slow_at:?}");
            println!(
                "  scripts are {}",
                if fast_at < slow_at {
                    "CONCURRENT (a slow script does not block others)"
                } else {
                    "SERIALIZED (a slow script blocks the queue)"
                }
            );

            // Does a finished script's channel keep delivering?
            let mut linger = document::eval(LINGER);
            let first = linger.recv::<String>().await;
            let mut ticks = Vec::new();
            for _ in 0..3 {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(1500),
                    linger.recv::<String>(),
                )
                .await
                {
                    Ok(Ok(m)) => ticks.push(m),
                    _ => break,
                }
            }
            println!("  linger {first:?} then {ticks:?}");
            println!(
                "  a returned script's channel {}",
                if ticks.is_empty() {
                    "DIES (a push channel must stay running)"
                } else {
                    "SURVIVES (a short script can be the channel)"
                }
            );
            std::process::exit(0);
        });
    });

    rsx! { div { "eval probe" } }
}
