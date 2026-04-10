my @a = (25,75,18); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
