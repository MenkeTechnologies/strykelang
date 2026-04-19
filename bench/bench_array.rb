a = []
500_000.times { |i| a.push(i) }
b = a.sort
puts "#{b[0]} #{b[499999]}"
