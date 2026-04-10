# parity:1669
my @a = (24, 60, 14); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
