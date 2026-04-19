function fib(n)
    n <= 1 && return n
    fib(n - 1) + fib(n - 2)
end
println(fib(30))
