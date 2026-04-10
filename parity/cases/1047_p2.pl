# parity:1047
my @a = (41, 37, 13); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
