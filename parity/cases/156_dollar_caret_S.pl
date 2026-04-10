# $^S: true only while executing code compiled for an eval (perlvar).
print "m:" . ($^S ? "1" : "0") . "\n";
eval { print "e:" . ($^S ? "1" : "0") . "\n"; };
print "a:" . ($^S ? "1" : "0") . "\n";
