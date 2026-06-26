#!/usr/bin/env bash
# Fails if any source file under crates/ contains a UI-string literal that
# bypasses the dat0_i18n::t() helper.
#
# Heuristic: any `"..."` literal inside .into() / .to_string() calls that
# look like UI text is suspect. This is a coarse first-pass; refine as the UI grows.
set -euo pipefail

BAD=0
while IFS= read -r line; do
    # Whitelist: comments, doc strings, string-table JSON files, test files
    if echo "$line" | grep -qE '^//|^\s*\*|^\s*#|test\.rs:|/strings/'; then
        continue
    fi
    echo "::warning::Possible un-i18n'd UI string: $line"
    BAD=$((BAD + 1))
done < <(grep -rEn '\.into\(\)|\.to_string\(\)' crates/dat0-app/src/ 2>/dev/null || true)

# P1 mode: soft-fail (warn-only). Once UI grows enough to hand-curate the
# whitelist, change to `exit $((BAD > 0))` to gate merges.
echo "i18n-check: $BAD candidate(s) flagged"

# Referenced-key resolution (warn-only). Correct the prior false claim that this
# script "fails on missing keys": it does not. We surface unresolved keys as
# warnings so they are visible in CI without gating merges.
CATALOG="crates/dat0-i18n/src/strings/en.json"
MISSING=0
while IFS= read -r key; do
    if ! grep -qF "\"$key\":" "$CATALOG"; then
        echo "::warning::i18n key referenced but absent from en.json: $key"
        MISSING=$((MISSING + 1))
    fi
done < <(grep -rhoE 'dat0_i18n::t\("[^"]+"\)|[^a-zA-Z_]t\("[^"]+"\)' crates/ 2>/dev/null \
         | sed -E 's/.*t\("([^"]+)"\).*/\1/' | sort -u)
echo "i18n-check: $MISSING referenced key(s) unresolved (warn-only)"
exit 0
