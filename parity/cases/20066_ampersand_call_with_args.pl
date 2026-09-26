# `&name(ARGS)` calls the sub with exactly ARGS; only bare `&name` passes the
# caller's @_ along. The parenthesized list used to parse as a separate term,
# so the call ran with no arguments.

sub show { "F(@_)" }
my $x = &show(2, 3);
print "$x\n";
print "a", &show(2), "\n";
print &show(4), "\n";
sub pass_along { &show }
sub empty_args { &show() }
print pass_along(7), " ", empty_args(8), "\n";
my @l = (&show(1), &show(2));
print scalar(@l), "\n";
print &defined_later(3), "\n";
sub defined_later { "L$_[0]" }
sub proto($) { "P(@_)" }
print &proto(1, 2), "\n";
package Z;
sub zz { "z@_" }
package main;
print &Z::zz(9), "\n";
