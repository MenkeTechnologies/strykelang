package X;
use strict;

my $max_intsize;

BEGIN {
    my $checkint = 1111;
    for my $d (5..64) {
        $checkint .= 1;
        my $int   = eval qq| $checkint |;
        if ($int =~ /[eE]/) {
            $max_intsize = $d - 1;
            last;
        }
    }
}

1;
