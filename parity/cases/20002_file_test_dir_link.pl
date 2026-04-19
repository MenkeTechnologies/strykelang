#!/usr/bin/perl
use strict;
use warnings;

# File test: directory tests
my $d = "/tmp";
print "-d_tmp: ", (-d $d ? 1 : 0), "\n";
print "-f_tmp: ", (-f $d ? 1 : 0), "\n";

# Symlink test
my $link = "/tmp/forge_test_link_$$.tmp";
my $target = "/tmp/forge_test_target_$$.tmp";
open(TGTFH, '>', $target) or die;
print TGTFH "test\n";
close(TGTFH);
symlink($target, $link);
print "-l_link: ", (-l $link ? 1 : 0), "\n";
print "-l_target: ", (-l $target ? 1 : 0), "\n";
print "-f_link: ", (-f $link ? 1 : 0), "\n";

unlink $link;
unlink $target;
