use strict;
use warnings;
# Inline case escapes inside interpolated strings: \U \L \u \l \Q \E.
my $name = "alice Smith";
print "\U$name\E done\n";       # ALICE SMITH done
print "\L$name\E done\n";       # alice smith done
print "\u$name done\n";         # Alice Smith  (capitalize first)
print "\l\Uhello\E done\n";     # hELLO        (lower first of UC'd)

my $special = "a.b*c";
my $pat = "\Q$special\E";
print "quoted: $pat\n";          # a\.b\*c

# In a regex
if ("a.b*c" =~ /^\Q$special\E$/) {
    print "regex match\n";
} else {
    print "no\n";
}
