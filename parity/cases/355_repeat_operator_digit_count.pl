# After a complete term `x` directly followed by digits is the repetition
# operator and its count, not an identifier named `x3`.

my $s = "ab"x3;
print "1: $s\n";
my @l = (1, 2)x2;
print "2: @l\n";
my $n = 4;
print "3: ", "-"x$n, "|", "=" x 2, "|", "z"x0, "|\n";
my @m = ('a') x3;
print "4: ", scalar(@m), " @m\n";
my $line = "*"x5 . "\n";
print "5: $line";
my %h = (x2 => "key");
print "6: $h{x2}\n";
sub x7 { "sub" }
print "7: ", x7(), "\n";
