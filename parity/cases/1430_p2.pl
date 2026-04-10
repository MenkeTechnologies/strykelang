# parity:1430
my @a = (48, 22, 5); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
