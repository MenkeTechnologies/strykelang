#!/usr/bin/perl
use strict;
use warnings;

# read() builtin - reading specified bytes from a filehandle
my $f = "/tmp/forge_test_read_$$.tmp";
open(WFH, '>', $f) or die;
print WFH "Hello, World!";
close(WFH);

open(RFH, '<', $f) or die;
my $buf;
my $n = read(RFH, $buf, 5);
print "n: $n\n";
print "buf: $buf\n";

# Read more
my $buf2;
$n = read(RFH, $buf2, 3);
print "n2: $n\n";
print "buf2: $buf2\n";

close(RFH);
unlink $f;
