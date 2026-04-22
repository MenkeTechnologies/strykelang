sum=0
for (( i=0; i<100000; i++ )); do
    (( sum += i ))
done
echo $sum
