# pack/unpack `U`: a Unicode code point stored UTF-8 encoded. A template that
# starts with `U` produces a character string; after `C0` the same code point
# is the three UTF-8 bytes. Text::Wrap uses `pack 'U'`.
# Character counts go through `split //`: stryke `length` is byte-based (BUG-247).

my $s = pack("U", 0x263A);
print scalar(my @cs = split //, $s), " ", ord($s), "\n";
my $w = pack("U*", 0x48, 0x49, 0x263A);
print scalar(my @cw = split //, $w), " ", join(",", map { ord } split //, $w), "\n";
print join(",", unpack("U*", "abc")), "\n";
print join(",", unpack("C*", pack("C0U", 0x263A))), "\n";
print length(pack("C0U", 0xE9)), "\n";
print join(",", unpack("U*", pack("U*", 0x41, 0xE9, 0x20AC))), "\n";
