h = {}
for i in range(100_000):
    h[i] = i * 2
s = 0
for k in h:
    s += h[k]
print(s)
