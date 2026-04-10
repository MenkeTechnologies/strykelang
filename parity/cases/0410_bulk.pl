my @b = unpack("C*", pack("C", 10)); printf "%d\n", $b[0];
