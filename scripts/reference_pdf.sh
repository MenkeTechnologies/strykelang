#!/usr/bin/env bash
set -euo pipefail

# Convert docs/reference.html → docs/reference.tex → docs/reference.pdf via
# pandoc. No browser rendering — pandoc reads the semantic HTML (sections,
# headings, code blocks, lists) and emits a paginated LaTeX document with a
# table of contents.
#
# Requires: pandoc + xelatex (or pdflatex). Refreshes the HTML first so the
# PDF tracks the current builtin/keyword corpus.

ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
HTML=$ROOT/docs/reference.html
TEX=$ROOT/docs/reference.tex
PDF=$ROOT/docs/reference.pdf

command -v pandoc >/dev/null || { echo "pandoc not installed (brew install pandoc)" >&2; exit 1; }

if command -v xelatex >/dev/null; then
    ENGINE=xelatex
elif command -v pdflatex >/dev/null; then
    ENGINE=pdflatex
else
    echo "need xelatex or pdflatex (brew install --cask mactex-no-gui)" >&2
    exit 1
fi

echo "regenerating $HTML"
(cd "$ROOT" && cargo run --quiet --bin gen-docs)
[[ -f $HTML ]] || { echo "missing: $HTML" >&2; exit 1; }

# Extract just the <main>…</main> body so the LaTeX side doesn't see the
# header, theme buttons, or scheme strip — those are HTML chrome with no
# print equivalent.
BODY=$(mktemp -t refpdf.XXXXXX.html)
awk 'BEGIN{p=0} /<main[ >]/{p=1} p{print} /<\/main>/{p=0}' "$HTML" >"$BODY"
[[ -s $BODY ]] || { echo "could not extract <main> from $HTML" >&2; exit 1; }

# Strip print-irrelevant chrome from the body before pandoc sees it:
#   1. `<a class="doc-anchor" href="#doc-…">#</a>` — HTML hover affordance.
#   2. `<h2 class="tutorial-title">…</h2>` — duplicates the cover page title.
#   3. `<p class="tutorial-subtitle">…</p>` — page intro, redundant in print.
#   4. The whole `<section class="tutorial-section">` whose `<h2>` is
#      "Chapters" — its chapter list duplicates the TOC.
# Portable across BSD/GNU sed by writing to a sibling tempfile.
CLEAN=$(mktemp -t refpdf.XXXXXX.html)
trap 'command rm -f "$BODY" "$CLEAN" "$THEME_RESOLVED"' EXIT
command sed -E \
    -e 's|<a class="doc-anchor"[^>]*>#</a>||g' \
    -e '/<h2 class="tutorial-title"/d' \
    -e '/<p class="tutorial-subtitle"/d' \
    "$BODY" \
| awk '
    # Drop the unique <section class="tutorial-section"> that has no id —
    # it is the Chapters index, redundant with the LaTeX TOC. All real
    # chapters carry id="ch-…" so they pass through.
    /<section class="tutorial-section">$/ { in_idx=1; next }
    in_idx && /<\/section>/             { in_idx=0; next }
    in_idx                               { next }
    { print }
' >"$CLEAN"
BODY=$CLEAN

VERSION=$(awk -F'"' '/^version/ {print $2; exit}' "$ROOT/Cargo.toml")
DATE=$(date -u +%Y-%m-%d)

# Cyberpunk theme fonts — Orbitron (variable, wght axis) for headings,
# Share Tech Mono for code. Mirrors docs/index.html. Cached in docs/.fonts/
# (gitignored). Both are SIL OFL 1.1 from the Google Fonts upstream repo.
FONT_DIR=$ROOT/docs/.fonts
command mkdir -p "$FONT_DIR"
if [[ ! -s $FONT_DIR/Orbitron-VF.ttf ]]; then
    echo "downloading Orbitron-VF.ttf"
    curl -fsSLo "$FONT_DIR/Orbitron-VF.ttf" \
        'https://github.com/google/fonts/raw/main/ofl/orbitron/Orbitron%5Bwght%5D.ttf'
fi
if [[ ! -s $FONT_DIR/ShareTechMono-Regular.ttf ]]; then
    echo "downloading ShareTechMono-Regular.ttf"
    curl -fsSLo "$FONT_DIR/ShareTechMono-Regular.ttf" \
        'https://github.com/google/fonts/raw/main/ofl/sharetechmono/ShareTechMono-Regular.ttf'
fi

# STIX Two Text body font — Greek/math coverage, available on macOS.
# Falls back silently if missing; xelatex will warn and use Latin Modern.
FONT_ARGS=()
if command -v fc-list >/dev/null; then
    FC_LIST=$(fc-list)
    if [[ $FC_LIST == *"STIX Two Text:"* ]]; then
        FONT_ARGS+=(-V mainfont="STIX Two Text")
    elif [[ $FC_LIST == *"DejaVu Serif:"* ]]; then
        FONT_ARGS+=(-V mainfont="DejaVu Serif")
    fi
    if [[ $FC_LIST == *"STIX Two Math:"* ]]; then
        FONT_ARGS+=(-V mathfont="STIX Two Math")
    fi
fi

# Resolve theme template — substitute the absolute font dir into a tmpfile.
THEME_SRC=$ROOT/scripts/reference_pdf_theme.tex
THEME_RESOLVED=$(mktemp -t refpdf_theme.XXXXXX.tex)
trap 'command rm -f "$BODY" "$CLEAN" "$THEME_RESOLVED"' EXIT
command sed "s|@FONT_DIR@|$FONT_DIR/|g" "$THEME_SRC" >"$THEME_RESOLVED"

# Shared pandoc args — report docclass with shifted headings makes
# <h2>=chapter, <h3>=section in the resulting PDF.
PANDOC_ARGS=(
    --from html
    --pdf-engine="$ENGINE"
    --standalone
    --toc
    --toc-depth=1
    --shift-heading-level-by=-1
    --include-in-header="$THEME_RESOLVED"
    -V documentclass=extreport
    -V geometry:margin=0.85in
    -V fontsize=9pt
    -V colorlinks=true
    "${FONT_ARGS[@]}"
    --metadata title="stryke — Language Reference"
    --metadata subtitle="v$VERSION"
    --metadata author="MenkeTechnologies"
    --metadata date="$DATE"
)

echo "building $TEX"
pandoc "$BODY" "${PANDOC_ARGS[@]}" --to latex -o "$TEX"

echo "building $PDF (this takes a few minutes)"
pandoc "$BODY" "${PANDOC_ARGS[@]}" -o "$PDF"

echo "wrote $TEX ($(du -h "$TEX" | cut -f1))"
echo "wrote $PDF ($(du -h "$PDF" | cut -f1))"
