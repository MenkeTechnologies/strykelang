# `EXPR for @a` aliases `$_` to the elements and restores the outer `$_`.
my @a = (1, 2, 3);
$_ *= 2 for @a;
print "@a\n";
s/^/n/ for @a;
print "@a\n";
$_ = "outer";
print for 1 .. 2;
print "\n$_\n";
