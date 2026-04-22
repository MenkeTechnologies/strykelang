function fib
    set n $argv[1]
    if test $n -lt 2
        echo $n
        return
    end
    set a (fib (math $n - 1))
    set b (fib (math $n - 2))
    echo (math $a + $b)
end
fib 20
