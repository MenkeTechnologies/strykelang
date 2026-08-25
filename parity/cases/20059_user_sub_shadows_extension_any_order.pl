# Under --compat a user sub beats a stryke extension of the same name, and
# Perl resolves calls against the whole file rather than the text above them.
#
# Two separate failures were possible. A call placed BEFORE the declaration
# was rejected outright ("`f` is a stryke extension (disabled by --compat)"),
# because the single-pass parser only learned a name at its declaration. And
# even after the declaration, the extensions that parse into a dedicated AST
# node (`f`, `d`, `l`, `c` …) kept dispatching to the extension, since that
# node carries no name for the runtime to resolve against the user's sub.

# Forward reference: called before it is declared.
print forward_probe(), "\n";

sub forward_probe { return f() . c() . d() }

# Names that collide with stryke extensions.
sub f { return "F" }
sub c { return "C" }
sub d { return "D" }
sub l { return "L" }

print f(), c(), d(), l(), "\n";

# Mutual recursion through colliding names, declared after first use.
sub outer { return inner() + 1 }
sub inner { return f_count() }
sub f_count { return 6 }
print outer(), "\n";

# The shadowing sub behaves like any other Perl sub: flattened @_, list
# return, usable as a list-slice source.
sub f_list { return (10, 20, 30) }
print( (f_list())[1], "\n");
print scalar(f_args(1, 2, 3)), "\n";
sub f_args { return scalar(@_) }

# A colliding name still works as a method and through a code ref.
my $ref = \&f;
print $ref->(), "\n";
