#!/bin/bash
# Benchmark stryke vs perl5 vs python3 vs ruby vs julia vs raku.
#
# Extends run_bench.sh with Python 3, Ruby, Julia, and Raku columns. Same methodology:
# hyperfine with warmup, mean of N runs, includes process startup.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
STRYKE="$HERE/../target/release/stryke"
PERL="${PERL:-perl}"
PYTHON="${PYTHON:-python3}"
RUBY="${RUBY:-ruby}"
JULIA="${JULIA:-julia}"
RAKU="${RAKU:-raku}"
LUAJIT="${LUAJIT:-luajit}"
RUNS="${RUNS:-10}"
WARMUP="${WARMUP:-3}"

if ! command -v hyperfine >/dev/null 2>&1; then
    printf 'error: hyperfine not found in PATH\n' >&2
    printf 'install: cargo install hyperfine (or brew install hyperfine)\n' >&2
    exit 1
fi

if [ ! -x "$STRYKE" ]; then
    printf 'error: %s not found. build first: cargo build --release\n' "$STRYKE" >&2
    exit 2
fi

TMPDIR_="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_"' EXIT

perl5_version() {
    "$PERL" -v 2>&1 | grep -m1 -i 'version' | sed 's/^This is //'
}

stryke_version() {
    "$STRYKE" -v 2>&1 | head -1
}

python_version() {
    "$PYTHON" --version 2>&1
}

ruby_version() {
    "$RUBY" --version 2>&1
}

julia_version() {
    "$JULIA" --version 2>&1
}

raku_version() {
    "$RAKU" --version 2>&1 | head -1
}

luajit_version() {
    "$LUAJIT" -v 2>&1 | head -1
}

# Detect which languages are available.
HAVE_JULIA=1
HAVE_RAKU=1
HAVE_LUAJIT=1
command -v "$JULIA"  >/dev/null 2>&1 || HAVE_JULIA=0
command -v "$RAKU"   >/dev/null 2>&1 || HAVE_RAKU=0
command -v "$LUAJIT" >/dev/null 2>&1 || HAVE_LUAJIT=0

printf '\n'
printf ' stryke benchmark harness (multi-language)\n'
printf ' ──────────────────────────────────────────────\n'
printf '  stryke:  %s\n'   "$(stryke_version)"
printf '  perl5:   %s\n'   "$(perl5_version)"
printf '  python:  %s\n'   "$(python_version)"
printf '  ruby:    %s\n'   "$(ruby_version)"
if [ "$HAVE_JULIA" = 1 ]; then
    printf '  julia:   %s\n'   "$(julia_version)"
fi
if [ "$HAVE_RAKU" = 1 ]; then
    printf '  raku:    %s\n'   "$(raku_version)"
fi
if [ "$HAVE_LUAJIT" = 1 ]; then
    printf '  luajit:  %s\n'   "$(luajit_version)"
fi
printf '  cores:   %s\n'   "$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo ?)"
printf '  warmup:  %s runs\n' "$WARMUP"
printf '  measure: hyperfine (min %s runs)\n\n' "$RUNS"

# Parse mean ms from hyperfine --export-json output.
measure() {
    local label="$1" cmd="$2"
    local json="$TMPDIR_/hf.json"
    hyperfine --warmup "$WARMUP" --min-runs "$RUNS" --shell=none \
        --export-json "$json" \
        --command-name "$label" "$cmd" >/dev/null 2>&1 || return 1
    "$PERL" -MJSON::PP -e '
        local $/; my $j = <STDIN>;
        my $d = JSON::PP::decode_json($j);
        my $r = $d->{results}[0];
        printf "%.1f\n", $r->{mean}*1000;
    ' < "$json"
}

ratio() {
    "$PERL" -e 'printf "%.2fx", $ARGV[0]/$ARGV[1]' -- "$1" "$2"
}

# Build header dynamically based on available languages.
# Use printf %10s to match data rows which use %10.1f and %10s.
hdr_fmt='  %-12s %10s  %10s  %10s  %10s'
sep_fmt='  %-12s %10s  %10s  %10s  %10s'
hdr_args=('bench' 'stryke ms' 'perl5 ms' 'python3 ms' 'ruby ms')
sep_args=('------------' '----------' '----------' '----------' '----------')
ratio_hdr_args=('vs perl5' 'vs python' 'vs ruby')
ratio_sep_args=('----------' '----------' '----------')

