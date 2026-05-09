#!/usr/bin/env bash
# Run `stryke test` on every exercism exercise, plus `--no-interop` syntax
# check on each solution file (catches Perl-isms like $a/$b, sub/say/reverse).
# Usage: from repo root —  bash examples/exercism_run_all.sh [--summary]
# Env:   ST=path/to/s   stryke binary (defaults to `s` on PATH, falls back to ./target/release/s)
#
# Each exercise lives at examples/exercism/<name>/ with t/test_*.stk inside.
# Tests `require "./Module.stk"`, so we cd into each before invoking `s test t`.
# Exit code: 0 if every exercise tests-green AND --no-interop-clean, 1 otherwise.

set -uo pipefail

SUMMARY_ONLY=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --summary) SUMMARY_ONLY=1; shift ;;
    -h|--help)
      sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    *) echo "exercism_run_all: unknown flag: $1" >&2; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EX_DIR="$ROOT/examples/exercism"

if [[ -n "${ST:-}" ]]; then
  :
elif command -v s >/dev/null 2>&1; then
  ST="s"
elif [[ -x "$ROOT/target/release/s" ]]; then
  ST="$ROOT/target/release/s"
else
  echo "exercism_run_all: no stryke binary found (set ST=, install \`s\`, or build target/release/s)" >&2
  exit 2
fi

pass=0
fail=0
failed_names=()

for d in "$EX_DIR"/*/; do
  [[ -d "$d/t" ]] || continue
  name="$(basename "$d")"
  ok=1
  err=""

  for sol in "$d"/*.stk; do
    [[ -f "$sol" ]] || continue
    if ! "$ST" --no-interop -c "$sol" >/dev/null 2>&1; then
      ok=0
      err="--no-interop"
      break
    fi
  done

  if [[ $ok -eq 1 ]] && ! (cd "$d" && "$ST" test t >/dev/null 2>&1); then
    ok=0
    err="test"
  fi

  if [[ $ok -eq 1 ]]; then
    pass=$((pass + 1))
    [[ $SUMMARY_ONLY -eq 1 ]] || printf "  OK   %s\n" "$name"
  else
    fail=$((fail + 1))
    failed_names+=("$name [$err]")
    printf "  FAIL %s [%s]\n" "$name" "$err" >&2
  fi
done

total=$((pass + fail))
printf "\nexercism: %d/%d passed (%d failed)\n" "$pass" "$total" "$fail"

if [[ $fail -gt 0 ]]; then
  printf "failed:\n" >&2
  printf "  %s\n" "${failed_names[@]}" >&2
  exit 1
fi
