# An integer in IV_MAX+1 .. UV_MAX is a UV: bit operators produce it, literals
# spell it, and + - * / keep it exact while the result fits an unsigned 64-bit value.
print ~0, "\n";
print ~0 >> 1, "\n";
print 1 << 63, "\n";
print -8 >> 1, "\n";
print -1 & 0xFFFFFFFFFFFFFFFF, "\n";
print 18446744073709551615, "\n";
print 9223372036854775808, "\n";
print 0xFFFFFFFFFFFFFFFF, "\n";
print 18446744073709551615 - 1, "\n";
print 18446744073709551615 + 1, "\n";
print 9223372036854775807 + 1, "\n";
print 9223372036854775808 - 1, "\n";
print ~5, "\n";
print -1 | 0, "\n";
print 1 << 64, "\n";
print 8 >> -1, " ", 8 << -1, "\n";
print ~0 == 18446744073709551615 ? "eq" : "ne", "\n";
print ~0 % 10, " ", int(~0 / 5), " ", ~0 & 255, " ", ~0 ^ 1, "\n";
print -(~0), "\n";
print 4611686018427387904 * 2, " ", 4611686018427387904 * 4, "\n";
printf "%d %u %s %x %o\n", ~0, ~0, ~0, ~0, ~0;
