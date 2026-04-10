#!/usr/bin/env bash
# Smoke-run ~/.zpwr/scripts/*.pl through pe (bounded time / non-interactive inputs).
# Resolves `pe`: target/debug/pe, then target/release/pe (run `cargo build --bin pe`).
set -uo pipefail
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
SCRIPTS="${ZPWR_SCRIPTS:-$HOME/.zpwr/scripts}"

resolve_pe() {
  if [[ -n "${PE:-}" && -x "$PE" ]]; then
    return 0
  fi
  if [[ -x "$REPO_ROOT/target/debug/pe" ]]; then
    PE=$REPO_ROOT/target/debug/pe
    return 0
  fi
  if [[ -x "$REPO_ROOT/target/release/pe" ]]; then
    PE=$REPO_ROOT/target/release/pe
    return 0
  fi
  echo "No pe binary: run 'cargo build --bin pe' in $REPO_ROOT" >&2
  return 1
}

resolve_pe || exit 2

TMP=$(command mktemp -d)
printf 'line with trailing spaces   \n' >"$TMP/sp.txt"
printf 'a\n' >"$TMP/sd1"
printf 'b\n' >"$TMP/sd2"

failed=0
run() {
  local name=$1
  shift
  printf '%-34s ' "$name"
  if "$@"; then
    echo OK
  else
    echo "FAIL (exit $?)"
    failed=1
  fi
}

export COLUMNS="${COLUMNS:-80}"

# 1: filter stdin (escape sequences)
run "escapeRemover.pl" timeout 5 "$PE" "$SCRIPTS/escapeRemover.pl" \
  <<<"$(printf '\x1b]0;title\x07\n\x1b[31mhi\x1b[0m\n')"

# 2–3: Getopt + box — `--` so pe does not swallow -h (script exits 1 from usage())
run "boxPrint.pl (-h)" bash -c 'timeout 5 "$1" -- "$2" -h; e=$?; exit $(( e == 0 || e == 1 ? 0 : e ))' _ "$PE" "$SCRIPTS/boxPrint.pl"

run "boxPrint.pl (stdin)" timeout 5 "$PE" "$SCRIPTS/boxPrint.pl" <<<"hello"

# 4: no argv → no interactive replace prompt
run "regexReplace.pl (no args)" timeout 5 "$PE" "$SCRIPTS/regexReplace.pl"

# 5: in-place minify on one file
run "minifySpaces.pl" timeout 5 "$PE" "$SCRIPTS/minifySpaces.pl" "$TMP/sp.txt"

# 6: no file args → empty for-loop, close less
run "c.pl (no argv)" timeout 8 "$PE" "$SCRIPTS/c.pl"

# 7: stdin + less (bounded)
run "stdinSdiffColorizer.pl" timeout 8 "$PE" "$SCRIPTS/stdinSdiffColorizer.pl" 72 <<<"left | right"

# 8–9: These use Perl two-arg open with a shell pipeline string; runtime needs
# native pipe open. Parse-check only (same scripts run under system perl with a real shell).
run "sdiffColorizer.pl (-c)" timeout 5 "$PE" -c "$SCRIPTS/sdiffColorizer.pl"

run "gitSdiffColorizer.pl (-c)" timeout 5 "$PE" -c "$SCRIPTS/gitSdiffColorizer.pl"

# 10: banner (external date + figlet — may warn if a helper is missing)
run "banner.pl" timeout 25 "$PE" "$SCRIPTS/banner.pl"

command rm -rf "$TMP"
exit "$failed"
