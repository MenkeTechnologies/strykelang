use strict;
use warnings;
# Non-capturing group (?:...) — preserves grouping without consuming a $N slot.
my $s = "foobar=42";
if ($s =~ /^(?:foo)(\w+)=(\d+)/) {
    print "1=$1\n";   # "bar" — (?:foo) doesn't capture
    print "2=$2\n";   # "42"
}
# Mixed (?:...) and (...)
if ("abc-xyz-123" =~ /^([a-z]+)(?:-[a-z]+)-(\d+)$/) {
    print "first=$1 num=$2\n";
}
# Atomic group (?>...) — no backtracking; matches greedily once.
if ("aaab" =~ /^(?>a*)b/) {
    print "atomic: matched\n";
}
if ("aaa" =~ /^(?>a*)b/) {
    print "atomic: matched\n";
} else {
    print "atomic: no match\n";
}
