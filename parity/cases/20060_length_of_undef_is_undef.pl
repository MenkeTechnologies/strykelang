# length(undef) is undef in Perl, not 0; length("") is 0. `defined length($x)`
# is the idiomatic "is this a real string?" test, so collapsing the two makes
# it always true. The loop runs past the JIT threshold so a native tier that
# re-boxes an i64 cannot hide behind the cold path.

my $u;
my $l = length($u);
print defined($l) ? "defined" : "undef", "\n";
print "[", (defined($l) ? $l : ""), "]\n";
print "[", length(""), "]\n";
print "[", length("abc"), "]\n";
print defined(length(undef)) ? "defined" : "undef", "\n";

my %h = (a => "xy");
print defined(length($h{a})) ? "defined" : "undef", "\n";
print defined(length($h{missing})) ? "defined" : "undef", "\n";

my ($real, $none) = (0, 0);
for my $i (1 .. 200) {
    my $s = $i % 4 ? "str$i" : undef;
    if (defined length($s)) { $real++ } else { $none++ }
}
print "$real $none\n";
