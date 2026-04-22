typeset -A h
for (( i=0; i<10000; i++ )); do
    h[$i]=$i
done
echo ${#h[@]}
