my int $sum = 0;
loop (my int $i = 0; $i < 5_000_000; $i = $i + 1) {
    $sum = $sum + $i;
}
say $sum;
