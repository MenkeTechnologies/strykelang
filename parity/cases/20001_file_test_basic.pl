#!/usr/bin/perl
use strict;
use warnings;

# Basic file test operators on a temp file
my $f = "/tmp/forge_test_$$.tmp";
open(WFH, '>', $f) or die "open: $!";
print WFH "hello world\n";
close(WFH);

print "-e: ", (-e $f ? 1 : 0), "\n";
print "-f: ", (-f $f ? 1 : 0), "\n";
print "-d: ", (-d $f ? 1 : 0), "\n";
print "-r: ", (-r $f ? 1 : 0), "\n";
print "-w: ", (-w $f ? 1 : 0), "\n";
print "-s: ", -s $f, "\n";
print "-z: ", (-z $f ? 1 : 0), "\n";

# Empty file test
my $ef = "/tmp/forge_test_empty_$$.tmp";
open(EFH, '>', $ef) or die;
close(EFH);
print "-z_empty: ", (-z $ef ? 1 : 0), "\n";
print "-s_empty: ", -s $ef, "\n";

# Non-existent
my $nf = "/tmp/forge_test_noexist_$$.tmp";
print "-e_noexist: ", (-e $nf ? 1 : 0), "\n";

unlink $f;
unlink $ef;
