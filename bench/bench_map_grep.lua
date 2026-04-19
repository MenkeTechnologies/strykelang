local data = {}
for i = 1, 500000 do
    data[i] = i
end
local doubled = {}
for i = 1, #data do
    doubled[i] = data[i] * 2
end
local count = 0
for i = 1, #doubled do
    if doubled[i] % 2 == 0 then
        count = count + 1
    end
end
print(count)
