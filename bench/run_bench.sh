#!/bin/bash
# Benchmark stryke vs perl5.
#
# Every serial bench is run twice: once on the canonical `bench/bench_*.pl`
# file, and once on a functionally-equivalent "perturbed" copy written to a
# temp file. If a compile-time AST shape matcher ever short-circuits one of
# the canonical shapes to a constant (as `bench_fusion.rs` used to do before
# it was deleted), the two columns will diverge loudly and this harness will
# print a **WARN** marker next to that row.
#
# Timing is delegated to hyperfine when available. Hyperfine warms the OS
# page cache, drops outliers, and reports mean ± stddev over N runs, which
# is the only sane way to measure sub-100ms programs. No hyperfine → abort
# (we will not ship fake numbers).

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
STRYKE="$HERE/../target/release/stryke"
PERL="${PERL:-perl}"
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

# Emit a functionally-equivalent perturbation of a bench file. The goal is
# to defeat *any* AST-shape matcher that might recognize the canonical
# file layout — renaming variables, adding a dead binding, wrapping the
# main block in a BEGIN-less prelude — while keeping stdout byte-identical.
perturb() {
    local src="$1" dst="$2"
    {
        printf 'my $__stryke_bench_guard = 1;\n'
        printf '$__stryke_bench_guard++ if 0;\n'
        # Rename common loop-counter identifiers so any `i_name == "i"` check fails.
        # Sed substitutions are scoped to `$i`/`my $i` and similar; aggressive enough
        # to perturb the AST but safe for the benches in bench/*.pl, which only use
        # `$i`, `$k`, `$x`, `$s`, `$sum`, `$count`, `$text`.
        sed \
            -e 's/\$i\b/\$__ii/g' \
            -e 's/\$sum\b/\$__s_sum/g' \
            -e 's/\$count\b/\$__c_count/g' \
            "$src"
    } > "$dst"
}

perl5_version() {
    "$PERL" -v 2>&1 | grep -m1 -i 'version' | sed 's/^This is //'
}

stryke_version() {
    "$STRYKE" -v 2>&1 | head -1
}

printf '\n'
printf ' stryke benchmark harness\n'
printf ' ---------------------------------------\n'
printf '  perl5:   %s\n'   "$(perl5_version)"
printf '  stryke:  %s\n'   "$(stryke_version)"
printf '  cores:   %s\n'   "$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo ?)"
printf '  warmup:  %s runs\n' "$WARMUP"
printf '  measure: hyperfine (min %s runs)\n\n' "$RUNS"

