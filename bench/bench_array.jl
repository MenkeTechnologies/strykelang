function main()
    a = Int[]
    for i in 0:499_999
        push!(a, i)
    end
    b = sort(a)
    println(b[1], " ", b[500_000])
end
main()
