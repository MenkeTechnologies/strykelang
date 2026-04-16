use strict;
use warnings;
# `local $\` (output record sep — appended after every print)
# `local $,` (output field sep — between print args)
# `local $"` (list interpolation sep — joins arrays in "@arr").
{
    local $\ = "!\n";
    local $, = "-";
    print "a", "b", "c";    # a-b-c!\n
}
# Outside the block: defaults restored
print "x", "y";              # xy   (no newline, no separator)
print "\n";

{
    local $" = "/";
    my @parts = ("usr", "local", "bin");
    print "@parts\n";        # usr/local/bin
}
my @parts = (1, 2, 3);
print "@parts\n";            # 1 2 3 (default $" = " ")
