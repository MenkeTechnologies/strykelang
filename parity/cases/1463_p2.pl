# parity:1463
my @a = (1, 24, 15); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
