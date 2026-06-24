#!/usr/bin/env zsh
# bump.sh — bump the strykelang version everywhere, commit, tag, push, publish.
#
# Usage:
#   ./scripts/bump.sh patch    # 0.17.22 → 0.17.23
#   ./scripts/bump.sh minor    # 0.17.22 → 0.18.0
#   ./scripts/bump.sh major    # 0.17.22 → 1.0.0
#   ./scripts/bump.sh 1.2.3    # set exact version
#
# Version strings live in six tracked files (this is the full set the
# `bump vX.Y.Z` commits touch). Cargo.lock is synced by the verify build,
# not hand-edited. Pushing the `vX.Y.Z` tag triggers .github/workflows/
# release.yml (GitHub Release + cross-platform binaries); crates.io is the
# `cargo publish` step here. strykelang is the only published crate — the
# old compsys/zsh sub-crates are gone.

set -e

ROOT="$(builtin cd "$(dirname "$0")/.." && pwd)"
builtin cd "$ROOT"

# ── parse current version (package version is the first `version = ` line) ──
CURRENT=$(perl -ne 'if (/^version\s*=\s*"(\d+\.\d+\.\d+)"/) { print $1; exit }' Cargo.toml)
if [[ -z "$CURRENT" ]]; then
  echo "could not read current version from Cargo.toml" >&2
  exit 1
fi
MAJOR=${CURRENT%%.*}
_rest=${CURRENT#*.}
MINOR=${_rest%%.*}
PATCH=${_rest#*.}

# ── compute new version ──
case "${1:-patch}" in
  patch) PATCH=$((PATCH + 1)) ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  [0-9]*.[0-9]*.[0-9]*)
    MAJOR=${1%%.*}
    _rest=${1#*.}
    MINOR=${_rest%%.*}
    PATCH=${_rest#*.}
    ;;
  *) echo "usage: bump.sh [patch|minor|major|X.Y.Z]"; exit 1 ;;
esac

NEW="${MAJOR}.${MINOR}.${PATCH}"
echo "bumping $CURRENT → $NEW"

# ── update version strings ──
# Cargo.toml: the anchored package `version` line only (leaves the unrelated
# [workspace.package] version untouched).
perl -pi -e "s/^version = \"\\Q$CURRENT\\E\"/version = \"$NEW\"/" Cargo.toml

# Docs build-lines (`stryke vX.Y.Z · …`) and man-page titles (`stryke X.Y.Z`).
# Each file carries exactly one occurrence of the exact version triple.
DOC_FILES=(docs/index.html docs/reference.html man/man1/stryke.1 man/man1/strykeall.1)
for f in $DOC_FILES; do
  perl -pi -e "s/\\Q$CURRENT\\E/$NEW/g" "$f"
done

echo "  Cargo.toml:          $NEW"
echo "  docs/index.html:     $NEW"
echo "  docs/reference.html: $NEW"
echo "  man/man1/stryke.1:   $NEW"
echo "  man/man1/strykeall.1: $NEW"

# ── verify build (also rewrites Cargo.lock to the new version) ──
echo ""
echo "building..."
cargo build || { echo "BUILD FAILED"; exit 1; }
echo "build ok"

# ── commit, tag, push (the tag push triggers release.yml) ──
echo ""
echo "committing + tagging v$NEW..."
# `-f` (force): docs/index.html and docs/reference.html are tracked but live
# under `docs/`, which carries .gitignore entries (docs/.fonts/, docs/book.*,
# *.tex/*.pdf). Without -f, `git add docs/...` prints "paths are ignored" and
# exits non-zero, which `set -e` turns into an abort before the commit.
git add -f Cargo.toml Cargo.lock $DOC_FILES
git commit -m "bump v$NEW"
git tag "v$NEW"
git push origin HEAD
git push origin "v$NEW"

# ── publish to crates.io (strykelang only) ──
echo ""
echo "publishing strykelang v$NEW to crates.io..."
cargo publish

# ── install locally ──
echo ""
echo "installing locally..."
cargo install --path . --force

echo ""
echo "done: strykelang v$NEW committed, tagged, pushed, published, installed"
