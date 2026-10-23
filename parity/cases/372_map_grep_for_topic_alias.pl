use strict;
use warnings;

# $_ aliases the source elements in map/grep blocks and for ($scalar).
my @w = ("x1", "y1", "x2");
map { s/x/z/ } @w;
print "@w\n";
my @l = (" a ", " b");
my @t = map { s/^\s+|\s+$//g; $_ } @l;
print "[@l][@t]\n";
my @a = (1, 2, 3, 4);
my @e = grep { $_ *= 2; $_ > 4 } @a;
print "@a|@e\n";
my @n = ("x\n", "y\n");
map { chomp } @n;
print "@n|\n";
my $s = "  pad  ";
for ($s) { s/^\s+//; s/\s+$// }
print "[$s]\n";
my $u = "abc";
s/b/B/ for $u;
print "$u\n";
my @b = (1, 2, 3);
for (@b) { { $_ *= 2 } }
print "@b\n";
$_ = "outer";
my @c = (1, 2);
map { $_ += 1 } @c;
print "$_|@c\n";
