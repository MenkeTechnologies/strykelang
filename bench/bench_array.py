a = []
for i in range(500_000):
    a.append(i)
b = sorted(a)
print(b[0], b[499999])
