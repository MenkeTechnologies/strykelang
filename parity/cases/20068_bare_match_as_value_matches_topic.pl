# A bare `/re/` (or `m//`) used as a value matches `$_`; only `qr//` builds a
# regex object.
$_ = "x1y2";
my ($a, $b) = /(\d)\D(\d)/;
print "$a$b\n";
my @d = /\d/g;
print "@d\n";
my $n = () = /\d/g;
print "$n\n";
my $ok = /y/;
print "ok=$ok\n";
my @l = map { /(\d)/ } qw(a1 b2 c3);
print "@l\n";
my @m = map { my @c = /(\w)(\d)/; @c } qw(a1 b2);
print "@m\n";
my @s = split /\d/, "a1b2c";
print "@s\n";
my $re = qr/(\d)/;
my ($c) = "ab7" =~ $re;
print "$c\n";
my @both = "k=v" =~ qr/(\w)=(\w)/;
print "@both\n";
for my $l ("a", "START", "b", "END", "c") { $_ = $l; print "$_ " if /START/ .. /END/ }
print "\n";
