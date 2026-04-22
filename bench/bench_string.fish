set s ""
for i in (seq 1 1000)
    set s "$s"x
end
echo (string length $s)
