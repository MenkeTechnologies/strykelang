# Unary minus is right-associative and also negates strings: an identifier-like
# string gains a leading "-", a leading "-" or "+" is flipped, numerics negate.

print "1: ", - -1, "\n";
print "2: ", - - -3, "\n";
print "3: ", -"foo", " ", -"-bar", " ", -"+baz", " ", -"_x", "\n";
print "4: ", -"12", " ", -"-12", " ", -"1.5", " ", -"", "\n";
print "5: ", -(-5), " ", -(3 - 5), " ", - 2 ** 2, "\n";
my $s = "name";
print "6: ", -$s, " ", -(-$s), "\n";
