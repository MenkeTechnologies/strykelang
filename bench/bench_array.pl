my @a;
for (my $i = 0; $i < 10000; $i = $i + 1) {
    push @a, $i;
}
my @b = sort { $a <=> $b } @a;
print $b[0], " ", $b[9999], "\n";
