# Interpolation of match offsets and hash slices, named captures in s///,
# list slices as slice subscripts, key/value slices, here-doc escapes.
my $s = "foo bar baz";
my ($p, $q) = $s =~ /(\w+) (\w+)/;
print "$p,$q $-[0] $+[0] $-[2] $+[2]\n";

my %h = (a => 1, b => 2, c => 3);
my @k = qw(a b);
print "@h{@k} @h{qw(a c)} @h{'b', 'c'}\n";
my @v = @h{(sort keys %h)[0, 1]};
print "@v\n";
my @w = @h{("c", "a")[0, 1]};
print "@w\n";

my $t = "abc";
$t =~ s/(?<first>.)(.)/$2$+{first}/;
print "$t\n";

my $r = { a => 1, b => 2, c => 3 };
my %kv = $r->%{qw(a c)};
print join(",", map { "$_=$kv{$_}" } sort keys %kv), "\n";
my $ar = [10, 20, 30];
my %iv = $ar->%[0, 2];
print join(",", map { "$_=$iv{$_}" } sort keys %iv), "\n";

my $x = 3;
my @a = (1, 2);
print <<"EOT";
value: $x @{[ $x * 2 ]} @a $a[1] \$x \@a
tab:\tend
EOT
print <<'EOT';
raw: $x \t \\
EOT
print "wide: ", length("\x{263A}ab"), "\n";
