function main()
    h = Dict{Int,Int}()
    for i in 0:99_999
        h[i] = i * 2
    end
    s = 0
    for v in values(h)
        s += v
    end
    println(s)
end
main()
