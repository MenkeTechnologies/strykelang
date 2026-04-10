# die inside eval: no " at FILE line N." suffix when the message ends with newline.
eval { die "x\n" };
print $@;