# Parse `mean ± stddev` from hyperfine --export-json output.
# We use --export-json over stdout scraping because hyperfine's stdout
# formatting changes across versions.
measure() {
    local label="$1" cmd="$2"
    local json="$TMPDIR_/hf.json"
    hyperfine --warmup "$WARMUP" --min-runs "$RUNS" --shell=none \
        --export-json "$json" \
        --command-name "$label" "$cmd" >/dev/null 2>&1 || return 1
    # mean and stddev are in seconds; format as ms.
    "$PERL" -MJSON::PP -e '
        local $/; my $j = <STDIN>;
        my $d = JSON::PP::decode_json($j);
        my $r = $d->{results}[0];
        printf "%.1f %.1f\n", $r->{mean}*1000, ($r->{stddev}//0)*1000;
    ' < "$json"
}

# Print a row: label | perl5 | stryke JIT | stryke noJIT | stryke perturbed | rs/perl5 | jit speedup | warn
#
# `noJit` runs the same canonical file under `STRYKE_NO_JIT=1` so the
# Cranelift block-JIT is disabled and only the bytecode interpreter runs.
# `jit/noJit` is `noJit_mean / jit_mean` — values >1.0 mean JIT helped.
#
# `warn` fires when canonical runs >20% faster than the perturbed variant,
# which would indicate a shape-specific fast path (the bug bench_fusion.rs
# represented). >20% because +/- noise in small benches can get ~10%.
row() {
    local label="$1" canonical="$2" perturbed="$3"

    read -r p5_mean p5_sd     < <(measure "perl_$label"               "$PERL $canonical")
    read -r rs_mean rs_sd     < <(measure "stryke_$label"             "$STRYKE $canonical")
    read -r rsnj_mean rsnj_sd < <(measure "stryke_${label}_nojit"     "$STRYKE --no-jit $canonical")
    read -r rsp_mean rsp_sd   < <(measure "stryke_${label}_perturbed" "$STRYKE $perturbed")

    local ratio jit_speedup
    ratio=$("$PERL" -e 'printf "%.2fx", $ARGV[0]/$ARGV[1]' -- "$rs_mean" "$p5_mean")
    jit_speedup=$("$PERL" -e 'printf "%.2fx", ($ARGV[1]==0?1:$ARGV[0]/$ARGV[1])' -- "$rsnj_mean" "$rs_mean")

    local warn=""
    # Only warn when the absolute gap is meaningful (>1ms) AND the ratio
    # exceeds 1.25. Tiny differences in sub-ms benches are noise.
    # `--` stops perl from reading a negative gap as a switch.
    local gap ratio_pp
    gap=$("$PERL" -e 'printf "%.2f", $ARGV[1]-$ARGV[0]' -- "$rs_mean" "$rsp_mean")
    ratio_pp=$("$PERL" -e 'printf "%.3f", ($ARGV[1]==0?1:$ARGV[1]/($ARGV[0]==0?1:$ARGV[0]))' -- "$rs_mean" "$rsp_mean")
    if "$PERL" -e 'exit !($ARGV[0] > 1.0 && $ARGV[1] > 1.25)' -- "$gap" "$ratio_pp"; then
        warn="  WARN: shape fast-path?"
    fi

    printf '  %-12s %10.1f  %10.1f  %10.1f  %10.1f  %8s  %8s%s\n' \
        "$label" "$p5_mean" "$rs_mean" "$rsnj_mean" "$rsp_mean" "$ratio" "$jit_speedup" "$warn"
}

printf '  %-12s %10s  %10s  %10s  %10s  %8s  %8s\n' \
    'bench' 'perl5 ms' 'stryke ms' 'noJit ms' 'perturb ms' 'rs/perl5' 'jit/noJit'
printf '  %-12s %10s  %10s  %10s  %10s  %8s  %8s\n' \
    '---------' '--------' '---------' '--------' '---------' '--------' '---------'

for name in startup fib loop string hash array regex map_grep; do
    file="$HERE/bench_${name}.pl"
    if [ ! -f "$file" ]; then continue; fi
    perturbed="$TMPDIR_/bench_${name}_perturbed.pl"
    perturb "$file" "$perturbed"
    row "$name" "$file" "$perturbed"
done

printf '\n  pmap vs map (stryke only, 50k items with per-item work)\n'
printf '  %-12s %10s  %10s  %10s\n' 'bench' 'map ms' 'pmap ms' 'speedup'
printf '  %-12s %10s  %10s  %10s\n' '---------' '--------' '--------' '--------'

read -r map_mean  _ < <(measure "stryke_map"  "$STRYKE $HERE/bench_pmap_perl.pl")
read -r pmap_mean _ < <(measure "stryke_pmap" "$STRYKE $HERE/bench_pmap.pl")
pmap_ratio=$("$PERL" -e 'printf "%.2fx", $ARGV[0]/$ARGV[1]' -- "$map_mean" "$pmap_mean")
printf '  %-12s %10.1f  %10.1f  %10s\n' 'pmap' "$map_mean" "$pmap_mean" "$pmap_ratio"

printf '\n  Notes:\n'
printf '    - All timings are mean of at least %s warm runs (warmup=%s).\n' "$RUNS" "$WARMUP"
printf '    - The "stryke ms" column has Cranelift block-JIT enabled (default).\n'
printf '    - The "noJit ms" column runs the same canonical file with\n'
printf '      "--no-jit" so only the bytecode interpreter executes — the\n'
printf '      "jit/noJit" ratio is noJit_mean / jit_mean (>1.0 = JIT helped).\n'
printf '    - The "perturb ms" column runs the same workload through a\n'
printf '      functionally-equivalent but shape-renamed copy. If it ever\n'
printf '      diverges from the canonical column, a compile-time shape\n'
printf '      matcher is recognizing the canonical file and cheating.\n'
printf '    - To override: RUNS=30 WARMUP=5 bash %s\n' "$(basename "$0")"
printf '\n'
