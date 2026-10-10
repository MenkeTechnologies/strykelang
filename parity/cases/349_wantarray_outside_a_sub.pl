# `wantarray` has no calling context at file scope: it is undef there.
# Inside a sub it reflects the call site, and inside a block eval at file scope
# it reflects the eval's own context.

print "1: ", defined(wantarray) ? "defined" : "undef", "\n";

sub ctx { defined(wantarray) ? (wantarray ? "list" : "scalar") : "void" }
my @l = ctx();
my $s = ctx();
print "2: $l[0] $s\n";
ctx();

sub show { print "3: ", ctx(), "\n" }
show();

sub nested { return ctx() }
my ($n) = nested();
print "4: $n\n";
my $sn = nested();
print "5: $sn\n";
