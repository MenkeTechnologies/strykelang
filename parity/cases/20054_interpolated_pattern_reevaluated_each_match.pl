# m/$var/ recompiles when $var changes.
#
# The match memo keys on the pattern SOURCE text plus the haystack, both of
# which are identical on every turn of this loop while $p changes underneath
# them — so a memo that does not exclude interpolating patterns replays the
# first iteration's answer forever.
for my $p ("a", "b", "c") {
    print(("abc" =~ /^$p/) ? 1 : 0);
}
print "\n";

# Same, with the first pattern failing, so a frozen pattern shows as all-zero
# rather than all-one.
for my $p ("b", "a") {
    print(("abc" =~ /^$p/) ? 1 : 0);
}
print "\n";

# Through an array element rather than the loop variable itself.
my @pats = ("x", "a", "b");
for my $i (0 .. $#pats) {
    my $p = $pats[$i];
    print(("abc" =~ /^$p/) ? 1 : 0);
}
print "\n";

# qr// built inside the loop.
for my $p ("a", "b") {
    my $re = qr/^$p/;
    print(("abc" =~ $re) ? 1 : 0);
}
print "\n";

# ${name} form, and a pattern where the variable is not at the start.
for my $p ("b", "z") {
    print(("abc" =~ /a${p}c/) ? 1 : 0);
}
print "\n";

# The memo must still be correct for a NON-interpolating pattern repeated on
# the same haystack, and an end-anchored pattern must not be mistaken for one
# that interpolates.
my $hits = 0;
for my $i (1 .. 100) {
    $hits++ if "abc" =~ /^abc$/;
    $hits++ if "abc" =~ /^zzz$/;
}
print "$hits\n";

# An escaped \$ is a literal dollar, not a variable.
my $p = "NOPE";
print(("a\$p" =~ /a\$p/) ? "esc\n" : "noesc\n");

# Flags belong to the key: the same text with and without /i must not share.
for my $i (1 .. 3) {
    print(("ABC" =~ /abc/i) ? "i" : ".", ("ABC" =~ /abc/) ? "c" : ".");
}
print "\n";
