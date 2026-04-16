use strict;
use warnings;
# @ISA inheritance + SUPER:: dispatch.
package Animal;
sub new   { my ($class, %a) = @_; bless { %a }, $class }
sub speak { my $self = shift; "Generic noise from $self->{name}" }

package Dog;
our @ISA = ('Animal');
sub speak {
    my $self = shift;
    my $base = $self->SUPER::speak();
    "[$base] then bark";
}

package main;
my $d = Dog->new(name => "Rex");
print $d->speak, "\n";
print "isa Dog:    ", ($d->isa("Dog")    ? "y" : "n"), "\n";
print "isa Animal: ", ($d->isa("Animal") ? "y" : "n"), "\n";
print "isa Cat:    ", ($d->isa("Cat")    ? "y" : "n"), "\n";
