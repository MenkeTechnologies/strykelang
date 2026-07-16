# substr as an lvalue mutates in place.
my $s = "hello";
substr($s, 0, 1) = "J";
print "$s\n";
# 4-arg substr replaces and returns the OLD value.
my $t = "world";
my $old = substr($t, 0, 1, "W");
print "$old $t\n";
# Negative offset counts from the end.
my $u = "abcdef";
substr($u, -2, 2) = "XY";
print "$u\n";
