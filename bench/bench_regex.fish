set count 0
for i in (seq 1 1000)
    if string match -rq '[0-9]+' "hello world 123"
        set count (math $count + 1)
    end
end
echo $count
