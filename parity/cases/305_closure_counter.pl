use strict;
use warnings;
# Closure capturing a lexical via reference: classic counter pattern.
sub make_counter {
    my $n = 0;
    return sub { ++$n };
}
my $c = make_counter();
print $c->(), "\n";
print $c->(), "\n";
print $c->(), "\n";
my $c2 = make_counter();
print $c2->(), "\n";   # independent counter
print $c->(),  "\n";   # original keeps going
