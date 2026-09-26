# `map(EXPR, LIST)`, `map({ BLOCK } LIST)`, `grep(EXPR, LIST)` and
# `grep({ BLOCK } LIST)`: the parenthesized call form, where the parens
# delimit the whole argument list. Core modules use it (File::Basename:
# `map("\Q$_\E", @_)`).

my @a = (1, 2, 3);
my @b = map("<$_>", @a);
print "@b\n";
print join(",", map(uc, qw(a b))), "\n";
print join(",", map(($_ * 2), 1, 2)), "\n";
my @c = map({ $_ + 10 } @a);
print "@c\n";
my %h = map(($_ => 1), qw(x y));
print join(",", sort keys %h), "\n";
my @g = grep(/a/, qw(ab cd ba));
print "@g\n";
my @g2 = grep({ $_ > 1 } @a);
print "@g2\n";
print scalar(grep(defined, 1, undef, 2)), "\n";
my @q = map("\Q$_\E", "a.b", "c+d");
print "@q\n";
print join(",", map { $_ * 3 } grep($_ % 2, @a)), "\n";
print join(",", (map($_ + 1, @a))[0, 2]), "\n";
