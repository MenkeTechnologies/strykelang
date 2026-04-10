my @a = (7,33,17); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
