# parity:1525
my @a = (1,2); my $x = scalar splice @a, 1, 1; printf "%d\n", $x + @a;
