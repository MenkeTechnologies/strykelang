#!/usr/bin/env bash
# Compare stock perl(1) vs pe(1) on parity/cases/*.pl (exact stdout+stderr bytes).
# Usage: from repo root —  bash parity/run_parity.sh
# Env: PERL=perl  PE=path/to/pe  (optional)

set -euo pipefail

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

failed=0
for f in "${cases[@]}"; do
  base=$(basename "$f")
  p_out=$(mktemp "${TMPDIR:-/tmp}/parity.pl.$$.XXXXXX")
  r_out=$(mktemp "${TMPDIR:-/tmp}/parity.pe.$$.XXXXXX")

  "$PERL" "$f" >"$p_out" 2>&1 || true
  "$PE" "$f" >"$r_out" 2>&1 || true

  if ! cmp -s "$p_out" "$r_out"; then
    echo "parity FAIL: $base" >&2
    echo "--- perl $base ---" >&2
    command cat "$p_out" >&2
    echo "--- pe $base ---" >&2
    command cat "$r_out" >&2
    echo "--- diff (perl vs pe) ---" >&2
    diff -u "$p_out" "$r_out" >&2 || true
    failed=1
  else
    echo "parity OK:   $base"
  fi

  command rm -f "$p_out" "$r_out"
done

if [[ "$failed" -ne 0 ]]; then
  echo "parity: $failed case(s) mismatch stock perl" >&2
  exit 1
fi
echo "parity: all ${#cases[@]} case(s) match perl output"
