use strict;
use warnings;
# Postfix statement modifiers (single — chaining is not legal in Perl).
my @nums = (1..10);
print "$_\n" for grep { $_ % 2 == 0 } @nums;     # 2 4 6 8 10

# postfix `if`
print "yes\n" if 1;
print "no\n"  if 0;
print "u\n"   unless 0;

# postfix `while`
my $i = 3;
print "i=$i\n", $i-- while $i > 0;

# postfix `foreach`
my $sum = 0;
$sum += $_ foreach (1, 2, 3, 4);
print "sum=$sum\n";

# postfix `until`
my $j = 0;
$j++ until $j >= 3;
print "j=$j\n";
