# `open` on a scalar reference reads from / writes to the scalar itself.

my $text = "l1\nl2\nl3\n";
open(my $in, '<', \$text) or die "open: $!";
while (my $line = <$in>) { chomp $line; print "1: $.:$line\n" }
close $in;

open($in, '<', \"x\ny\n") or die;
my @all = <$in>;
close $in;
print "2: ", scalar(@all), "\n";

my $out = "";
open(my $w, '>', \$out) or die;
print $w "hello ", 42;
printf $w " %03d", 7;
close $w;
print "3: [$out]\n";

open($w, '>>', \$out) or die;
print $w "!";
close $w;
print "4: [$out]\n";

# `>` truncates what was there.
open($w, '>', \$out) or die;
print $w "fresh";
close $w;
print "5: [$out]\n";

# The scalar is current after every print, before close.
my $live;
open(my $lw, '>', \$live) or die;
print $lw "a";
print "6: [$live]\n";
print $lw "b";
print "7: [$live]\n";
close $lw;

{
    local $/ = "2";
    open(my $fh, '<', \"line1\nline2\nline3\n") or die;
    my $first = <$fh>;
    print "8: [$first]\n";
}
{
    local $/ = "";
    open(my $fh, '<', \"a\nb\n\n\nc\n") or die;
    my @para = <$fh>;
    print "9: ", scalar(@para), "\n";
}
