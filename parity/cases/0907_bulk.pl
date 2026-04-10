my @a = (21,49,15); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
