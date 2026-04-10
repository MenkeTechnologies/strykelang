my @a = (48,62,7); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
