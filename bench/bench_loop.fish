set sum 0
for i in (seq 0 99999)
    set sum (math $sum + $i)
end
echo $sum
