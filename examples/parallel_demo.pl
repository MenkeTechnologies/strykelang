#!/usr/bin/env perlrs
# Parallel operations demo — showcasing rayon-powered parallelism

use strict;
use warnings;

# Generate a large range
my @data = (1..100);

# Parallel map: double every element across all cores
my @doubled = pmap { $_ * 2 } @data;
print "pmap: first 10 doubled = ", join(", ", @doubled[0..9]), "\n";

# Parallel grep: filter evens across all cores
my @evens = pgrep { $_ % 2 == 0 } @data;
print "pgrep: ", scalar @evens, " even numbers found\n";

# Parallel sort: sort using all cores
my @sorted = psort { $a <=> $b } reverse @data;
print "psort: first 10 = ", join(", ", @sorted[0..9]), "\n";

# Parallel for: execute side effects in parallel
print "pfor: processing...\n";
pfor {
    my $square = $_ * $_;
} @data;
print "pfor: done\n";

# Chained parallel operations
my @result = pmap { $_ ** 2 } pgrep { $_ % 3 == 0 } @data;
print "chained: squares of multiples of 3 = ", join(", ", @result[0..4]), "...\n";

print "All parallel operations completed successfully!\n";
