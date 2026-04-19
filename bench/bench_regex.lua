local text = "The quick brown fox jumps over the lazy dog"
local count = 0
for i = 1, 100000 do
    if text:match("(%w+)%s+(%w+)$") then
        count = count + 1
    end
end
print(count)
