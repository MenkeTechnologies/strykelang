print 3 ^ 5;
print "\n";
print 8 >> 1;
print "\n";
# Bare `<<` tokenizes as heredoc in forge (not `ShiftLeft`); use `<<=` for left-shift parity.
my $x = 3;
$x <<= 1;
print $x;
print "\n";
