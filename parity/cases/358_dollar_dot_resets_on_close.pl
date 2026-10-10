# `$.` counts per open handle and is reset by `close` and by re-opening.

my $data = "a\nb\nc\n";

open(my $in, '<', \$data) or die;
while (<$in>) { print "1: $.\n" }
close $in;
print "2: ", $., "\n";

open($in, '<', \$data) or die;
my $first = <$in>;
print "3: $.\n";
close $in;

open(my $second, '<', \"x\ny\n") or die;
while (<$second>) { chomp; print "4: $.:$_\n" }
close $second;
