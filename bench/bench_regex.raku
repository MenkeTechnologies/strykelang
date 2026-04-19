my $text = "The quick brown fox jumps over the lazy dog";
my int $count = 0;
loop (my int $i = 0; $i < 100_000; $i = $i + 1) {
    if $text ~~ /(\w+) \s+ (\w+) $/ {
        $count = $count + 1;
    }
}
say $count;
