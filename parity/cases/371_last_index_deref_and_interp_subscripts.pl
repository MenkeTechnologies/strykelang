use strict;
use warnings;

# $#{EXPR} / $#$ref — last index through a reference, in code and in strings.
my $r = [10, 20, 30];
my $h = { list => [1, 2, 3, 4], a => 1, b => 2 };
print $#$r, " ", $#{$r}, " ", $#{ $h->{list} }, " ", $#{[]}, "\n";
for my $i (0 .. $#$r) { print $r->[$i], $i == $#{$r} ? "\n" : "," }
print "last=$#$r braced=$#{$r} nested=$#{ $h->{list} } anon=$#{[1,2]}\n";

# $$ref subscripts and slices through references inside strings.
my $x = 7;
my $sref = \$x;
print "val=$$sref $$r[0] $$r[-1] $$h{a} $$h{list}[1] @$r[0,1] @{$r}[1,2] @{$r}[0..$#$r] @{$h}{qw(a b)} @$h{'b'}\n";

# Non-bareword hash keys and arithmetic indices inside strings.
my %kv = (k => "v", "a b" => 2, "c-d" => 3, -neg => "n", 5 => "five");
my @a = (1, 2, 3);
my $i = 1;
my $key = "a";
print "$kv{'k'} $kv{ k } $kv{'a b'} $kv{'c-d'} $kv{-neg} $kv{5} $kv{$key . ' b'}\n";
print qq{$kv{"k"} $kv{"a b"} $kv{$key . " b"}\n};
print "$a[0+1] $a[$i+1] $a[$i - 1] $a[-1] $a[$#a] $a[ 1 ]\n";
my %named = ('x y' => 9);
my $deep = { k => \%named };
print "$deep->{'k'}{'x y'} $$deep{k}{'x y'}\n";
