# parity:1014
my @a = (35, 35, 3); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
