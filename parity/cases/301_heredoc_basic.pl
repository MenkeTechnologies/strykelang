use strict;
use warnings;
# Bareword <<EOF — interpolated heredoc terminating at EOF on its own line.
my $name = "world";
print <<EOF;
hello $name
line two
EOF
print "after\n";
