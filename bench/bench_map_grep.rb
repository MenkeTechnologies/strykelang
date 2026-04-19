data = (1..500_000).to_a
doubled = data.map { |x| x * 2 }
evens = doubled.select { |x| x.even? }
puts evens.length
