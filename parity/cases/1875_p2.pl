# parity:1875
my @a = (47, 96, 13); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
