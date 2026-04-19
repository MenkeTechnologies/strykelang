my $s = "";
loop (my int $i = 0; $i < 500_000; $i = $i + 1) {
    $s ~= "x";
}
say $s.chars;
