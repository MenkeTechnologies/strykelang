function main()
    data = collect(1:500_000)
    doubled = map(x -> x * 2, data)
    evens = filter(x -> x % 2 == 0, doubled)
    println(length(evens))
end
main()
