# parity:1061
my @a = (9,8,7); my @r = splice @a, 1, 1; printf "%d\n", $r[0] + scalar @a;
