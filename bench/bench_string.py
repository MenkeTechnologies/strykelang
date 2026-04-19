import io
buf = io.StringIO()
for i in range(500_000):
    buf.write("x")
s = buf.getvalue()
print(len(s))
