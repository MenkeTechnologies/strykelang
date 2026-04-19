#!/usr/bin/env bash
# Smoke-run ~/.zpwr/scripts/*.pl through fo (bounded time / non-interactive inputs).
# Resolves `fo`: target/debug/fo, then target/release/fo (run `cargo build --bin fo`).
set -uo pipefail
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
SCRIPTS="${ZPWR_SCRIPTS:-$HOME/.zpwr/scripts}"

resolve_fo() {
  if [[ -n "${FO:-}" && -x "$FO" ]]; then
    return 0
  fi
  if [[ -x "$REPO_ROOT/target/debug/fo" ]]; then
    FO=$REPO_ROOT/target/debug/fo
    return 0
  fi
  if [[ -x "$REPO_ROOT/target/release/fo" ]]; then
    FO=$REPO_ROOT/target/release/fo
    return 0
  fi
  echo "No fo binary: run 'cargo build --bin fo' in $REPO_ROOT" >&2
  return 1
}

resolve_fo || exit 2

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
run "escapeRemover.pl" timeout 5 "$FO" "$SCRIPTS/escapeRemover.pl" \
  <<<"$(printf '\x1b]0;title\x07\n\x1b[31mhi\x1b[0m\n')"

# 2–3: Getopt + box — `--` so fo does not swallow -h (script exits 1 from usage())
run "boxPrint.pl (-h)" bash -c 'timeout 5 "$1" -- "$2" -h; e=$?; exit $(( e == 0 || e == 1 ? 0 : e ))' _ "$FO" "$SCRIPTS/boxPrint.pl"

run "boxPrint.pl (stdin)" timeout 5 "$FO" "$SCRIPTS/boxPrint.pl" <<<"hello"

# 4: no argv → no interactive replace prompt
run "regexReplace.pl (no args)" timeout 5 "$FO" "$SCRIPTS/regexReplace.pl"

# 5: in-place minify on one file
run "minifySpaces.pl" timeout 5 "$FO" "$SCRIPTS/minifySpaces.pl" "$TMP/sp.txt"

# 6: no file args → empty for-loop, close less
run "c.pl (no argv)" timeout 8 "$FO" "$SCRIPTS/c.pl"

# 7: stdin + less (bounded)
run "stdinSdiffColorizer.pl" timeout 8 "$FO" "$SCRIPTS/stdinSdiffColorizer.pl" 72 <<<"left | right"

# 8: Two-arg `open $fh, "sdiff … |"` — pipe-from-command (sh -c). Discard script output only
# (not the run() label/OK lines).
run "sdiffColorizer.pl" bash -c 'timeout 10 "$@" >/dev/null' _ "$FO" "$SCRIPTS/sdiffColorizer.pl" \
  "$TMP/sd1" "$TMP/sd2"

# 9: Opens `git difftool … |` (may block or exit non-zero without a clean tree); syntax + start only.
run "gitSdiffColorizer.pl (-c)" timeout 5 "$FO" -c "$SCRIPTS/gitSdiffColorizer.pl"

# 10: banner (external date + figlet — may warn if a helper is missing)
run "banner.pl" timeout 25 "$FO" "$SCRIPTS/banner.pl"

command rm -rf "$TMP"
exit "$failed"
