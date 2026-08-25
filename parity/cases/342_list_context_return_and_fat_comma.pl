# Two places where a list stopped being a list.
#
# `return COND ? (LIST) : ()` evaluated its arms in scalar context, so the comma
# operator collapsed the pair list to its last element; and `=>` did not separate
# a paren-less list operator's arguments, so everything after the first was
# dropped. `Carp::_fetch_sub` is called as `_fetch_sub utf8 => 'is_utf8'` and
# `Carp::caller_info` returns exactly the ternary shape.

sub pairs { my $n = shift; return $n ? (a => $n, b => $n * 2) : () }
my @p = pairs(3);
print "1: @p\n";
my %h = pairs(3);
print "2: $h{a} $h{b}\n";
print "3: ", scalar(pairs(0)), "\n" if 0;      # perl: empty list in scalar ctx
my @empty = pairs(0);
print "4: ", scalar(@empty), "\n";

# Nested ternaries keep list context all the way down.
sub nested { my $n = shift; return $n > 1 ? ($n > 2 ? (1, 2, 3) : (4, 5)) : () }
print "5: ", join(",", nested(3)), " | ", join(",", nested(2)), " | ", join(",", nested(0)), "\n";

# A ternary with no list arm still returns a scalar.
sub scalarish { my $n = shift; return $n ? "yes" : "no" }
print "6: ", scalarish(1), " ", scalarish(0), "\n";

# `=>` is a comma, so a paren-less call takes every argument after it.
sub join3 { my @a = @_; return scalar(@a) . ":" . join("|", @a) }
print "7: ", join3(one => "two"), "\n";
print "8: ", join3 one => "two", "\n";
print "9: ", join3 "a", b => "c", "\n";

# The bareword to the left of `=>` is still auto-quoted.
sub first { return $_[0] }
print "10: ", first(bareword => 1), "\n";

# An aggregate assignment in scalar context is the right-hand side's element
# count, so `while (my %i = f())` ends when `f` returns an empty list.
my $i = 0;
while (my %g = ($i < 2 ? (n => $i) : ())) { print "11: n=$g{n}\n"; $i++ }
print "12: ", scalar(my %z = (x => 1, y => 2)), " ", scalar(my @q = (7, 8, 9)), "\n";
if (my @e = ()) { print "13: nonempty\n" } else { print "13: empty\n" }
