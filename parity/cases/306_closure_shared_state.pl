use strict;
use warnings;
# Two closures sharing the same lexical — both see updates from each other.
sub make_pair {
    my $n = 0;
    return (sub { ++$n }, sub { $n });
}
my ($incr, $peek) = make_pair();
$incr->();
$incr->();
print $peek->(), "\n";   # 2
$incr->();
print $peek->(), "\n";   # 3
