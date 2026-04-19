#!/usr/bin/perl
use strict;
use warnings;

# seek, tell, read on a temp file
my $f = "/tmp/forge_test_seek_$$.tmp";
open(WFH, '>', $f) or die "open write: $!";
print WFH "ABCDEFGHIJ";
close(WFH);

open(RFH, '<', $f) or die "open read: $!";

# tell at start
print "tell0: ", tell(RFH), "\n";

# read 3 bytes
my $buf;
my $n = read(RFH, $buf, 3);
print "read3: $buf\n";
print "n: $n\n";
print "tell3: ", tell(RFH), "\n";

# seek to position 5
seek(RFH, 5, 0);
print "tell5: ", tell(RFH), "\n";
$n = read(RFH, $buf, 2);
print "read_at5: $buf\n";

# seek from end
seek(RFH, -3, 2);
$n = read(RFH, $buf, 3);
print "read_end3: $buf\n";

close(RFH);
unlink $f;
