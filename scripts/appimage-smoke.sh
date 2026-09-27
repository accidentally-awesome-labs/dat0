#!/usr/bin/env bash
# Start the AppImage on clean Linux systems and check that it draws its page.
#
#   scripts/appimage-smoke.sh <dat0.AppImage> <version> [image ...]
#
# Each system is a fresh container given what README asks of a Linux desktop,
# WebKitGTK 4.1, and the test's own display (Xvfb) and screenshot tool, and
# nothing else. The AppImage must print <version>, keep WebKit's helper
# processes running, and draw a page: a screenshot of a blank window holds a
# couple of hundred colours, and dat0's first page thousands. `--version` alone
# proves nothing about the page: an AppImage whose WebKit could not talk to the
# host's helpers printed its version and opened a blank window.
#
# Needs docker and ImageMagick. Screenshots land beside the AppImage, in smoke/.
set -euo pipefail

# ── Inside a container ─────────────────────────────────────────────────────────
if [ "${1:-}" = --inside ]; then
  version=$2 name=$3
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq >/dev/null
  apt-get install -y -qq --no-install-recommends \
    libwebkit2gtk-4.1-0 ca-certificates xvfb x11-apps >/dev/null
  work=$(mktemp -d)
  cp /in/dat0.AppImage "$work/"
  cd "$work"
  # A container has no FUSE, so the image is extracted rather than mounted.
  ./dat0.AppImage --appimage-extract >/dev/null
  printed=$(./squashfs-root/AppRun --version)
  echo "$name: $printed"
  case "$printed" in
    "dat0 $version "*) ;;
    *) echo "::error::$name: --version printed '$printed', not dat0 $version"; exit 1 ;;
  esac

  Xvfb :99 -screen 0 1400x900x24 >/dev/null 2>&1 &
  export DISPLAY=:99 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
    DAT0_CONFIG_DIR="$work/config" XDG_DATA_HOME="$work/data" XDG_CACHE_HOME="$work/cache"
  ./squashfs-root/AppRun >"/out/$name.log" 2>&1 &
  app=$!
  helpers() { grep -h -s -E '^(WebKitWebProces|WebKitNetworkPr)' /proc/[0-9]*/comm | sort -u | wc -l; }
  for _ in $(seq 60); do [ "$(helpers)" -ge 2 ] && break; sleep 0.5; done
  # The page draws after the helpers start; a mismatched pair dies in that time.
  sleep 15
  if ! kill -0 "$app" 2>/dev/null; then
    tail -20 "/out/$name.log"
    echo "::error::$name: dat0 exited"
    exit 1
  fi
  if [ "$(helpers)" -lt 2 ]; then
    tail -20 "/out/$name.log"
    echo "::error::$name: WebKit's helper processes are not running, so the window is blank"
    exit 1
  fi
  xwd -root -silent -out "/out/$name.xwd"
  kill "$app"
  exit 0
fi

# ── On the host ────────────────────────────────────────────────────────────────
appimage=$(realpath "$1")
version=$2
shift 2
images=("$@")
[ ${#images[@]} -gt 0 ] || images=(ubuntu:22.04 debian:12 ubuntu:24.04)
out="$(dirname "$appimage")/smoke"
mkdir -p "$out"
for image in "${images[@]}"; do
  name=${image//[:\/]/-}
  docker run --rm \
    -v "$appimage:/in/dat0.AppImage:ro" \
    -v "$(realpath "$0"):/in/smoke.sh:ro" \
    -v "$out:/out" \
    "$image" bash /in/smoke.sh --inside "$version" "$name"
  colours=$(convert "$out/$name.xwd" -format %k info:)
  convert "$out/$name.xwd" "$out/$name.png" && rm "$out/$name.xwd"
  if [ "$colours" -lt 1000 ]; then
    echo "::error::$name: the window drew $colours colours, which is a blank page (see smoke/$name.png)"
    exit 1
  fi
  echo "$name: the page drew ($colours colours)"
done
