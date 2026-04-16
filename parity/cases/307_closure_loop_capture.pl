use strict;
use warnings;
# Closure capture inside a loop — each iteration's lexical is captured fresh.
my @cbs;
for my $i (1..4) {
    push @cbs, sub { $i * 10 };
}
print $_->(), "\n" for @cbs;
