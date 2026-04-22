fib() {
    local n=$1
    (( n < 2 )) && echo $n && return
    echo $(( $(fib $((n-1))) + $(fib $((n-2))) ))
}
fib 20
