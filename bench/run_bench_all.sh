#!/bin/bash
# Benchmark stryke vs perl5 vs python3 vs ruby vs julia.
#
# Extends run_bench.sh with Python 3, Ruby, and Julia columns. Same methodology:
# hyperfine with warmup, mean of N runs, includes process startup.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
STRYKE="$HERE/../target/release/stryke"
PERL="${PERL:-perl}"
PYTHON="${PYTHON:-python3}"
RUBY="${RUBY:-ruby}"
JULIA="${JULIA:-julia}"
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

printf '\n'
printf ' stryke benchmark harness (5-way)\n'
printf ' ──────────────────────────────────────────────\n'
printf '  stryke:  %s\n'   "$(stryke_version)"
printf '  perl5:   %s\n'   "$(perl5_version)"
printf '  python:  %s\n'   "$(python_version)"
printf '  ruby:    %s\n'   "$(ruby_version)"
printf '  julia:   %s\n'   "$(julia_version)"
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

printf '  %-12s %10s  %10s  %10s  %10s  %10s  %10s  %10s  %10s  %10s\n' \
    'bench' 'stryke ms' 'perl5 ms' 'python3 ms' 'ruby ms' 'julia ms' 'vs perl5' 'vs python' 'vs ruby' 'vs julia'
printf '  %-12s %10s  %10s  %10s  %10s  %10s  %10s  %10s  %10s  %10s\n' \
    '---------' '---------' '--------' '----------' '-------' '--------' '--------' '---------' '-------' '--------'

for name in startup fib loop string hash array regex map_grep; do
    pl="$HERE/bench_${name}.pl"
    py="$HERE/bench_${name}.py"
    rb="$HERE/bench_${name}.rb"
    jl="$HERE/bench_${name}.jl"
    [ -f "$pl" ] || continue
    [ -f "$py" ] || continue
    [ -f "$rb" ] || continue
    [ -f "$jl" ] || continue

    rs_mean=$(measure "stryke_$name"  "$STRYKE $pl")
    p5_mean=$(measure "perl_$name"    "$PERL $pl")
    py_mean=$(measure "python_$name"  "$PYTHON $py")
    rb_mean=$(measure "ruby_$name"    "$RUBY $rb")
    jl_mean=$(measure "julia_$name"   "$JULIA $jl")

    r_p5=$(ratio "$rs_mean" "$p5_mean")
    r_py=$(ratio "$rs_mean" "$py_mean")
    r_rb=$(ratio "$rs_mean" "$rb_mean")
    r_jl=$(ratio "$rs_mean" "$jl_mean")

    printf '  %-12s %10.1f  %10.1f  %10.1f  %10.1f  %10.1f  %10s  %10s  %10s  %10s\n' \
        "$name" "$rs_mean" "$p5_mean" "$py_mean" "$rb_mean" "$jl_mean" "$r_p5" "$r_py" "$r_rb" "$r_jl"
done

printf '\n  Notes:\n'
printf '    - All timings include process startup (not steady-state).\n'
printf '    - Mean of %s warm runs (warmup=%s), measured by hyperfine.\n' "$RUNS" "$WARMUP"
printf '    - "vs X" = stryke_ms / X_ms — values <1.0x mean stryke is faster.\n'
printf '    - Julia timings include JIT compilation (first-run cost).\n'
printf '    - To override: RUNS=30 WARMUP=5 bash %s\n' "$(basename "$0")"
printf '\n'
