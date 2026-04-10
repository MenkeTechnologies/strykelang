my @a = (43,17,19); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
