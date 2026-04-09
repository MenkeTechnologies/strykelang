my @data = (1..500_000);
my @doubled = map { $_ * 2 } @data;
my @evens = grep { $_ % 2 == 0 } @doubled;
print scalar @evens, "\n";
