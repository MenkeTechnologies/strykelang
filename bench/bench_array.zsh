a=()
for (( i=0; i<10000; i++ )); do
    a+=($i)
done
echo ${#a[@]}
