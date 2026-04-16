use strict;
use warnings;
# Named captures (?<name>...) populate %+; numbered captures still work.
my $s = "John Doe, age 42";
if ($s =~ /^(?<first>\w+) (?<last>\w+), age (?<age>\d+)/) {
    print "first=$+{first}\n";
    print "last=$+{last}\n";
    print "age=$+{age}\n";
    print "first num=$1\n";
    print "age num=$3\n";
}
# %- gives all matches per name (arrayref) — single-match here
print "first via %-: $-{first}->[0]\n";
