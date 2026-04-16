use strict;
use warnings;
# Lookahead (?=...) / negative (?!) / lookbehind (?<=...) / negative (?<!).
my @pos;
for my $s ("abc123", "abc", "x123y", "999end") {
    push @pos, ($s =~ /(\d+)(?=end)/)         ? "Y:$1" : "N";
}
print join(",", @pos), "\n";

# negative lookahead — match digits NOT followed by 'x'
my @neg;
for my $s ("12x", "12y", "5", "0x") {
    push @neg, ($s =~ /^(\d+)(?!x)/) ? "Y:$1" : "N";
}
print join(",", @neg), "\n";

# lookbehind — match a digit preceded by '$'
my @lb;
for my $s ('$5', 'X5', '$$9', '99') {
    push @lb, ($s =~ /(?<=\$)(\d)/) ? "Y:$1" : "N";
}
print join(",", @lb), "\n";

# negative lookbehind — digit NOT preceded by 0
for my $s ("7", "07", "10") {
    if ($s =~ /(?<!0)(\d)$/) {
        print "match: $1 in $s\n";
    } else {
        print "no match in $s\n";
    }
}
