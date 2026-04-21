sub fib_ {
    my $n = shift @_;
    return $n if $n <= 1;
    return fib_($n - 1) + fib_($n - 2);
}
print fib_(30), "\n";
