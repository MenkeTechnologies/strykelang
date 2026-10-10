# `scalar(gmtime)` is the ctime string without a newline, and `wantarray` in a
# scalar-context call is false (the empty string), not 0.

print "1: [", scalar(gmtime(0)), "]\n";
print "2: [", scalar(gmtime(86400 * 365)), "]\n";
my $t = gmtime(1_000_000_000);
print "3: [$t]\n";

sub ctx { return wantarray }
my $s = ctx();
my @l = ctx();
print "4: [", $s, "] [", $l[0], "] ", (defined $s ? "defined" : "undef"), "\n";
print "5: ", (length($s) == 0 ? "empty" : "nonempty"), "\n";
