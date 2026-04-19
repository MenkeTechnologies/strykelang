h = {}
100_000.times { |i| h[i] = i * 2 }
sum = 0
h.each_key { |k| sum += h[k] }
puts sum
