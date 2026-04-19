#!/usr/bin/env bash
# Run parity/cpan_topn/smoke_all.pl under stryke(1) with -I local/lib/perl5 (stryke ignores PERL5LIB).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOCAL_LIB="$ROOT/parity/cpan_topn/local/lib/perl5"
VENDOR_PERL="$ROOT/vendor/perl"
STRYKE="${STRYKE:-$ROOT/target/release/stryke}"
SMOKE="$ROOT/parity/cpan_topn/smoke_all.pl"

export LC_ALL=C
export LANG=C

if [[ ! -d "$LOCAL_LIB" ]]; then
  echo "cpan_topn: missing $LOCAL_LIB — run: bash parity/cpan_topn/install_deps.sh" >&2
  exit 2
fi

if [[ ! -x "$STRYKE" ]]; then
  echo "cpan_topn: building release stryke …" >&2
  (builtin cd "$ROOT" && cargo build --release --locked -q)
fi

if [[ ! -x "$STRYKE" ]]; then
  echo "cpan_topn: no executable at STRYKE=$STRYKE" >&2
  exit 2
fi

# stryke(1) does not read PERL5LIB. Put in-tree vendor/perl *before* local: CPAN may ship an XS
# `Sub/Util.pm` that breaks Try::Tiny; vendor stubs + native `Sub::Util::set_subname` must win.
echo "cpan_topn: STRYKE=$STRYKE -I $VENDOR_PERL -I $LOCAL_LIB …" >&2

"$STRYKE" -I "$VENDOR_PERL" -I "$LOCAL_LIB" "$SMOKE"
echo "cpan_topn: smoke_all.pl OK" >&2
