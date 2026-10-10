# When both operands are strings, `& | ^` operate byte by byte and return a
# string. `&` stops at the shorter operand; `|` and `^` pad it with NULs. A
# numeric operand on either side makes the operation numeric.

print "1: ", "AB" | "  ", "\n";
print "2: ", "ab" & "_~", "\n";
print "3: ", "AB" ^ "  ", "\n";
print "4: ", length("abc" & "a"), " ", length("a" | "abc"), " ", length("a" ^ "abc"), "\n";
print "5: ", "12" | "3", "\n";
print "6: ", 12 | "3", " ", "12" | 3, " ", 12 | 3, "\n";
my $s = "ab";
$s |= "  ";
print "7: $s\n";
$s = "AB";
$s &= "_~";
print "8: $s\n";
print "9: ", join(",", map { ord } split //, "a" ^ "b"), "\n";
