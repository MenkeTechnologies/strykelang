# `do { BLOCK }` passes outer list context to the last expression (grep returns a list, not a count).
my @l = (1, 2, 3, 2, 1);
my @u = do {
    my %seen;
    grep { !$seen{$_}++ } @l
};
print scalar @u, "\n";
