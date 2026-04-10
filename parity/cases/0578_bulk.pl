my @a = (34,46,9); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
