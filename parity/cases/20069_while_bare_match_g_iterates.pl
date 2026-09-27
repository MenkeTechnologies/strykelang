# `/x/g` in a condition resumes at pos($_) instead of re-matching from 0.
$_ = "abc";
while (/(\w)/g) { print $1, pos(), " " }
print "\n";
$_ = "aXbXcX";
my $c = 0;
$c++ while /X/g;
print "$c\n";
$_ = "hello";
/l+/g;
print pos(), "\n";
