#!/usr/bin/env bash
# Regenerate the cargo-about block of NOTICE.md in place.
#
#   scripts/notice-regen.sh        rewrite NOTICE.md
#   scripts/notice-regen.sh OUT    write the regenerated NOTICE.md to OUT and
#                                  leave NOTICE.md alone (the notice gate diffs
#                                  OUT against the committed file)
#
# Only the text between the `<!-- BEGIN cargo-about generated -->` and
# `<!-- END cargo-about generated -->` markers is generated. Everything around
# it — the project notice, and the icons, fonts and vendored JavaScript that
# cargo-about cannot see — is written by hand and copied through untouched.
# Redirecting `cargo about generate` into NOTICE.md replaces all of it.
#
# Linux only, and one cargo-about version only: the gate in
# .github/workflows/notice.yml runs on Linux with the version pinned below,
# and output from another host (PD-003) or another release differs. On a Mac,
# download the regenerated NOTICE.md from the failed `notice` check instead.
set -euo pipefail

# The single pin. notice.yml installs whatever this line says.
CARGO_ABOUT_VERSION=0.9.2

cd "$(git rev-parse --show-toplevel)"
out=${1:-NOTICE.md}

if [ "$(uname -s)" != Linux ]; then
  echo "error: NOTICE.md is generated on Linux only (PD-003)." >&2
  echo "Download NOTICE.md from the failed 'notice' check's artifacts instead." >&2
  exit 1
fi

have=$(cargo about --version 2>/dev/null | awk '{ print $2 }' || true)
if [ "$have" != "$CARGO_ABOUT_VERSION" ]; then
  echo "error: cargo-about $CARGO_ABOUT_VERSION is required (found: ${have:-none})" >&2
  echo "  cargo install cargo-about --locked --features cli --version $CARGO_ABOUT_VERSION" >&2
  exit 1
fi

generated=$(mktemp)
spliced=$(mktemp)
trap 'rm -f "$generated" "$spliced"' EXIT

cargo about generate -c about.toml docs/about-template.hbs >"$generated"

if ! awk -v gen="$generated" '
  /^<!-- BEGIN cargo-about generated -->/ {
    print
    while ((getline line < gen) > 0) print line
    inside = 1; saw_begin = 1
    next
  }
  /^<!-- END cargo-about generated -->/ { inside = 0; saw_end = 1 }
  !inside { print }
  END { exit !(saw_begin && saw_end) }
' NOTICE.md >"$spliced"; then
  echo "error: NOTICE.md is missing a cargo-about marker" >&2
  exit 1
fi

# `cat >` rather than `mv`, so NOTICE.md keeps its own permissions.
cat "$spliced" >"$out"
