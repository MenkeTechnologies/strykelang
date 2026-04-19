function main()
    text = "The quick brown fox jumps over the lazy dog"
    pat = r"(\w+)\s+(\w+)$"
    count = 0
    for i in 1:100_000
        if occursin(pat, text)
            count += 1
        end
    end
    println(count)
end
main()
