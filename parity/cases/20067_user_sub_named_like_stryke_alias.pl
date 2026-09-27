# A Perl program's `sub t` / `sub pr` must be callable under --compat even
# though stryke parses `t` (thread macro) and `pr` (print alias) specially.
# The control `tt` never collided.
our $g = 1;
sub shw { print "g=$g\n" }
sub t { local $g = 2; shw() }
sub tt { local $g = 3; shw() }
t();
tt();
shw();
sub pr { "P(@_)" }
print pr(1, 2), "\n";
sub sp { join "-", @_ }
print sp(split /,/, "x,y"), "\n";
