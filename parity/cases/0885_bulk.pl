my @a = (9,8,7); my $x = splice @a, 1, 1; printf "%d\n", $x + scalar @a;
