#!/usr/bin/env stryke
# Fibonacci with memoization

use strict;
use warnings;

my %memo;

fn fib_n {
    my $n = shift @_;
    return $n if $n <= 1;
    return $memo{$n} if exists $memo{$n};
    $memo{$n} = fib_n($n - 1) + fib_n($n - 2);
    return $memo{$n};
}

for my $i (0..20) {
    printf "fib(%2d) = %d\n", $i, fib_n($i);
}
