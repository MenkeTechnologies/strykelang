# parity:1224
my @a = (25, 83, 6); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
