my @a;
loop (my int $i = 0; $i < 500_000; $i = $i + 1) {
    @a.push($i);
}
my @b = @a.sort({ $^a <=> $^b });
say @b[0], " ", @b[499999];
