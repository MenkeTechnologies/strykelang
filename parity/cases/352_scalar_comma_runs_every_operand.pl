# A comma expression in scalar context evaluates every operand for its side
# effects and yields the last. The C-style `for` step is such an expression.

my ($i, $j) = (0, 10);
my $v = ($i++, $j--, "last");
print "1: $v $i $j\n";

my @trace;
sub t { push @trace, $_[0]; $_[0] }
my $s = (t("a"), t("b"), t("c"));
print "2: $s @trace\n";

for (my ($p, $q) = (0, 10); $p < 4; $p++, $q--) { print "3: $p $q\n" }

my $count = 0;
for (my $k = 0; $k < 6; $k += 2, $count++) { }
print "4: $count\n";

my $x = 5;
my $y = (($x += 1), ($x *= 2));
print "5: $y $x\n";
