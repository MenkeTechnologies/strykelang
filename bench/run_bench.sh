#!/bin/bash
# Benchmark perlrs vs perl5; for perlrs, compare Cranelift JIT on vs off (`PERLRS_NO_JIT`).
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
printf "  %-18s %10s %10s %10s %10s\n" "TEST" "perl5" "jit_on" "jit_off" "off/on"
printf "  %-18s %10s %10s %10s %10s\n" "────────────────────" "─────────" "──────────" "──────────" "──────"

BENCHDIR="$(dirname "$0")"

for bench in startup fib loop string hash array regex map_grep; do
    file="$BENCHDIR/bench_${bench}.pl"
    if [ ! -f "$file" ]; then continue; fi

    p1=$(time_ms "$PERL $file"); p2=$(time_ms "$PERL $file"); p3=$(time_ms "$PERL $file")
    rj1=$(time_ms "$PERLRS $file"); rj2=$(time_ms "$PERLRS $file"); rj3=$(time_ms "$PERLRS $file")
    rn1=$(time_ms "env PERLRS_NO_JIT=1 $PERLRS $file"); rn2=$(time_ms "env PERLRS_NO_JIT=1 $PERLRS $file"); rn3=$(time_ms "env PERLRS_NO_JIT=1 $PERLRS $file")

    pm=$(median3 "$p1" "$p2" "$p3")
    jm=$(median3 "$rj1" "$rj2" "$rj3")
    nm=$(median3 "$rn1" "$rn2" "$rn3")

    ratio=$("$PERL" -e "printf '%.2fx', (\$ARGV[1] / \$ARGV[0])" "$jm" "$nm" 2>/dev/null)

    printf "  %-18s %8sms %8sms %8sms %10s\n" "$bench" "${pm}" "${jm}" "${nm}" "$ratio"
done

printf "\n  ── PARALLEL vs SEQUENTIAL (perlrs only, 10k) ───────────────────\n"
printf "  %-10s %12s %12s %12s\n" "" "map(ms)" "pmap(ms)" "pmap/map"
printf "  %-10s %12s %12s %12s\n" "──────────" "────────────" "────────────" "────────────"

s1=$(time_ms "$PERLRS $BENCHDIR/bench_pmap_perl.pl"); s2=$(time_ms "$PERLRS $BENCHDIR/bench_pmap_perl.pl"); s3=$(time_ms "$PERLRS $BENCHDIR/bench_pmap_perl.pl")
sm=$(median3 "$s1" "$s2" "$s3")
p1=$(time_ms "$PERLRS $BENCHDIR/bench_pmap.pl"); p2=$(time_ms "$PERLRS $BENCHDIR/bench_pmap.pl"); p3=$(time_ms "$PERLRS $BENCHDIR/bench_pmap.pl")
pm=$(median3 "$p1" "$p2" "$p3")
speedup=$("$PERL" -e "printf '%.2fx', \$ARGV[0] / \$ARGV[1]" "$sm" "$pm" 2>/dev/null)
printf "  %-10s %12sms %12sms %12s\n" "JIT on" "${sm}" "${pm}" "$speedup"

s1o=$(time_ms "env PERLRS_NO_JIT=1 $PERLRS $BENCHDIR/bench_pmap_perl.pl"); s2o=$(time_ms "env PERLRS_NO_JIT=1 $PERLRS $BENCHDIR/bench_pmap_perl.pl"); s3o=$(time_ms "env PERLRS_NO_JIT=1 $PERLRS $BENCHDIR/bench_pmap_perl.pl")
smo=$(median3 "$s1o" "$s2o" "$s3o")
p1o=$(time_ms "env PERLRS_NO_JIT=1 $PERLRS $BENCHDIR/bench_pmap.pl"); p2o=$(time_ms "env PERLRS_NO_JIT=1 $PERLRS $BENCHDIR/bench_pmap.pl"); p3o=$(time_ms "env PERLRS_NO_JIT=1 $PERLRS $BENCHDIR/bench_pmap.pl")
pmo=$(median3 "$p1o" "$p2o" "$p3o")
speedup_o=$("$PERL" -e "printf '%.2fx', \$ARGV[0] / \$ARGV[1]" "$smo" "$pmo" 2>/dev/null)
printf "  %-10s %12sms %12sms %12s\n" "JIT off" "${smo}" "${pmo}" "$speedup_o"

printf "\n  ── SYSTEM ─────────────────────────────────────────\n"
printf "  >>> BENCHMARK COMPLETE <<<\n"
printf " ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░\n"
