#!/usr/bin/env bash
set -euo pipefail
root="$(git rev-parse --show-toplevel)"
cd "$root"
blocked="$(git ls-files --cached --ignored --exclude-from=.gitignore)"
if [[ -n "$blocked" ]]; then
  printf '%s\n' 'Internal or local files must not be tracked:' "$blocked" >&2
  exit 1
fi
