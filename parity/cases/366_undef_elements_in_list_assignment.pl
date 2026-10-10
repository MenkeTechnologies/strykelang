# A scalar undef is a one-element list; only the empty list is zero elements.
my @a = (undef);
print scalar(@a), "\n";
my $u;
my @b = $u;
print scalar(@b), "\n";
my %h;
my @c = $h{missing};
print scalar(@c), "\n";
sub ret_undef { return undef }
sub ret_empty { return }
sub ret_nothing { }
sub ret_list { return (undef, undef) }
my @d = ret_undef();
my @e = ret_empty();
my @f = ret_nothing();
my @g = ret_list();
print scalar(@d), scalar(@e), scalar(@f), scalar(@g), "\n";
my ($x, $y) = ret_undef();
print defined $x ? "d" : "u", defined $y ? "d" : "u", "\n";
my @i = (1, undef, 3);
print scalar(@i), "\n";
@a = ();
@a = (undef);
print scalar(@a), "\n";
@a = ret_undef();
print scalar(@a), "\n";
@a = ret_empty();
print scalar(@a), "\n";
my $n = () = (undef);
print "$n\n";
my $cnt = (my @j = (undef, undef));
print "$cnt\n";
push @a, undef;
push @a, ret_undef();
push @a, ret_empty();
print scalar(@a), "\n";
my @k = map { undef } 1 .. 3;
print scalar(@k), "\n";
my @l = (ret_undef(), ret_empty(), ret_undef());
print scalar(@l), "\n";
print scalar(() = ret_undef()), scalar(() = ret_empty()), "\n";
