text = "The quick brown fox jumps over the lazy dog"
pat = /(\w+)\s+(\w+)$/
count = 0
100_000.times do
  count += 1 if pat.match(text)
end
puts count
