#!/usr/bin/perl
use strict;
use warnings;

# File test: -T and -B (text/binary)
my $tf = "/tmp/forge_test_text_$$.tmp";
open(TFH, '>', $tf) or die;
print TFH "This is plain text content.\n";
close(TFH);
print "-T_text: ", (-T $tf ? 1 : 0), "\n";
print "-B_text: ", (-B $tf ? 1 : 0), "\n";

my $bf = "/tmp/forge_test_bin_$$.tmp";
open(BFH, '>', $bf) or die;
print BFH "\x00\x01\x02\x03\x04\x05\x80\x81\x82\x83";
close(BFH);
print "-T_bin: ", (-T $bf ? 1 : 0), "\n";
print "-B_bin: ", (-B $bf ? 1 : 0), "\n";

unlink $tf;
unlink $bf;
