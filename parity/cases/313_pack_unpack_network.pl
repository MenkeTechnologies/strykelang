use strict;
use warnings;
# Network-order pack/unpack: n (uint16 BE), N (uint32 BE), v (uint16 LE), V (uint32 LE).
my $bin = pack("nN", 0x1234, 0xDEADBEEF);
print "len=", length($bin), "\n";          # 6 bytes
my @bytes = unpack("C*", $bin);
printf "bytes=%s\n", join(" ", map { sprintf("%02X", $_) } @bytes);
# Round trip
my ($n, $N) = unpack("nN", $bin);
printf "n=0x%04X N=0x%08X\n", $n, $N;

# Little-endian
my $le = pack("vV", 0x1234, 0xDEADBEEF);
my @lb = unpack("C*", $le);
printf "le bytes=%s\n", join(" ", map { sprintf("%02X", $_) } @lb);
my ($v, $V) = unpack("vV", $le);
printf "v=0x%04X V=0x%08X\n", $v, $V;
