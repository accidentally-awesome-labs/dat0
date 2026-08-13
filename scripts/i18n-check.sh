#!/usr/bin/env bash
# Two independent passes over the i18n surface.
#
#   Pass 1 (WARNING)  — heuristic hunt for UI-string literals that bypass
#                       `dat0_i18n::t()`. Coarse by construction; it false-
#                       positives on every non-UI `.into()` / `.to_string()`
#                       in the crate, so it can inform but must never gate.
#
#   Pass 2 (GATE)     — every key the code REFERENCES must exist in en.json.
#                       This one is exact, not heuristic: `t()` echoes the key
#                       back on a miss, so an unresolved key is a raw
#                       `catalog.empty.files` rendered into the user's face.
#                       There is nothing to hand-curate and no judgement call,
#                       which is why this half gates and pass 1 does not.
#
# SCOPE: `crates/*/src` only. Test files are deliberately excluded — several
# tests reference deliberately-absent keys to assert the miss behaviour itself
# (crates/dat0-i18n/tests/basic.rs:10 uses "does.not.exist";
# crates/dat0-core/tests/i18n_p10c_keys.rs:18 asserts an orphan key STAYS
# removed). Scanning them would make the gate demand the very keys other tests
# demand be absent.
set -euo pipefail

CATALOG="crates/dat0-i18n/src/strings/en.json"
SRC_DIRS=(crates/*/src)

# ── Pass 1 — un-i18n'd literal heuristic (WARN ONLY) ────────────────────────
BAD=0
while IFS= read -r line; do
    # Whitelist: comments, doc strings, string-table JSON files.
    if echo "$line" | grep -qE '^//|^\s*\*|^\s*#|/strings/'; then
        continue
    fi
    echo "::warning::Possible un-i18n'd UI string: $line"
    BAD=$((BAD + 1))
# Scoped to the crates that actually render user-facing strings. The engine,
# format and keychain crates speak to callers, not to people, and including
# them buries the signal under ~600 irrelevant `.into()` hits.
done < <(grep -rEn '\.into\(\)|\.to_string\(\)' crates/dat0-core/src/ crates/dat0-ui/src/ 2>/dev/null || true)
echo "i18n-check: $BAD candidate(s) flagged (pass 1, advisory)"

# ── Pass 2 — referenced-key resolution (HARD GATE) ──────────────────────────
#
# Two extractors, because there are two ways a key reaches `t()`:
#
#  (a) String-literal call sites — `t("catalog.title")`. A regex sees these.
#
#  (b) Const key-lists — `pub const CATALOG_EMPTY_KEYS: &[&str] = &[…]`. These
#      exist precisely BECAUSE their call sites compose the key at runtime
#      (from a group name, an enum variant name, …), so extractor (a) is
#      structurally blind to them. Declaring the list is the contract that
#      makes them checkable; this extractor is the half that reads it.
#      Consumers today: `error_ux::engine::ENGINE_ERROR_KEYS`,
#      `catalog::CATALOG_EMPTY_KEYS`.
# Comment lines are stripped BEFORE the regex runs. Without that, any doc
# comment that mentions a `t("…")` call site in prose — which the two
# extractors below are themselves documented with — makes the gate demand a key
# named `…`. Measured: `catalog/mod.rs` and `view/title_bar.rs` both tripped it
# the day this pass started gating. Rewording the prose would have worked once;
# stripping comments works for every doc comment anyone writes from now on.
strip_comments() {
    sed -E 's,^[[:space:]]*//.*,,; s,^[[:space:]]*\*.*,,'
}

extract_literal_keys() {
    for f in $(grep -rl --include='*.rs' -E 't\("' "${SRC_DIRS[@]}" 2>/dev/null || true); do
        strip_comments <"$f"
    done \
        | grep -oE 'dat0_i18n::t\("[^"]+"\)|[^a-zA-Z_]t\("[^"]+"\)' \
        | sed -E 's/.*t\("([^"]+)"\).*/\1/' || true
}

extract_const_list_keys() {
    # State machine: enter on the `pub const *_KEYS: &[&str]` declaration,
    # emit every string literal until the terminating `];`. Line comments are
    # stripped first so a `// see "foo"` inside the list is not mistaken for
    # a key.
    grep -rl --include='*.rs' -E 'const [A-Z0-9_]+_KEYS[[:space:]]*:[[:space:]]*&\[&str' \
        "${SRC_DIRS[@]}" 2>/dev/null \
        | xargs -r awk '
            /const [A-Z0-9_]+_KEYS[ \t]*:[ \t]*&\[&str/ { inlist = 1 }
            inlist {
                line = $0
                sub(/\/\/.*/, "", line)
                while (match(line, /"[^"]*"/)) {
                    print substr(line, RSTART + 1, RLENGTH - 2)
                    line = substr(line, RSTART + RLENGTH)
                }
                if ($0 ~ /\];/) { inlist = 0 }
            }
        ' || true
}

MISSING=0
MISSING_KEYS=()
while IFS= read -r key; do
    [ -n "$key" ] || continue
    if ! grep -qF "\"$key\":" "$CATALOG"; then
        echo "::error::i18n key referenced in crates/*/src but absent from ${CATALOG}: $key"
        MISSING_KEYS+=("$key")
        MISSING=$((MISSING + 1))
    fi
done < <( { extract_literal_keys; extract_const_list_keys; } | sort -u )

if [ "$MISSING" -gt 0 ]; then
    echo "i18n-check: FAILED — $MISSING referenced key(s) have no entry in $CATALOG"
    echo "Each of these renders as the raw key string to the user (t() echoes on a miss)."
    printf '  - %s\n' "${MISSING_KEYS[@]}"
    exit 1
fi

echo "i18n-check: all referenced keys resolve (pass 2, gate)"
exit 0
