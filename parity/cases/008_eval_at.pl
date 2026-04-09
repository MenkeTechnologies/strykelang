eval { 1 };
print "e:" . ($@ eq "" ? "1" : "0") . "\n";
print "ok\n";
eval { die "failmsg" };
print $@;
