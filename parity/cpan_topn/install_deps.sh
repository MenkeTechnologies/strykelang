#!/usr/bin/env bash
# Install MODULES.txt into parity/cpan_topn/local/ using cpanm (pure-Perl deps).
set -euo pipefail

# System cpanm runs stock perl; a repo PERL5LIB (e.g. vendor/perl stubs) breaks it.
unset PERL5LIB PERL5OPT || true

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOCAL="$ROOT/parity/cpan_topn/local"
MODULES_FILE="$ROOT/parity/cpan_topn/MODULES.txt"

if ! command -v cpanm >/dev/null 2>&1; then
  echo "cpan_topn: cpanm not on PATH. Install: sudo apt install cpanminus   or   curl -L https://cpanmin.us | perl - App::cpanminus" >&2
  exit 2
fi

mapfile -t MODS < <(grep -v '^[[:space:]]*#' "$MODULES_FILE" | grep -v '^[[:space:]]*$' || true)
if [[ ${#MODS[@]} -eq 0 ]]; then
  echo "cpan_topn: no modules in $MODULES_FILE" >&2
  exit 2
fi

echo "cpan_topn: installing ${#MODS[@]} module(s) into $LOCAL …" >&2
mkdir -p "$LOCAL"
cpanm --local-lib-contained "$LOCAL" --notest "${MODS[@]}"
# Core-adjacent modules are often "already up to date" without a copy under local/; then
# vendor/perl stubs (e.g. Getopt::Long) win on @INC. Force a real tree for these.
for force_mod in Getopt::Long Text::Tabs; do
  echo "cpan_topn: reinstall $force_mod into local …" >&2
  cpanm --local-lib-contained "$LOCAL" --notest --reinstall "$force_mod"
done
echo "cpan_topn: done." >&2
