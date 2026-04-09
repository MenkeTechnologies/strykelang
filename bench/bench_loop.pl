my $sum = 0;
for (my $i = 0; $i < 5_000_000; $i = $i + 1) {
    $sum = $sum + $i;
}
print $sum, "\n";
