set a
for i in (seq 0 9999)
    set -a a $i
end
echo (count $a)
