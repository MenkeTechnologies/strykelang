use strict;
use warnings;
# <<"EOF" — explicit double-quoted form, interpolated.
my $a = 1;
my @arr = (10, 20, 30);
print <<"EOF";
a=$a
arr=@arr
sum=${\ ($a + $arr[1])}
EOF
