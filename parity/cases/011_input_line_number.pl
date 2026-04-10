# $. is undef until a line is read from a handle (perlvar); matches last-read handle line count.
print "undef_before:" . (defined($.) ? "0" : "1") . "\n";
open my $fh, "<", $0 or die;
print "undef_after_open:" . (defined($.) ? "0" : "1") . "\n";
my $x = <$fh>;
print "after_read:" . $. . "\n";
close $fh;
