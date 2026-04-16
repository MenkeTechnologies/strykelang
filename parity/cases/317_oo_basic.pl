use strict;
use warnings;
# Minimal Perl OO: package, bless, instance method, class method.
package Animal;
sub new {
    my ($class, %args) = @_;
    bless { name => $args{name}, sound => $args{sound} }, $class;
}
sub name  { $_[0]->{name} }
sub speak { my $self = shift; sprintf("%s says %s", $self->{name}, $self->{sound}) }

package main;
my $cow = Animal->new(name => "Bessie", sound => "moo");
my $dog = Animal->new(name => "Rex",    sound => "woof");
print $cow->name, "\n";
print $cow->speak, "\n";
print $dog->speak, "\n";
print "ref: ", ref($cow), "\n";
print "isa Animal: ", ($cow->isa("Animal") ? "y" : "n"), "\n";
print "can speak:  ", ($cow->can("speak")  ? "y" : "n"), "\n";
print "can bark:   ", ($cow->can("bark")   ? "y" : "n"), "\n";
