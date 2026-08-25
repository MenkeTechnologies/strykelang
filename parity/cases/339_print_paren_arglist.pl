# A list operator immediately followed by `(` takes those parens as its COMPLETE
# argument list; whatever follows the `)` belongs to the surrounding expression
# and is evaluated in void context. Spacing is irrelevant.
print ("a"), "b";
print "\n";
print("c"), "d";
print "\n";
print ("e", "f"), "g";
print "\n";

# The parens close the arg list, so the trailing operator applies to print's
# RETURN value, not to its arguments.
print (1 + 2) * 3;
print "\n";
print ("h") . "ignored";
print "\n";

# Nested parens inside the argument list are ordinary grouping.
print (("i"), ("j"));
print "\n";

# A statement modifier after the closing paren still applies.
print ("k") if 1;
print "\n";
print ("l") unless 0;
print "\n";

# Other list operators follow the same rule.
my @a = (3, 1);
push (@a, 9), 7;
print "@a\n";
printf ("%s-%s", "m", "n"), "discarded";
print "\n";

# `join (...)` is the operator taking the parens, so print still receives the
# whole comma list after it.
print join ("-", 1, 2), "Z";
print "\n";

# Without parens, print takes the entire comma list as usual.
print "o", "p";
print "\n";

# Low-precedence operators are legal inside the parens even though they bind
# looser than the comma that separates arguments.
print (1 and 2);
print "\n";
print (0 or 7);
print "\n";
print (1 && 2, 3);
print "\n";
