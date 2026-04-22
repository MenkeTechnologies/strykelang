set a (seq 1 1000)
set sum 0
for x in $a
    if test (math $x % 2) -eq 0
        set sum (math $sum + $x '*' 2)
    end
end
echo $sum
