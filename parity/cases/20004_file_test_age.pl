#!/usr/bin/perl
use strict;
use warnings;

# File test: -M -A -C return fractional days
my $f = "/tmp/forge_test_age_$$.tmp";
open(AGEFH, '>', $f) or die;
print AGEFH "test\n";
close(AGEFH);

my $m = -M $f;
my $a = -A $f;
my $c = -C $f;

# Just freshly created: should be very small (< 1 day)
my $m_ok = ($m >= 0 && $m < 1) ? 1 : 0;
my $a_ok = ($a >= 0 && $a < 1) ? 1 : 0;
my $c_ok = ($c >= 0 && $c < 1) ? 1 : 0;
print "M_ok: $m_ok\n";
print "A_ok: $a_ok\n";
print "C_ok: $c_ok\n";
# They should be numeric
my $m_num = ($m + 0 == $m) ? 1 : 0;
print "M_num: $m_num\n";

unlink $f;
