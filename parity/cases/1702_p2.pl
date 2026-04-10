# parity:1702
my @a = (30, 62, 1); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
