# An inner `my` ends with its block; `return @a` is the length in scalar context.
my $x = 7;
{ my $x = 5; print "in=$x\n"; }
print "out=$x\n";
for my $i (1, 2) { my $x = $i * 10; }
print "loop=$x\n";
if (1) { my $x = 99; print "if=$x\n"; }
print "after=$x\n";
my $i = 3;
for my $i (1 .. 2) { }
for (my $i = 0; $i < 2; $i++) { }
print "i=$i\n";

sub arr { my @a = (4, 5, 6); return @a }
sub tail { my @a = (4, 5, 6); @a }
sub hsh { my %h = (a => 1, b => 2); %h }
sub lst { return (4, 5, 6) }
sub cnt { my @a = (7, 8); return (@a, 9) }
my $c = arr();
my $t = tail();
my $l = lst();
my ($f) = lst();
my @all = arr();
print "$c $t $l $f ", scalar(@all), "\n";
my $n = () = lst();
print "$n ", scalar(my @k = hsh()), "\n";
print scalar(cnt()), "\n";
