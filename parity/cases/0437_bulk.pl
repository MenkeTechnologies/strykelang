my @a = (11,59,1); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
