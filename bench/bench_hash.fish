# Fish doesn't have native associative arrays
# Simulate with two parallel arrays
set -g keys
set -g vals
for i in (seq 0 9999)
    set -a keys $i
    set -a vals $i
end
echo (count $keys)
