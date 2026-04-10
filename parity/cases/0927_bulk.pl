my @b = unpack("C*", pack("C", 127)); printf "%d\n", $b[0];
