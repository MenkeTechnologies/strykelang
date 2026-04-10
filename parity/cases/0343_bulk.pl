my @a = (29,1,2); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
