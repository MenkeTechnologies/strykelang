my @a = (30,20,6); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
