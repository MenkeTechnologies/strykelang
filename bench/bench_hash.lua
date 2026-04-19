local h = {}
for i = 0, 99999 do
    h[i] = i * 2
end
local sum = 0
for _, v in pairs(h) do
    sum = sum + v
end
print(sum)
