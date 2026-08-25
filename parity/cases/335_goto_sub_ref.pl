# `goto &{EXPR}` / `goto &$coderef` — tail call into a run-time code ref with the
# current @_ and the goto-ing sub's calling context. Exporter is built on this.
sub target { return "target(" . join(",", @_) . ")" }

sub via_block { goto &{ \&target } }
print "1: ", via_block(1, 2), "\n";

my $ref = \&target;
sub via_scalar { goto &$ref }
print "2: ", via_scalar("a"), "\n";

sub via_anon { goto &{ sub { return "anon:" . join("|", @_) } } }
print "3: ", via_anon(7, 8), "\n";

# The target inherits the goto-ing sub's calling context.
sub ctx { return wantarray ? "list" : "scalar" }
sub hop_ctx { goto &{ \&ctx } }
my @l = hop_ctx();
my $s = hop_ctx();
print "4: $l[0] $s\n";

# Chained goto: each hop keeps the same @_.
sub last_hop { return "end(" . join(",", @_) . ")" }
sub mid_hop  { goto &{ \&last_hop } }
sub first_hop { goto &{ \&mid_hop } }
print "5: ", first_hop("x", "y"), "\n";

# `&{EXPR}` where EXPR already holds a code ref calls that ref directly
# (a symbolic name lookup applies only to a string).
my $cr = \&target;
print "6: ", &{$cr}("z"), "\n";
