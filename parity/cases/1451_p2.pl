# parity:1451
my %h3 = (a=>1); delete $h3{a}; printf "%d\n", exists $h3{a} ? 1 : 0;
