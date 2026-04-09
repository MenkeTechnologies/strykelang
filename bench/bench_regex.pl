my $text = "The quick brown fox jumps over the lazy dog " x 1000;
my $count = 0;
while ($text =~ /\b\w{4,}\b/g) {
    $count = $count + 1;
}
print $count, "\n";
