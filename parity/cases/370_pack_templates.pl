# pack / unpack: groups, length prefixes, modifiers, bit and hex strings, checksums.
print unpack("H*", pack("N/a*", "hello")), "\n";
print join("|", unpack("N/a*", pack("N/a*", "hello"))), "\n";
print join(",", unpack("(A2)*", "aabbccd")), "\n";
print unpack("%32C*", "abc"), " ", unpack("%8b*", "\xff\x01"), "\n";
print unpack("H*", pack("s< l> q", -3, 31, 15)), "\n";
print unpack("H*", pack("B8 b8 h4 H4", "10101010", "10101010", "1234", "1234")), "\n";
print join(",", unpack("B8 b8 h4 H4", pack("B8 b8 h4 H4", "10101010", "10101010", "1234", "1234"))), "\n";
print pack("u", "hello world"), unpack("u", pack("u", "hello world")), "\n";
print unpack("H*", pack("a* x2 n", "ab", 258)), "\n";
print join(",", unpack("a2 X2 a2 \@1 a1", "abcd")), "\n";
print unpack("H*", pack("n/a* w/a", "abc", "de")), "\n";
print join(",", unpack('a1 @4 a1 .', "abcdefgh")), "\n";
print unpack("H*", pack('a3 @1 a1', "xyz", "y")), "\n";
print unpack("H*", pack('a2 x!4 a1', "xy", "z")), "\n";
print unpack("H*", pack("f d> j", 1.5, -2.25, -2)), "\n";
print join(",", unpack("c C W", "\xff\xff\x41")), "\n";
printf "%d %s\n", unpack("w", pack("w", 300)), join(",", unpack("(a1 n)2", "a\0\1b\0\2"));
