# A list assignment is itself an lvalue list: it can feed another assignment.
my @c = (my $p, my $q) = (5, 6, 7);
print "$p $q @c\n";
my $n = (my ($r, $s) = (1, 2, 3));
print "$n $r $s\n";
my @d = (my ($t, $u) = (9));
print scalar(@d), " $t ", defined $u ? "d" : "u", "\n";
my ($v, $w);
my @e = ($v, $w) = (1, 2, 3, 4);
print "@e\n";
my @f = (my ($head, @tail) = (1, 2, 3));
print "@f | $head | @tail\n";
my $count = () = (my ($g, $h) = (1, 2, 3));
print "$count\n";
((my $m), (my $o)) = (10, 20);
print "$m $o\n";
