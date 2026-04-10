my @a = (1,2);
splice @a, 1, 0, (9);
print join("", @a), "\n";
