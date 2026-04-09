my $s = "";
for (my $i = 0; $i < 10000; $i = $i + 1) {
    $s .= "x";
}
print length($s), "\n";
