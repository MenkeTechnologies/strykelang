my @a = (12,78,5); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
