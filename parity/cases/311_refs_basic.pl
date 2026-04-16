use strict;
use warnings;
# Reference taking and full-form deref ${...} / @{...} / %{...}.
my $s = 42;
my @a = (1, 2, 3);
my %h = (k => "v");
my $sr = \$s;
my $ar = \@a;
my $hr = \%h;
print "scalar deref: ${$sr}\n";          # 42
print "array deref:  @{$ar}\n";           # 1 2 3
print "elem (full):  ${$ar}[1]\n";        # 2
print "hash deref:   ${$hr}{k}\n";        # v
print "ref types:   ", ref($sr), " ", ref($ar), " ", ref($hr), "\n";
# Arrow deref
print "arrow elem:   $ar->[2]\n";         # 3
print "arrow hash:   $hr->{k}\n";         # v
# Anonymous refs
my $aref = [10, 20, 30];
my $href = { a => 1, b => 2 };
print "anon arr:     $aref->[1]\n";       # 20
print "anon hash:    $href->{b}\n";       # 2
# Array of arrays
my @matrix = ([1,2], [3,4], [5,6]);
print "matrix[1][1]: $matrix[1][1]\n";    # 4 (arrow inferred)
# Hash of arrays
my %hoa = (evens => [2,4,6], odds => [1,3,5]);
print "hoa: @{$hoa{evens}}\n";            # 2 4 6
