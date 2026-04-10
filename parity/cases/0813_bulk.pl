my @a = (39,91,16); @a = sort { $a <=> $b } @a; printf "%d\n", $a[2];
