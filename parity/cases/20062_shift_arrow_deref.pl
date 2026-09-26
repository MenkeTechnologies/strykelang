# `shift->[0]`, `shift->{key}` and `shift->method` are the standard Perl OO
# accessor idiom. With no operand the shift/pop operand is the implicit @_,
# and the `->` that follows is a postfix dereference of the shifted value.

package Point;
sub new { my $class = shift; bless { x => shift, y => shift }, $class }
sub x { shift->{x} }
sub y { shift->{y} }
sub sum { my $self = shift; $self->x + $self->y }
sub twice { shift->sum * 2 }

package main;
my $p = Point->new(3, 4);
print $p->x, " ", $p->y, " ", $p->sum, " ", $p->twice, "\n";

sub first_of { shift->[0] }
sub last_of { pop->[-1] }
print first_of([7, 8]), " ", last_of([1], [5, 6]), "\n";

my @q = ({ n => 1 }, { n => 2 });
print shift(@q)->{n}, " ", scalar(@q), "\n";

# shift followed by an operator still ends the operand.
sub or_default { shift || "dflt" }
print or_default(0), " ", or_default("v"), "\n";
