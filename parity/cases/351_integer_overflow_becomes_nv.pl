# `+ - *` that overflow the IV range stay exact while the result fits a UV and
# become an NV beyond UV_MAX. Literals beyond UV_MAX are NVs as well.

print "1: ", 9223372036854775807 + 1, "\n";
print "2: ", 9223372036854775807 * 2, "\n";
print "3: ", 4294967296 * 4294967296, "\n";
print "4: ", -9223372036854775807 - 2, "\n";
print "5: ", 18446744073709551616, "\n";
print "6: ", 100000000000000000000, "\n";
print "7: ", 9223372036854775807 + 9223372036854775807, "\n";
print "8: ", 9223372036854775807 + 9223372036854775807 + 2, "\n";
print "9: ", -9223372036854775807 - 1, "\n";
print "10: ", 2**53 + 1, "\n";
print "11: ", 9007199254740991 + 1, "\n";
print "12: ", 1e15 + 1, "\n";
