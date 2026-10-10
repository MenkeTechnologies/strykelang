# A width above 65535 is valid and pads exactly.

my $wide = sprintf("%70000s", "x");
print "1: ", length($wide), " ", substr($wide, -3), "\n";

my $left = sprintf("%-70000s|", "y");
print "2: ", length($left), " ", substr($left, 0, 3), "\n";

my $zero = sprintf("%070000d", 42);
print "3: ", length($zero), " ", substr($zero, -4), "\n";

my $prec = sprintf("%.70000s", "z" x 80000);
print "4: ", length($prec), "\n";
