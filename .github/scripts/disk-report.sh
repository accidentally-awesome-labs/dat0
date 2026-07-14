#!/usr/bin/env bash
# Compact, greppable disk snapshot for CI.
#
# WHY: the hosted Linux runner has intermittently died with
#   System.IO.IOException: No space left on device
# — the runner PROCESS itself, mid-step — which reddened main (b55758b) and
# blocked PRs (#51). Every diagnosis was GUESSWORK because we had no disk
# telemetry: hypotheses about debuginfo, the release tree, and a "~100 GB debug
# build" were all wrong. A clean `cargo test --workspace --no-run` is only ~23 GB,
# and the one run we finally measured had 107 GB free — so the failures look
# environmental (runner-image disk varies run to run), not build-driven.
#
# This prints one line per phase so the LOW-WATER MARK is visible on every run,
# including a failed one, and comparable across runs:
#   DISK[after-test] size=145G used=61G avail=84G use=42% target=23G
#
# Never fails the job — instrumentation must not become a new failure mode.
phase="${1:-unknown}"
triple="${2:-}"

line=$(df -h / 2>/dev/null | awk 'NR==2') || line=""
size=$(awk '{print $2}' <<<"$line"); used=$(awk '{print $3}' <<<"$line")
avail=$(awk '{print $4}' <<<"$line"); pct=$(awk '{print $5}' <<<"$line")

tgt="-"
if [ -n "$triple" ] && [ -d "target/$triple" ]; then
  tgt=$(du -sh "target/$triple" 2>/dev/null | cut -f1) || tgt="?"
fi

echo "DISK[$phase] size=${size:-?} used=${used:-?} avail=${avail:-?} use=${pct:-?} target=${tgt}"

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  printf '| %s | %s | %s | %s |\n' "$phase" "${avail:-?}" "${used:-?}" "$tgt" >> "$GITHUB_STEP_SUMMARY"
fi
exit 0
