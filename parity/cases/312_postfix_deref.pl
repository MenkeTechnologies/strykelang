use strict;
use warnings;
use feature 'postderef';
no warnings 'experimental::postderef';
# Postfix deref ($ref->@*, ->%*, ->@[..], ->%{..}) — Perl 5.20+, default 5.24+.
my $aref = [10, 20, 30, 40];
my $href = { a => 1, b => 2, c => 3 };

print "all elems: ", join(",", $aref->@*), "\n";       # 10,20,30,40
print "slice:     ", join(",", $aref->@[0, 2]), "\n";  # 10,30

my @keys = $href->%*;     # flat list of pairs
print "kv count: ", scalar(@keys), "\n";               # 6
print "key b:    ", $href->{b}, "\n";                  # 2
print "hash slice: ", join(",", $href->@{qw(a c)}), "\n"; # 1,3
