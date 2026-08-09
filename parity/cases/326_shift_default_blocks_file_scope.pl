# The mirror image of 325: at file scope every plain block is still "outside a
# subroutine", so bare `shift` inside `map` / `grep` / `sort` / `eval {}` /
# `do {}` / a bare block / a `for` body drains `@ARGV`, one element per hit.
#
# Each print reports the element that block consumed, so the order in which the
# blocks fire is pinned too — a block that ran twice or not at all would shift
# the whole tail of the output.

@ARGV = ('m1', 'g1', 's1', 'e1', 'd1', 'b1', 'f1', 'w1', 'tail');

my @m = map { shift } (1);
print "map=@m\n";

my @g = grep { shift; 1 } (1);
print "after-grep=$ARGV[0]\n";

my @s = sort { shift; 0 } (1, 2);
print "after-sort=$ARGV[0]\n";

my $e = eval { shift };
print "eval-block=$e\n";

my $d = do { shift };
print "do-block=$d\n";

{ my $b = shift; print "bare-block=$b\n"; }

for (1) { my $f = shift; print "for-block=$f\n"; }

my $i = 0;
while ($i++ < 1) { my $w = shift; print "while-block=$w\n"; }

print "left=@ARGV\n";

# `BEGIN` runs at compile time and is also "outside a subroutine": its bare
# `shift` takes from @ARGV. It runs before the statements above, so it seeds
# @ARGV itself to stay independent of them.
BEGIN {
    @ARGV = ('begin1', 'begin2');
    print "begin-shift=", shift, "\n";
    print "begin-left=@ARGV\n";
}
