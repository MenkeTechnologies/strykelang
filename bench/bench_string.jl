function main()
    buf = IOBuffer()
    for i in 1:500_000
        write(buf, 'x')
    end
    s = String(take!(buf))
    println(length(s))
end
main()
