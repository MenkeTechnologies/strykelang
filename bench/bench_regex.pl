my $text = "The quick brown fox jumps over the lazy dog";
my $count = 0;
for (my $i = 0; $i < 100_000; $i = $i + 1) {
    if ($text =~ /(\w+)\s+(\w+)$/) {
        $count = $count + 1;
    }
}
print $count, "\n";
