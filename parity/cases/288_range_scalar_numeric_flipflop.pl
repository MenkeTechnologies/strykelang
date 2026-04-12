my $acc = "";
for my $i (1 .. 6) {
    $. = $i;
    $acc .= "[" . (3 .. 5) . "]";
}
print $acc, "\n";
