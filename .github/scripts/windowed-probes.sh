#!/usr/bin/env bash
# Run the real-window probes and fail if any of them does.
#
# Each probe opens a real dat0 window — WebKitGTK on Linux — drives or measures
# it, prints a report, and exits 0 only when every check passed. They are the
# one kind of test here that sees what a user sees: layout, focus, the SQL
# editor, a second window. The headless suite cannot, which is how screens that
# rendered could still do nothing (PD-023).
#
#   .github/scripts/windowed-probes.sh [probe ...]    default: all six
#
# Needs a display. CI runs it under `xvfb-run`; on a desktop it uses yours.
# Expects the probes already built:
#   cargo build --release -p dat0-ui --example <probe> ...
set -uo pipefail

if [ -z "${DISPLAY:-}" ]; then
  echo "error: no DISPLAY. Run under xvfb-run, or from a desktop session." >&2
  exit 2
fi

probes=("$@")
if [ ${#probes[@]} -eq 0 ]; then
  probes=(shell_probe window_probe modal_trap_probe settings_window_probe console_probe visual_probe)
fi

root=$(git rev-parse --show-toplevel)
bin=$root/target/release/examples

# WebKitGTK's DMA-BUF renderer wants a GPU a virtual display does not have.
export WEBKIT_DISABLE_DMABUF_RENDERER=1

# A throwaway profile per probe. Kept short: the single-instance socket lives
# under XDG_DATA_HOME, and a Unix socket path must fit in 108 bytes.
profile=$(mktemp -d /tmp/d0p.XXXXXX)
trap 'rm -rf "$profile"' EXIT

# A private session bus per probe: GTK and the keychain's Secret Service backend
# both talk D-Bus, and a probe must not reach the host's.
wrap=(timeout 300)
if command -v dbus-run-session >/dev/null; then
  wrap+=(dbus-run-session --)
fi

failed=()
for p in "${probes[@]}"; do
  echo "::group::$p"
  rm -rf "${profile:?}"/*
  DAT0_CONFIG_DIR=$profile/c XDG_DATA_HOME=$profile/d \
    XDG_CACHE_HOME=$profile/k XDG_CONFIG_HOME=$profile/x \
    "${wrap[@]}" "$bin/$p"
  rc=$?
  echo "::endgroup::"
  if [ $rc -ne 0 ]; then
    echo "::error::$p failed (exit $rc)"
    failed+=("$p")
  fi
done

if [ ${#failed[@]} -ne 0 ]; then
  echo "failed: ${failed[*]}"
  exit 1
fi
echo "all ${#probes[@]} probes passed"
