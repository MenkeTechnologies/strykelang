a=()
for (( i=1; i<=1000; i++ )); do
    a+=($i)
done
sum=0
for x in "${a[@]}"; do
    (( x % 2 == 0 )) && (( sum += x * 2 ))
done
echo $sum
