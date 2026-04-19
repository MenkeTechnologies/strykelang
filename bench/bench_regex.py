import re
text = "The quick brown fox jumps over the lazy dog"
pat = re.compile(r"(\w+)\s+(\w+)$")
count = 0
for i in range(100_000):
    if pat.search(text):
        count += 1
print(count)
