#!/usr/bin/env zsh
# bump.sh — bump version, sync all Cargo.toml files, publish all crates
#
# Usage:
#   ./scripts/bump.sh patch    # 0.8.8 → 0.8.9
#   ./scripts/bump.sh minor    # 0.8.8 → 0.9.0
#   ./scripts/bump.sh major    # 0.8.8 → 1.0.0
#   ./scripts/bump.sh 1.2.3    # set exact version

set -e

ROOT="$(builtin cd "$(dirname "$0")/.." && pwd)"
builtin cd "$ROOT"

# ── parse current version ──
CURRENT=$(perl -ne 'if (/^version\s*=\s*"(\d+\.\d+\.\d+)"/) { print $1; exit }' Cargo.toml)
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

# ── update all version references ──
# root Cargo.toml: package version + workspace.package.version
perl -pi -e "s/^version = \"\\Q$CURRENT\\E\"/version = \"$NEW\"/" Cargo.toml

# compsys and zsh dep versions in root Cargo.toml
perl -pi -e "s/(compsys = \\{ path = \"compsys\", version = \")\\Q$CURRENT\\E/\${1}$NEW/" Cargo.toml
perl -pi -e "s/(zsh = \\{ path = \"zsh\", version = \")\\Q$CURRENT\\E/\${1}$NEW/" Cargo.toml

# zsh/Cargo.toml: compsys dep version
perl -pi -e "s/(compsys = \\{ path = \"\\.\\.\/compsys\", version = \")\\Q$CURRENT\\E/\${1}$NEW/" zsh/Cargo.toml

echo "  Cargo.toml package: $NEW"
echo "  workspace.package:  $NEW"
echo "  compsys dep:        $NEW"
echo "  zsh dep:            $NEW"
echo "  zsh→compsys dep:    $NEW"

# ── verify it builds ──
echo ""
echo "building..."
cargo build 2>&1 | grep -E '^error' && { echo "BUILD FAILED"; exit 1; }
echo "build ok"

# ── publish in order ──
echo ""
echo "publishing compsys v$NEW..."
cargo publish --allow-dirty -p compsys 2>&1 | tail -1

echo "publishing zsh v$NEW..."
cargo publish --allow-dirty -p zsh 2>&1 | tail -1

echo "publishing strykelang v$NEW..."
cargo publish --allow-dirty -p strykelang 2>&1 | tail -1

# ── install locally ──
echo ""
echo "installing locally..."
cargo install --path . 2>&1 | tail -1

echo ""
echo "done: strykelang v$NEW published and installed"
