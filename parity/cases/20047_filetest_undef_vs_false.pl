# A file test has two distinct falsehoods. When the stat itself fails the test
# is undef; when the stat succeeds but the predicate does not hold it is the
# empty-string false. Collapsing both to 0 loses the distinction that tells
# "no such file" apart from "exists, but is not a directory".
my $missing = "/no/such/path/for/parity/probe";

for my $op (qw(e f d r w x z)) {
    my $v = eval "-$op \$missing";
    printf "%s_missing  %s\n", $op, defined($v) ? "['$v']" : "UNDEF";
}

# The interpreter binary itself always exists and is a plain file.
my $present = $^X;
printf "e_present   %s\n", (-e $present) ? "['" . (-e $present) . "']" : "UNDEF";
printf "d_on_file   %s\n", defined(-d $present) ? "['" . (-d $present) . "'] len=" . length(-d $present) : "UNDEF";
printf "f_on_file   %s\n", (-f $present) ? "['" . (-f $present) . "']" : "UNDEF";

# A directory that exists: -d true, -f the empty-string false.
printf "d_on_dir    %s\n", (-d "/") ? "['" . (-d "/") . "']" : "UNDEF";
printf "f_on_dir    %s\n", defined(-f "/") ? "['" . (-f "/") . "'] len=" . length(-f "/") : "UNDEF";

# A false file test is still false in boolean position.
print "bool_missing ", ((-e $missing) ? "T" : "F"), "\n";
print "bool_dir     ", ((-d "/") ? "T" : "F"), "\n";
