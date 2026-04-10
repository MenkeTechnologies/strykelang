my @b = unpack("C*", pack("C", 80)); printf "%d\n", $b[0];
