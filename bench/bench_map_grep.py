data = list(range(1, 500_001))
doubled = [x * 2 for x in data]
evens = [x for x in doubled if x % 2 == 0]
print(len(evens))
