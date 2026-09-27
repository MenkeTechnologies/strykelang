# References compare and hash by identity; qr// stringifies as (?^FLAGS:...)
# and keeps its flags when interpolated.
my $a = [1];
my $b = [1];
my $c = $a;
print $a == $b ? "eq" : "ne", " ", $a == $c ? "eq" : "ne", " ", "$a" eq "$c" ? "seq" : "sne", "\n";
my %seen;
$seen{$_}++ for $a, $b, $c;
print scalar(keys %seen), "\n";
print "$a" =~ /^ARRAY\(0x[0-9a-f]+\)$/ ? "ok" : "bad", "\n";
my $o = bless [], "K";
print "$o" =~ /^K=ARRAY\(0x[0-9a-f]+\)$/ ? "ok" : "bad", "\n";
my @arr;
print \@arr == \@arr ? "same" : "diff", "\n";
my $re = qr/ab/i;
print "$re\n";
print "xAB" =~ /x$re/ ? "y" : "n", "\n";
my $cs = qr/ab/;
print "xAB" =~ /x$cs/i ? "y" : "n", "\n";
print qr/a/msix, " ", qr/b/, "\n";
