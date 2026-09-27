# List::Util is XS in perl: its functions have no Perl body in List/Util.pm.
# Under --compat stryke provides them natively (BUG-319). Pins the (&@)
# block-first call form, the XS functions' scalar-context results, pairs'
# blessed _Pair objects, and head/tail's SIZE-first signature.
use strict; use warnings;
use List::Util qw(sum sum0 max min maxstr minstr first any all none notall reduce
                  reductions product shuffle uniq uniqnum uniqint pairs unpairs pairkeys
                  pairvalues pairmap pairgrep pairfirst head tail zip mesh zip_shortest);
my @n = (3, 1, 4, 1, 5, 9, 2, 6);
print sum(@n), " ", sum0(), " ", product(2, 3, 7), " ", sum(1.5, 2.25), "\n";
print defined(sum()) ? "def" : "undef", "\n";
print max(@n), " ", min(@n), " ", max("10", "9", "100"), "\n";
print maxstr("apple", "pear", "fig"), " ", minstr("apple", "pear", "fig"), "\n";
print first { $_ > 3 } @n; print "\n";
print((any { $_ == 9 } @n) ? "yes" : "no", " ", (all { $_ > 0 } @n) ? "yes" : "no", "\n");
print((none { $_ > 10 } @n) ? "yes" : "no", " ", (notall { $_ > 1 } @n) ? "yes" : "no", "\n");
print "[", (any { 0 } 1), "]\n";
print reduce { $a * $b } 1 .. 5; print "\n";
print join(",", reductions { $a + $b } 1 .. 5), "\n";
print scalar(reductions { $a + $b } 1 .. 5), "\n";
print scalar(my @s = shuffle(@n)), "\n";
print join(",", uniq(@n)), " ", scalar(uniq(@n)), "\n";
print join(",", uniqnum(1, "1.0", 2, "2", 3.5)), " ", join(",", uniqint(1.5, 1, 2.9, 2, -1.2)), "\n";
my @p = pairs(one => 1, two => 2);
print ref($p[0]), " ", $p[0]->key, "=", $p[0]->value, " ", scalar(@p), "\n";
print join(",", unpairs([a => 1], [b => 2])), " ", join(",", unpairs(@p)), "\n";
print join(",", pairkeys(a => 1, b => 2)), " ", join(",", pairvalues(a => 1, b => 2)), "\n";
print scalar(pairkeys(a => 1, b => 2)), "\n";
print join(",", pairmap { "$a-$b" } (x => 1, y => 2)), " ", scalar(pairmap { ($a, $b, 1) } (x => 1, y => 2)), "\n";
print join(",", pairfirst { $b > 1 } (x => 1, y => 2, z => 3)), " ", scalar(pairgrep { $b > 1 } (x => 1, y => 2, z => 3)), "\n";
print defined(scalar(pairfirst { 0 } (x => 1))) ? "def" : "undef", "\n";
print join(",", head(3, @n)), " ", join(",", tail(2, @n)), " ", join(",", head(-6, @n)), " ", join(",", tail(-6, @n)), "\n";
print scalar(head(2, 5, 6, 7)), " ", scalar(tail(2, 5, 6, 7)), "\n";
print join("|", map { join(",", map { defined $_ ? $_ : "u" } @$_) } zip([1, 2, 3], [4, 5])), "\n";
print join(",", map { defined $_ ? $_ : "u" } mesh([1, 2, 3], [4, 5])), " ", scalar(zip_shortest([1, 2, 3], [4, 5])) ? "ref" : "none", "\n";
my $cnt = () = first { $_ > 10 } (1, 2);
print "$cnt\n";
print List::Util::sum(1, 2, 3), " ", List::Util::first(sub { $_ eq "b" }, qw(a b c)), "\n";
print exists $INC{"List/Util.pm"} ? "loaded" : "missing", "\n";
