# Runtime errors caught by `eval` carry perl's exact text: the script name, the
# line, a trailing newline, and the standard wording for each failure.

my $file = __FILE__;
sub show { my $m = $@; $m =~ s/\Q$file\E/FILE/g; print $_[0], ": ", $m }

eval { my $z = 0; my $r = 1 / $z };
show(1);
eval { my $z = 0; my $r = 1 % $z };
show(2);
eval { my $u; $u->method };
show(3);
eval { my $r = {}; $r->method };
show(4);
eval { my $r = [1]; $r->method };
show(5);
{ package Known; sub new { bless {}, shift } }
eval { Known->new->nope };
show(6);
eval { my $e = ""; $e->method };
show(7);
eval { nosuchfunction(1) };
show(8);
eval { my $x = sqrt(-4) };
show(9);
eval { my $x = log(0) };
show(10);
eval { die "plain\n" };
show(11);
eval { die "no newline" };
show(12);
