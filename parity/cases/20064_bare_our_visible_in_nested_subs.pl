# `our $x;` with no initializer declares the package variable for the rest of
# the lexical scope — including sub bodies nested in blocks, BEGIN blocks and
# closures, which stryke checks for `strict vars` at run time. The bare
# declaration must not reset a value an earlier BEGIN stored. File::Basename
# has exactly this shape (`our($Fileparse_fstype)` read from a sub in BEGIN).

use strict;
our ($X, @A, %H);
our $KEPT;
BEGIN { $KEPT = "from BEGIN" }
our $KEPT;

BEGIN {
    my @unused = (1);
    sub setx { my $old = $X; $X = shift; defined $old ? $old : "undef" }
}
{
    my $n = 0;
    sub push_a { push @A, @_; $n += @_; scalar(@A) }
}
my $put = sub { my ($k, $v) = @_; $H{$k} = $v; join ",", sort keys %H };

print setx(5), " ", setx(6), " $X\n";
print push_a(1, 2), " ", push_a(3), " @A\n";
print $put->(b => 2), " ", $put->(a => 1), "\n";
print "$KEPT\n";
