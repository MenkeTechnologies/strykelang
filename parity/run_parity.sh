#!/usr/bin/env bash
# Compare stock perl(1) vs fo(1) on parity/cases/*.pl (exact stdout+stderr bytes).
# Usage: from repo root —  bash parity/run_parity.sh [--summary] [--json OUT] [--fail-log PATH]
# Env:   PERL=perl  FO=path/to/fo  (optional)
#
# Flags:
#   --summary            Suppress per-case OK/FAIL lines on stdout; the totals
#                        line still prints. Short `parity FAIL: NAME` still
#                        goes to stderr so progress is visible.
#   --json PATH          Write a JSON summary to PATH
#                        (default: parity/parity_summary.json). Committable:
#                        `gen-docs` reads it to stamp the hub's parity badge.
#   --fail-log PATH      Write per-case failure details (both outputs + diff)
#                        to PATH. Use `-` to emit to stderr (pre-flag
#                        behavior). Default: parity/parity_failures.log.
#                        The file is truncated at the start of each run.

set -euo pipefail

SUMMARY_ONLY=0
JSON_OUT=""
FAIL_LOG=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --summary)      SUMMARY_ONLY=1; shift ;;
    --json)         JSON_OUT="${2:-}"; shift 2 ;;
    --json=*)       JSON_OUT="${1#--json=}"; shift ;;
    --fail-log)     FAIL_LOG="${2:-}"; shift 2 ;;
    --fail-log=*)   FAIL_LOG="${1#--fail-log=}"; shift ;;
    *) echo "parity: unknown flag: $1" >&2; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export LC_ALL=C
export LANG=C

PERL="${PERL:-perl}"
FO="${FO:-$ROOT/target/release/fo}"

if ! command -v "$PERL" >/dev/null 2>&1; then
  echo "parity: '$PERL' not found on PATH" >&2
  exit 2
fi

if [[ ! -x "$FO" ]]; then
  echo "parity: building release fo (cargo build --release)…" >&2
  (builtin cd "$ROOT" && cargo build --release --locked -q)
fi

if [[ ! -x "$FO" ]]; then
  echo "parity: no executable at FO=$FO" >&2
  exit 2
fi

shopt -s nullglob
cases=("$ROOT"/parity/cases/*.pl)
if [[ ${#cases[@]} -eq 0 ]]; then
  echo "parity: no scripts in parity/cases/*.pl" >&2
  exit 2
fi

# Failure log destination: `-` = stderr (pre-flag behavior); empty = default
# path `parity/parity_failures.log`; anything else = that path. Truncate at
# run-start so each run produces a clean, self-contained log.
if [[ -z "$FAIL_LOG" ]]; then
  FAIL_LOG="$ROOT/parity/parity_failures.log"
fi
if [[ "$FAIL_LOG" = "-" ]]; then
  exec 7>&2
else
  : >"$FAIL_LOG"
  exec 7>"$FAIL_LOG"
fi

total="${#cases[@]}"
passed=0
failed=0
for f in "${cases[@]}"; do
  base=$(basename "$f")
  p_out=$(mktemp "${TMPDIR:-/tmp}/parity.pl.$$.XXXXXX")
  r_out=$(mktemp "${TMPDIR:-/tmp}/parity.fo.$$.XXXXXX")

  "$PERL" "$f" >"$p_out" 2>&1 || true
  "$FO" --compat "$f" >"$r_out" 2>&1 || true

  if ! cmp -s "$p_out" "$r_out"; then
    # Short progress line always hits stderr so the user sees forward motion.
    echo "parity FAIL: $base" >&2
    # Full details → fail-log stream (fd 7).
    {
      echo "==== $base ===="
      echo "--- perl $base ---"
      command cat "$p_out"
      echo "--- fo $base ---"
      command cat "$r_out"
      echo "--- diff (perl vs fo) ---"
      diff -u "$p_out" "$r_out" || true
      echo
    } >&7
    failed=$((failed + 1))
  else
    [[ "$SUMMARY_ONLY" -eq 0 ]] && echo "parity OK:   $base"
    passed=$((passed + 1))
  fi

  command rm -f "$p_out" "$r_out"
done

# Close the fail-log fd so summaries can't interleave into it.
exec 7>&-

# Percent to two decimals via awk (BSD /bin/sh doesn't do floats).
pct=$(awk -v p="$passed" -v t="$total" 'BEGIN{ if (t==0) print "0.00"; else printf "%.2f", 100*p/t }')

version=$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$ROOT/Cargo.toml")
version="${version:-unknown}"
generated=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# Emit totals line on stdout — always, regardless of `--summary`.
printf 'parity: %d/%d passed (%s%%) · failed %d · forge v%s\n' \
  "$passed" "$total" "$pct" "$failed" "$version"

# Point the user at the failure log when there were any and it's not stderr.
if [[ "$failed" -gt 0 && "$FAIL_LOG" != "-" ]]; then
  echo "parity: failure details in $FAIL_LOG" >&2
fi

# Optional JSON summary — only written when explicitly requested via
# `--json PATH`. Not auto-generated: exit code is the binary pass/fail
# signal, the log file has the triage detail, so a third artifact is noise.
if [[ -n "$JSON_OUT" ]]; then
  tmp_json=$(mktemp "${TMPDIR:-/tmp}/parity.summary.$$.XXXXXX")
  printf '{\n  "total": %d,\n  "passed": %d,\n  "failed": %d,\n  "percent": %s,\n  "forge_version": "%s",\n  "generated_at": "%s"\n}\n' \
    "$total" "$passed" "$failed" "$pct" "$version" "$generated" >"$tmp_json"
  command mv "$tmp_json" "$JSON_OUT"
fi

if [[ "$failed" -ne 0 ]]; then
  exit 1
fi
