#!/usr/bin/env bash
# Run parity/cpan_topn/smoke_all.pl under fo(1) with -I local/lib/perl5 (fo ignores PERL5LIB).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOCAL_LIB="$ROOT/parity/cpan_topn/local/lib/perl5"
VENDOR_PERL="$ROOT/vendor/perl"
FO="${FO:-$ROOT/target/release/fo}"
SMOKE="$ROOT/parity/cpan_topn/smoke_all.pl"

export LC_ALL=C
export LANG=C

if [[ ! -d "$LOCAL_LIB" ]]; then
  echo "cpan_topn: missing $LOCAL_LIB — run: bash parity/cpan_topn/install_deps.sh" >&2
  exit 2
fi

if [[ ! -x "$FO" ]]; then
  echo "cpan_topn: building release fo …" >&2
  (builtin cd "$ROOT" && cargo build --release --locked -q)
fi

if [[ ! -x "$FO" ]]; then
  echo "cpan_topn: no executable at FO=$FO" >&2
  exit 2
fi

# fo(1) does not read PERL5LIB. Put in-tree vendor/perl *before* local: CPAN may ship an XS
# `Sub/Util.pm` that breaks Try::Tiny; vendor stubs + native `Sub::Util::set_subname` must win.
echo "cpan_topn: FO=$FO -I $VENDOR_PERL -I $LOCAL_LIB …" >&2

"$FO" -I "$VENDOR_PERL" -I "$LOCAL_LIB" "$SMOKE"
echo "cpan_topn: smoke_all.pl OK" >&2
