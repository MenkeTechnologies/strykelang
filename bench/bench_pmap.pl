my @data = (1..50_000);
my @result = pmap { my $x = $_; $x = $x * $x + 3 for 1..20; $x } @data;
print scalar @result, "\n";
