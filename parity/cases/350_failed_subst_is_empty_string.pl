# `s///` returns the number of substitutions, and the empty string (not 0) when
# nothing matched. `tr///` returns a count, so 0 stays 0.

my $s = "aaa";
my $r;

$r = ($s =~ s/x/y/);
print "1: [", $r, "] ", defined $r ? "defined" : "undef", " ", length($r), "\n";

$r = ($s =~ s/a/b/);
print "2: [$r] $s\n";

$r = ($s =~ s/a/c/g);
print "3: [$r] $s\n";

$r = ($s =~ tr/z//);
print "4: [$r]\n";

$_ = "hello";
my $n = s/q//;
print "5: [", $n, "] ", ($n ? "true" : "false"), " ", ($n eq "" ? "empty" : "nonempty"), "\n";

# /r never reports a count; it returns the (possibly unchanged) string.
my $copy = ($s =~ s/zzz//r);
print "6: [$copy]\n";
print "7: [", scalar(() = "abc" =~ /x/g), "]\n";
