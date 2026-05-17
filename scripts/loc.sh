#!/usr/bin/env zsh
# loc.sh — line-of-code stats for strykelang via tokei.
#
# Emits the headline numbers the report.html / README pull from, plus a
# stryke-only / test-only / .stk / parity / examples breakdown. Run from
# the repo root.
#
# Requires: tokei (https://github.com/XAMPPRocky/tokei).
#
# Output sections:
#   1. Production Rust under strykelang/ (excluding *_tests.rs)
#   2. Inline *_tests.rs under strykelang/
#   3. Integration tests under tests/
#   4. .stk corpus under examples/ + parity/ + tests/
#   5. Tooling: build.rs + benches/ + fuzz/
#   6. Builtin / test-fn counts from the running stryke binary
#   7. One-line summary for report.html copy-paste

set -e

if ! command -v tokei >/dev/null; then
  print -u2 "loc.sh: tokei not on PATH. brew install tokei"
  exit 1
fi

cd "${0:h}/.."

print "── 1. Production Rust under strykelang/ (excl. *_tests.rs) ──"
tokei strykelang -e '*_tests.rs'

print
print "── 2. Inline *_tests.rs under strykelang/ ──"
tokei $(find strykelang -name '*_tests.rs')

print
print "── 3. Integration tests under tests/ ──"
tokei tests

print
print "── 4. .stk corpus (examples/ + parity/ + tests/) ──"
STK_FILES=$(find examples parity tests -name '*.stk' 2>/dev/null | wc -l | tr -d ' ')
STK_LINES=$(find examples parity tests -name '*.stk' -exec cat {} + 2>/dev/null | wc -l | tr -d ' ')
print "  files: $STK_FILES"
print "  lines: $STK_LINES"
ROSETTA_FILES=$(find examples/rosetta -name '*.stk' 2>/dev/null | wc -l | tr -d ' ')
NOINTEROP_FILES=$(ls examples/*_no_interop.stk 2>/dev/null | wc -l | tr -d ' ')
print "  rosetta cases: $ROSETTA_FILES"
print "  no-interop demos: $NOINTEROP_FILES"

print
print "── 5. Tooling (build.rs + benches/ + fuzz/) ──"
tokei build.rs benches fuzz

print
print "── 6. Runtime reflection (requires ./target/debug/stryke) ──"
if [[ -x ./target/debug/stryke ]]; then
  ./target/debug/stryke --no-interop -e '
    p "primaries:  " . len(keys %b)
    p "aliases:    " . len(keys %a)
    p "keywords:   " . len(keys %k)
    p "all keys:   " . len(keys %all)
  '
else
  print -u2 "  (./target/debug/stryke not built — run \`cargo build\` first)"
fi

print
print "── 7. Test-fn count (#[test] across tests/ + strykelang/) ──"
TEST_FNS=$(grep -rE '^\s*#\[(test|tokio::test)\]' tests strykelang 2>/dev/null | wc -l | tr -d ' ')
print "  $TEST_FNS"

print
print "── 8. Headline summary (report.html copy-paste) ──"
PROD=$(find strykelang -name '*.rs' ! -name '*_tests.rs' -exec cat {} + | wc -l | tr -d ' ')
TESTS_DIR=$(find tests -name '*.rs' -exec cat {} + | wc -l | tr -d ' ')
TOOLING_LINES=$(( $(wc -l < build.rs) + $(find benches -name '*.rs' -exec cat {} + | wc -l) + $(find fuzz -name '*.rs' -exec cat {} + | wc -l) ))
STK=$(find examples parity tests -name '*.stk' -exec cat {} + 2>/dev/null | wc -l | tr -d ' ')
TOTAL=$(( PROD + TESTS_DIR + TOOLING_LINES ))
COMMITS=$(git rev-list --count HEAD)
PCT=$(( 100 * PROD / TOTAL ))
printf "  prod (strykelang/, excl tests):  %s\n" "$PROD"
printf "  tests/ dir:                      %s\n" "$TESTS_DIR"
printf "  tooling (build.rs/benches/fuzz): %s\n" "$TOOLING_LINES"
printf "  total Rust:                      %s  (prod %s%%)\n" "$TOTAL" "$PCT"
printf "  .stk corpus:                     %s\n" "$STK"
printf "  git commits:                     %s\n" "$COMMITS"
