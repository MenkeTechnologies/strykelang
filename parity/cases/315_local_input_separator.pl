use strict;
use warnings;
# `local $/` for slurp mode and custom record separator. Tests outside-of-I/O
# scoping behavior (the variable must restore on block exit).
print "before: ", defined($/) ? "[$/]" : "undef", "\n";
{
    local $/;          # undef = slurp
    print "inside slurp: ", defined($/) ? "[$/]" : "undef", "\n";
}
print "after slurp: [$/]\n";

{
    local $/ = "|";    # custom delimiter
    print "inside custom: [$/]\n";
}
print "after custom: [$/]\n";

# Nested local
{
    local $/ = "X";
    {
        local $/ = "Y";
        print "deep: [$/]\n";
    }
    print "mid: [$/]\n";
}
print "outer: [$/]\n";
