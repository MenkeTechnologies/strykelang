#!/usr/bin/env bash
# Compare stock perl(1) vs pe(1) on parity/cases/*.pl (exact stdout+stderr bytes).
# Usage: from repo root —  bash parity/run_parity.sh [--summary] [--json OUT]
# Env:   PERL=perl  PE=path/to/pe  (optional)
#
# Flags:
#   --summary        Suppress per-case OK/FAIL lines; print the totals line only.
#                    Failed cases still emit their diff to stderr so `--summary
#                    2>/dev/null` gives a clean single-line stdout.
#   --json PATH      Write a JSON summary to PATH (default: parity/parity_summary.json).
#                    Committable: the docs site generator (`gen-docs`) reads it
#                    to stamp the parity badge on docs/index.html.

set -euo pipefail

SUMMARY_ONLY=0
JSON_OUT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --summary) SUMMARY_ONLY=1; shift ;;
    --json)    JSON_OUT="${2:-}"; shift 2 ;;
    --json=*)  JSON_OUT="${1#--json=}"; shift ;;
    *) echo "parity: unknown flag: $1" >&2; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export LC_ALL=C
export LANG=C

PERL="${PERL:-perl}"
PE="${PE:-$ROOT/target/release/pe}"

if ! command -v "$PERL" >/dev/null 2>&1; then
  echo "parity: '$PERL' not found on PATH" >&2
  exit 2
fi

if [[ ! -x "$PE" ]]; then
  echo "parity: building release pe (cargo build --release)…" >&2
  (builtin cd "$ROOT" && cargo build --release --locked -q)
fi

if [[ ! -x "$PE" ]]; then
  echo "parity: no executable at PE=$PE" >&2
  exit 2
fi

shopt -s nullglob
cases=("$ROOT"/parity/cases/*.pl)
if [[ ${#cases[@]} -eq 0 ]]; then
  echo "parity: no scripts in parity/cases/*.pl" >&2
  exit 2
fi

total="${#cases[@]}"
passed=0
failed=0
for f in "${cases[@]}"; do
  base=$(basename "$f")
  p_out=$(mktemp "${TMPDIR:-/tmp}/parity.pl.$$.XXXXXX")
  r_out=$(mktemp "${TMPDIR:-/tmp}/parity.pe.$$.XXXXXX")

  "$PERL" "$f" >"$p_out" 2>&1 || true
  "$PE" --compat "$f" >"$r_out" 2>&1 || true

  if ! cmp -s "$p_out" "$r_out"; then
    echo "parity FAIL: $base" >&2
    echo "--- perl $base ---" >&2
    command cat "$p_out" >&2
    echo "--- pe $base ---" >&2
    command cat "$r_out" >&2
    echo "--- diff (perl vs pe) ---" >&2
    diff -u "$p_out" "$r_out" >&2 || true
    failed=$((failed + 1))
  else
    [[ "$SUMMARY_ONLY" -eq 0 ]] && echo "parity OK:   $base"
    passed=$((passed + 1))
  fi

  command rm -f "$p_out" "$r_out"
done

# Percent to two decimals via awk (BSD /bin/sh doesn't do floats).
pct=$(awk -v p="$passed" -v t="$total" 'BEGIN{ if (t==0) print "0.00"; else printf "%.2f", 100*p/t }')

version=$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$ROOT/Cargo.toml")
version="${version:-unknown}"
generated=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# Emit totals line on stdout — always, regardless of `--summary`.
printf 'parity: %d/%d passed (%s%%) · failed %d · perlrs v%s\n' \
  "$passed" "$total" "$pct" "$failed" "$version"

# Write JSON summary (committable; `gen-docs` reads this to stamp docs).
if [[ -z "$JSON_OUT" ]]; then
  JSON_OUT="$ROOT/parity/parity_summary.json"
fi
tmp_json=$(mktemp "${TMPDIR:-/tmp}/parity.summary.$$.XXXXXX")
printf '{\n  "total": %d,\n  "passed": %d,\n  "failed": %d,\n  "percent": %s,\n  "perlrs_version": "%s",\n  "generated_at": "%s"\n}\n' \
  "$total" "$passed" "$failed" "$pct" "$version" "$generated" >"$tmp_json"
command mv "$tmp_json" "$JSON_OUT"

if [[ "$failed" -ne 0 ]]; then
  exit 1
fi
