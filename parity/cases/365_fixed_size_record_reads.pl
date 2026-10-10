# `$/ = \N` reads fixed-size records; the last record may be shorter.
my $data = "abcdefghij";
open(my $fh, '<', \$data) or die;
{
    local $/ = \3;
    my @r = <$fh>;
    print scalar(@r), ": ", join("|", @r), "\n";
}
close $fh;

open($fh, '<', \$data) or die;
{
    local $/ = \4;
    my $first = <$fh>;
    my $second = <$fh>;
    print "$first,$second,", ref($/) ? "ref" : "plain", "\n";
}
{
    local $/ = \10;
    my $rest = <$fh>;
    print "[$rest]\n";
    print defined(scalar <$fh>) ? "more" : "eof", "\n";
}
print ref($/) ? "ref" : "plain", " ", $/ eq "\n" ? "newline" : "other", "\n";
close $fh;

my $two = "ab\ncd\nef\n";
open($fh, '<', \$two) or die;
{
    local $/ = \4;
    my @r = <$fh>;
    print scalar(@r), ":", join("|", map { s/\n/N/gr } @r), "\n";
}
