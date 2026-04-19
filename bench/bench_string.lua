local t = {}
for i = 1, 500000 do
    t[i] = "x"
end
local s = table.concat(t)
print(#s)
