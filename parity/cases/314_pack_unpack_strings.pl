use strict;
use warnings;
# String pack codes: a (raw, NUL pad), A (space pad), Z (NUL terminator).
my $a = pack("a5", "hi");          # "hi\0\0\0"
my $A = pack("A5", "hi");          # "hi   "
my $Z = pack("Z5", "hi");          # "hi\0\0\0"
printf "a:[%s] len=%d\n", $a =~ s/\0/./gr, length($a);
printf "A:[%s] len=%d\n", $A, length($A);
printf "Z:[%s] len=%d\n", $Z =~ s/\0/./gr, length($Z);

# Truncation: too-long input is cut to width
my $t = pack("a3", "abcdef");
printf "trunc:[%s] len=%d\n", $t, length($t);

# Unpack back — A* strips trailing whitespace, a* keeps NULs
my ($u1) = unpack("A5", $A);
printf "A unpack:[%s]\n", $u1;

# Padded fields and concatenation
my $rec = pack("A4 a4 N", "foo", "bar", 42);
my ($f, $b, $n) = unpack("A4 a4 N", $rec);
printf "rec: f=[%s] b=[%s] n=%d\n", $f, $b =~ s/\0/./gr, $n;
