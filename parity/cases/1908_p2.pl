# parity:1908
my @a = (0, 1, 23); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
