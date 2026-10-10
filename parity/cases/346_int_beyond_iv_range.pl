# `int` truncates toward zero. A value that no longer fits an IV stays a number
# rather than saturating at IV_MAX; an NV inside the unsigned range stays exact.

print "1: ", int(1e20), "\n";
print "2: ", int(-1e20), "\n";
print "3: ", int(1e19), "\n";
print "4: ", int(9.3e18), "\n";
print "5: ", int(-9.3e18), "\n";
print "6: ", int(1.8446744073709552e19), "\n";
print "7: ", int(2.5), " ", int(-2.5), " ", int(0.99), " ", int(-0.99), "\n";
print "8: ", int(9**9**9), " ", int(-9**9**9), "\n";
print "9: ", int(9223372036854775807), "\n";
print "10: ", int("42.9xyz"), " ", int("  7  "), " ", int(""), "\n";
