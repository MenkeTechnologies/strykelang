sub fib(Int $n --> Int) {
    return $n if $n <= 1;
    fib($n - 1) + fib($n - 2)
}
say fib(30);
