#!/usr/bin/env bash
# Extracts the auto-generated section of NOTICE.md so it can be diffed
# against a freshly-generated `cargo about` output. Section is delimited
# by HTML comments emitted by docs/about-template.hbs.
set -euo pipefail

INPUT="${1:-NOTICE.md}"

awk '
    /^<!-- BEGIN cargo-about generated -->/ { capture = 1; next }
    /^<!-- END cargo-about generated -->/   { capture = 0; next }
    capture { print }
' "$INPUT"
