# An anonymous sub written and invoked in one statement: `sub { ... }->(ARGS)`
# at the start of a statement must parse the postfix call, not stop at the
# closing brace. Also through a string eval, whose text is all statements.

sub { print "direct: @_\n" }->("a", "b");
sub { print "modifier\n" }->() if 1;
sub { print "never\n" }->() unless 1;

my $r = eval q{sub { $_[0] * 2 }->(21)};
print defined $r ? "eval: $r\n" : "eval failed: $@";

# Chained: the call returns a coderef that is invoked again.
sub { my $n = shift; sub { print "chained: ", $n + shift, "\n" } }->(40)->(2);

# A plain anonymous sub statement is still just a value, not a call.
sub { print "not called\n" };
print "done\n";
