# Numeric strings keep full 64-bit integer precision in bit operators and
# arithmetic; beyond UV_MAX they become NVs.
print "18446744073709551615" + 0, "\n";
print "18446744073709551616" + 0, "\n";
print "9223372036854775808" + 0, "\n";
print "9223372036854775809" & "18446744073709551615" , "\n";
{ no warnings; print "9223372036854775809" | 0, "\n"; }
my $x = 18446744073709551615;
$x++;
print "$x\n";
my $y = 9223372036854775807;
$y++;
print "$y\n";
my $z = ~0;
$z -= 1;
print "$z\n";
my $w = 1;
$w <<= 63;
print "$w\n";
$w >>= 3;
print "$w\n";
$w |= 1;
print "$w\n";
$w &= ~1;
print "$w\n";
$w ^= ~0;
print "$w\n";
