# join EXPR, LIST with empty LIST returns "" (perlfunc). scalar keys on empty hash is 0.
print join("-", ());
print "\n";
my %e;
print 0 + keys %e;
print "\n";
