# `<=>` on an unordered pair (NaN) returns undef, not 0.

my $nan = "nan" + 0;
my $inf = 9**9**9;
my $nan2 = $inf - $inf;

for my $pair ([1, $nan], [$nan, 1], [$nan2, $nan2], [$inf, $inf], [1, 2], [2, 1], [3, 3]) {
    my $r = $pair->[0] <=> $pair->[1];
    print defined $r ? "def:$r" : "undef", "\n";
}

# Comparison operators involving NaN are all false except !=.
print +($nan == $nan ? "eq" : "ne"), " ", ($nan != $nan ? "ne" : "eq"), " ",
      ($nan < 1 ? "lt" : "nlt"), " ", ($nan > 1 ? "gt" : "ngt"), "\n";
