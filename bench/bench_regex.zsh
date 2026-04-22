count=0
for (( i=0; i<1000; i++ )); do
    [[ "hello world 123" =~ [0-9]+ ]] && (( count++ ))
done
echo $count
