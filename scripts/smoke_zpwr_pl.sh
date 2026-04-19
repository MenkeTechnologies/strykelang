#!/usr/bin/env bash
# Smoke-run ~/.zpwr/scripts/*.pl through stryke (bounded time / non-interactive inputs).
# Resolves `stryke`: target/debug/stryke, then target/release/stryke (run `cargo build --bin stryke`).
set -uo pipefail
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
SCRIPTS="${ZPWR_SCRIPTS:-$HOME/.zpwr/scripts}"

resolve_fo() {
  if [[ -n "${STRYKE:-}" && -x "$STRYKE" ]]; then
    return 0
  fi
  if [[ -x "$REPO_ROOT/target/debug/stryke" ]]; then
    STRYKE=$REPO_ROOT/target/debug/stryke
    return 0
  fi
  if [[ -x "$REPO_ROOT/target/release/stryke" ]]; then
    STRYKE=$REPO_ROOT/target/release/stryke
    return 0
  fi
  echo "No stryke binary: run 'cargo build --bin stryke' in $REPO_ROOT" >&2
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
run "escapeRemover.pl" timeout 5 "$STRYKE" "$SCRIPTS/escapeRemover.pl" \
  <<<"$(printf '\x1b]0;title\x07\n\x1b[31mhi\x1b[0m\n')"

# 2–3: Getopt + box — `--` so stryke does not swallow -h (script exits 1 from usage())
run "boxPrint.pl (-h)" bash -c 'timeout 5 "$1" -- "$2" -h; e=$?; exit $(( e == 0 || e == 1 ? 0 : e ))' _ "$STRYKE" "$SCRIPTS/boxPrint.pl"

run "boxPrint.pl (stdin)" timeout 5 "$STRYKE" "$SCRIPTS/boxPrint.pl" <<<"hello"

# 4: no argv → no interactive replace prompt
run "regexReplace.pl (no args)" timeout 5 "$STRYKE" "$SCRIPTS/regexReplace.pl"

# 5: in-place minify on one file
run "minifySpaces.pl" timeout 5 "$STRYKE" "$SCRIPTS/minifySpaces.pl" "$TMP/sp.txt"

# 6: no file args → empty for-loop, close less
run "c.pl (no argv)" timeout 8 "$STRYKE" "$SCRIPTS/c.pl"

# 7: stdin + less (bounded)
run "stdinSdiffColorizer.pl" timeout 8 "$STRYKE" "$SCRIPTS/stdinSdiffColorizer.pl" 72 <<<"left | right"

# 8: Two-arg `open $fh, "sdiff … |"` — pipe-from-command (sh -c). Discard script output only
# (not the run() label/OK lines).
run "sdiffColorizer.pl" bash -c 'timeout 10 "$@" >/dev/null' _ "$STRYKE" "$SCRIPTS/sdiffColorizer.pl" \
  "$TMP/sd1" "$TMP/sd2"

# 9: Opens `git difftool … |` (may block or exit non-zero without a clean tree); syntax + start only.
run "gitSdiffColorizer.pl (-c)" timeout 5 "$STRYKE" -c "$SCRIPTS/gitSdiffColorizer.pl"

# 10: banner (external date + figlet — may warn if a helper is missing)
run "banner.pl" timeout 25 "$STRYKE" "$SCRIPTS/banner.pl"

command rm -rf "$TMP"
exit "$failed"
