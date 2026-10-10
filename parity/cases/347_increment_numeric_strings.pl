# `++` on a string that looks like a number is numeric, and keeps a fraction.
# Only strings of the form /^[a-zA-Z]*[0-9]*$/ get the magic string increment.

for my $v ("1.5", "-1.5", "0.5", "1e2", "9", "-1", "", " 3", "0x10", "a9", "Zz", "zz", "a", "-a") {
    my $x = $v;
    $x++;
    print "[$v] -> [$x]\n";
}

my $f = 1.5;     $f++;  print "float: $f\n";
my $u;           $u++;  print "undef: $u\n";
my $m = -0.5;    $m++;  print "neg: $m\n";
my $big = 9223372036854775807;
$big++;
print "iv_max: $big\n";
my $d = "1.5"; $d--; print "dec: $d\n";
