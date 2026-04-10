# wantarray: undef in void context, false in scalar, true in list.
sub w { print defined(wantarray) ? (wantarray ? "L" : "S") : "U"; print "\n"; }
w();
scalar w();
my @a = w();
