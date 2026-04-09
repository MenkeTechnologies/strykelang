my %h;
for (my $i = 0; $i < 100_000; $i = $i + 1) {
    $h{$i} = $i * 2;
}
my $sum = 0;
for my $k (keys %h) {
    $sum = $sum + $h{$k};
}
print $sum, "\n";
