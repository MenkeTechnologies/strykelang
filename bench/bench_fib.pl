sub fib {
    my $n = shift @_;
    return $n if $n <= 1;
    return fib($n - 1) + fib($n - 2);
}
print fib(25), "\n";
