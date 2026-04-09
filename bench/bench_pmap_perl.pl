my @data = (1..10000);
my @result = map { $_ * $_ + $_ * 3 + 7 } @data;
print scalar @result, "\n";