if [ "$HAVE_JULIA" = 1 ]; then
    hdr_fmt="$hdr_fmt  %10s"
    sep_fmt="$sep_fmt  %10s"
    hdr_args+=('julia ms')
    sep_args+=('----------')
    ratio_hdr_args+=('vs julia')
    ratio_sep_args+=('----------')
fi
if [ "$HAVE_RAKU" = 1 ]; then
    hdr_fmt="$hdr_fmt  %10s"
    sep_fmt="$sep_fmt  %10s"
    hdr_args+=('raku ms')
    sep_args+=('----------')
    ratio_hdr_args+=('vs raku')
    ratio_sep_args+=('----------')
fi
if [ "$HAVE_LUAJIT" = 1 ]; then
    hdr_fmt="$hdr_fmt  %10s"
    sep_fmt="$sep_fmt  %10s"
    hdr_args+=('luajit ms')
    sep_args+=('----------')
    ratio_hdr_args+=('vs luajit')
    ratio_sep_args+=('----------')
fi

# Add ratio columns to format
for _ in "${ratio_hdr_args[@]}"; do
    hdr_fmt="$hdr_fmt  %10s"
    sep_fmt="$sep_fmt  %10s"
done

printf "${hdr_fmt}\n" "${hdr_args[@]}" "${ratio_hdr_args[@]}"
printf "${sep_fmt}\n" "${sep_args[@]}" "${ratio_sep_args[@]}"

for name in startup fib loop string hash array regex map_grep; do
    pl="$HERE/bench_${name}.pl"
    py="$HERE/bench_${name}.py"
    rb="$HERE/bench_${name}.rb"
    jl="$HERE/bench_${name}.jl"
    rk="$HERE/bench_${name}.raku"
    lj="$HERE/bench_${name}.lua"
    [ -f "$pl" ] || continue
    [ -f "$py" ] || continue
    [ -f "$rb" ] || continue

    rs_mean=$(measure "stryke_$name"  "$STRYKE $pl")
    p5_mean=$(measure "perl_$name"    "$PERL $pl")
    py_mean=$(measure "python_$name"  "$PYTHON $py")
    rb_mean=$(measure "ruby_$name"    "$RUBY $rb")

    r_p5=$(ratio "$rs_mean" "$p5_mean")
    r_py=$(ratio "$rs_mean" "$py_mean")
    r_rb=$(ratio "$rs_mean" "$rb_mean")

    row=$(printf '  %-12s %10.1f  %10.1f  %10.1f  %10.1f' \
        "$name" "$rs_mean" "$p5_mean" "$py_mean" "$rb_mean")
    ratios=$(printf '%10s  %10s  %10s' "$r_p5" "$r_py" "$r_rb")

    if [ "$HAVE_JULIA" = 1 ] && [ -f "$jl" ]; then
        jl_mean=$(measure "julia_$name" "$JULIA $jl")
        r_jl=$(ratio "$rs_mean" "$jl_mean")
        row=$(printf '%s  %10.1f' "$row" "$jl_mean")
        ratios=$(printf '%s  %10s' "$ratios" "$r_jl")
    fi

    if [ "$HAVE_RAKU" = 1 ] && [ -f "$rk" ]; then
        rk_mean=$(measure "raku_$name" "$RAKU $rk")
        r_rk=$(ratio "$rs_mean" "$rk_mean")
        row=$(printf '%s  %10.1f' "$row" "$rk_mean")
        ratios=$(printf '%s  %10s' "$ratios" "$r_rk")
    fi

    if [ "$HAVE_LUAJIT" = 1 ] && [ -f "$lj" ]; then
        lj_mean=$(measure "luajit_$name" "$LUAJIT $lj")
        r_lj=$(ratio "$rs_mean" "$lj_mean")
        row=$(printf '%s  %10.1f' "$row" "$lj_mean")
        ratios=$(printf '%s  %10s' "$ratios" "$r_lj")
    fi

    printf '%s  %s\n' "$row" "$ratios"
done

printf '\n  Notes:\n'
printf '    - All timings include process startup (not steady-state).\n'
printf '    - Mean of %s warm runs (warmup=%s), measured by hyperfine.\n' "$RUNS" "$WARMUP"
printf '    - "vs X" = stryke_ms / X_ms — values <1.0x mean stryke is faster.\n'
printf '    - Julia timings include JIT compilation (first-run cost).\n'
printf '    - Raku timings include MoarVM startup overhead.\n'
printf '    - LuaJIT uses Lua patterns (not PCRE) for regex bench.\n'
printf '    - To override: RUNS=30 WARMUP=5 bash %s\n' "$(basename "$0")"
printf '\n'
