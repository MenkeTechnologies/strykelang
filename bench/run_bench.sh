#!/bin/bash
# Benchmark perlrs vs perl5
# Runs each test 3 times and reports median

PERLRS="$(dirname "$0")/../target/release/perlrs"
PERL="perl"

if [ ! -x "$PERLRS" ]; then
    echo "Error: release binary not found. Run: cargo build --release"
    exit 1
fi

time_cmd() {
    # Returns time in milliseconds
    local start end
    start=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')
    eval "$@" > /dev/null 2>&1
    end=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')
    echo $(( (end - start) / 1000000 ))
}

# macOS doesn't have date +%s%N, use perl for timing
time_ms() {
    local cmd="$1"
    "$PERL" -MTime::HiRes=time -e "
        my \$start = time();
        system(qq{$cmd >/dev/null 2>&1});
        my \$elapsed = (time() - \$start) * 1000;
        printf \"%.1f\n\", \$elapsed;
    "
}

median3() {
    echo "$1 $2 $3" | tr ' ' '\n' | sort -n | sed -n '2p'
}

printf "\n"
printf " ██████╗ ███████╗██████╗ ██╗     ██████╗ ███████╗\n"
printf " ██╔══██╗██╔════╝██╔══██╗██║     ██╔══██╗██╔════╝\n"
printf " ██████╔╝█████╗  ██████╔╝██║     ██████╔╝███████╗\n"
printf " ██╔═══╝ ██╔══╝  ██╔══██╗██║     ██╔══██╗╚════██║\n"
printf " ██║     ███████╗██║  ██║███████╗██║  ██║███████║\n"
printf " ╚═╝     ╚══════╝╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝╚══════╝\n"
printf " ┌──────────────────────────────────────────────────────┐\n"
printf " │ BENCHMARK SUITE // perlrs vs perl5                  │\n"
printf " └──────────────────────────────────────────────────────┘\n\n"

printf "  perl5:  %s\n" "$($PERL -v 2>&1 | grep 'version' | head -1)"
printf "  perlrs: %s\n" "$($PERLRS -v 2>&1 | head -1)"
printf "  cores:  %s\n\n" "$(sysctl -n hw.ncpu 2>/dev/null || nproc)"

printf "  ── SEQUENTIAL BENCHMARKS ─────────────────────────────────────\n"
printf "  %-22s %10s %10s %10s\n" "TEST" "perl5(ms)" "perlrs(ms)" "RATIO"
printf "  %-22s %10s %10s %10s\n" "────────────────────" "─────────" "──────────" "─────"

BENCHDIR="$(dirname "$0")"

for bench in startup fib loop string hash array regex map_grep; do
    file="$BENCHDIR/bench_${bench}.pl"
    if [ ! -f "$file" ]; then continue; fi

    # 3 runs each
    p1=$(time_ms "$PERL $file"); p2=$(time_ms "$PERL $file"); p3=$(time_ms "$PERL $file")
    r1=$(time_ms "$PERLRS $file"); r2=$(time_ms "$PERLRS $file"); r3=$(time_ms "$PERLRS $file")

    pm=$(median3 "$p1" "$p2" "$p3")
    rm=$(median3 "$r1" "$r2" "$r3")

    # Compute ratio
    ratio=$("$PERL" -e "printf '%.2fx', $rm / $pm" 2>/dev/null)

    printf "  %-22s %10s %10s %10s\n" "$bench" "${pm}ms" "${rm}ms" "$ratio"
done

printf "\n  ── PARALLEL vs SEQUENTIAL (perlrs only) ───────────────────────\n"
printf "  %-22s %10s %10s %10s\n" "TEST" "map(ms)" "pmap(ms)" "SPEEDUP"
printf "  %-22s %10s %10s %10s\n" "────────────────────" "────────" "────────" "───────"

# Sequential map in perlrs
s1=$(time_ms "$PERLRS $BENCHDIR/bench_pmap_perl.pl"); s2=$(time_ms "$PERLRS $BENCHDIR/bench_pmap_perl.pl"); s3=$(time_ms "$PERLRS $BENCHDIR/bench_pmap_perl.pl")
sm=$(median3 "$s1" "$s2" "$s3")

# Parallel pmap in perlrs
p1=$(time_ms "$PERLRS $BENCHDIR/bench_pmap.pl"); p2=$(time_ms "$PERLRS $BENCHDIR/bench_pmap.pl"); p3=$(time_ms "$PERLRS $BENCHDIR/bench_pmap.pl")
pm=$(median3 "$p1" "$p2" "$p3")

speedup=$("$PERL" -e "printf '%.2fx', $sm / $pm" 2>/dev/null)
printf "  %-22s %10s %10s %10s\n" "map vs pmap (10k)" "${sm}ms" "${pm}ms" "$speedup"

printf "\n  ── SYSTEM ─────────────────────────────────────────\n"
printf "  >>> BENCHMARK COMPLETE <<<\n"
printf " ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░\n"
