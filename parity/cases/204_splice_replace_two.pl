my @a = (1,2,3,4);
splice @a, 1, 2, (9);
print join(",", @a), "\n";
