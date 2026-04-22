s=""
for (( i=0; i<1000; i++ )); do
    s+="x"
done
echo ${#s}
