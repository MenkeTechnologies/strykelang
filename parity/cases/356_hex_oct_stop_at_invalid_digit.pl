# `hex` and `oct` parse the longest valid prefix and ignore the rest.

print "1: ", hex("ff"), " ", hex("0xFF"), " ", hex("xff"), " ", hex("ffzz"), " ", hex("0xFG"), "\n";
print "2: ", hex(""), " ", hex("zz"), " ", hex("1_0"), "\n";
print "3: ", oct("755"), " ", oct("789"), " ", oct("0755"), " ", oct("7_7"), "\n";
print "4: ", oct("0x1f"), " ", oct("x1f"), " ", oct("0b101"), " ", oct("b101"), " ", oct("0b12"), "\n";
print "5: ", oct("0o17"), " ", oct("o17"), " ", oct(" 12 "), " ", oct(""), "\n";
