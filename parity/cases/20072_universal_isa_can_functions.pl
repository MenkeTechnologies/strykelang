# UNIVERSAL::isa / UNIVERSAL::can as functions, and ->can returning the
# method's real code ref.
package Animal; sub new { bless { n => $_[1] }, $_[0] } sub name { $_[0]{n} }
package Dog; our @ISA = ("Animal");
package main;
my $d = Dog->new("rex");
print UNIVERSAL::isa($d, "Animal") ? 1 : 0, UNIVERSAL::isa($d, "HASH") ? 1 : 0,
      UNIVERSAL::isa([], "ARRAY") ? 1 : 0, UNIVERSAL::isa({}, "ARRAY") ? 1 : 0,
      UNIVERSAL::isa("Dog", "Animal") ? 1 : 0, UNIVERSAL::isa(undef, "Dog") ? 1 : 0, "\n";
my $m = $d->can("name");
print $m->($d), "\n";
my $f = UNIVERSAL::can($d, "name");
print $f->($d), "\n";
print defined(UNIVERSAL::can($d, "bark")) ? 1 : 0, "\n";
sub helper { 1 }
print defined(&helper) ? 1 : 0, defined(&nope) ? 1 : 0, defined(&main::nope) ? 1 : 0, "\n";
