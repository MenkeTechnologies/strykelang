# parity:1153
my @a9 = sort { length($b) <=> length($a) } qw/x xx xxx/; printf "%s\n", $a9[0];
