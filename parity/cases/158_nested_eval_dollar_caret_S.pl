# $^S stays true in nested eval blocks until the outermost eval returns.
eval {
    eval { print "n:" . ($^S ? "1" : "0") . "\n"; };
    print "m:" . ($^S ? "1" : "0") . "\n";
};
print "o:" . ($^S ? "1" : "0") . "\n";
