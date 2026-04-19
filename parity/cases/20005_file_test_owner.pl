#!/usr/bin/perl
use strict;
use warnings;

# File test: ownership -o -O
my $f = "/tmp/forge_test_own_$$.tmp";
open(OWNFH, '>', $f) or die;
print OWNFH "mine\n";
close(OWNFH);

print "-o: ", (-o $f ? 1 : 0), "\n";
print "-O: ", (-O $f ? 1 : 0), "\n";
print "-R: ", (-R $f ? 1 : 0), "\n";
print "-W: ", (-W $f ? 1 : 0), "\n";

unlink $f;
