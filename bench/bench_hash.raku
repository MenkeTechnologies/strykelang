my %h;
loop (my int $i = 0; $i < 100_000; $i = $i + 1) {
    %h{$i} = $i * 2;
}
my int $sum = 0;
for %h.values -> $v {
    $sum = $sum + $v;
}
say $sum;
