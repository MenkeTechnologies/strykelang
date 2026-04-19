local a = {}
for i = 0, 499999 do
    a[#a + 1] = i
end
table.sort(a)
print(a[1] .. " " .. a[500000])
