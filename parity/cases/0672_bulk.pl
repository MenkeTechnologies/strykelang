my @a = (16,4,8); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
