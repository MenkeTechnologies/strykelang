# `each` keeps its iterator with the container, so it works through a reference
# and on arrays, not only on a named hash. Exhaustion rewinds the iterator.

my $h = { a => 1, b => 2, c => 3 };
my @seen;
while (my ($k, $v) = each %$h) { push @seen, "$k=$v" }
print "1: ", join(",", sort @seen), "\n";

# Exhausted -> rewound: a second loop sees every pair again.
@seen = ();
while (my ($k, $v) = each %{$h}) { push @seen, "$k=$v" }
print "2: ", join(",", sort @seen), "\n";

# Scalar context returns the key only.
my @keys;
while (defined(my $k = each %$h)) { push @keys, $k }
print "3: ", join(",", sort @keys), "\n";

# Arrays yield (index, value).
my @a = qw(x y z);
while (my ($i, $v) = each @a) { print "4: $i=$v\n" }

# Through an array reference.
my $r = \@a;
while (my ($i, $v) = each @$r) { print "5: $i=$v\n" }

# Scalar context on an array yields the index.
my @idx;
while (defined(my $i = each @a)) { push @idx, $i }
print "6: @idx\n";

# Two aliases of one container share one iterator.
my $r2 = \@a;
my ($i1) = each @$r;
my ($i2) = each @$r2;
print "7: $i1 $i2\n";
