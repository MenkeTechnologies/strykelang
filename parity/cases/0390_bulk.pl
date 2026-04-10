my @a = (20,30,11); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
