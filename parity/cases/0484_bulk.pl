my @a = (2,88,10); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
