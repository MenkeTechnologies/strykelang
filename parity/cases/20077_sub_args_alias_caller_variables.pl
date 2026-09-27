# @_ aliases the caller's arguments (BUG-312): writing $_[N] — assignment,
# ++, s///, .=, chop, a foreach over @_ — changes the variable passed. The
# alias survives a `shift` (indices move down), ends when @_ is rebuilt,
# chains through `$_[N]` passed to another sub, works for method calls (where
# $_[0] is the invocant), and reaches subs of a module
# (File::Basename::dirname strips the trailing slash through
# _strip_trailing_sep($dirname)).
use strict; use warnings;
use File::Basename qw(dirname);
sub f { $_[0]++; $_[1] =~ s/a/b/; $_[2] = 7; $_[3] .= "x"; chop $_[4]; }
my ($a1, $b1, $c1, $d1, $e1) = (1, "a", 3, "d", "ee");
f($a1, $b1, $c1, $d1, $e1);
print "$a1 $b1 $c1 $d1 $e1\n";
sub method_like { my $self = shift; $_[0] = "set:$self"; }
my $out = ""; method_like("obj", $out); print "$out\n";
sub late_shift { $_[0] = "first"; shift; $_[0] = "second"; }
my ($m, $n) = ("m", "n"); late_shift($m, $n); print "$m $n\n";
sub no_write { my ($v) = @_; $v .= "!"; return $v }
my $keep = "k"; print no_write($keep), " $keep\n";
sub rebuilt { @_ = ("z"); $_[0] = "q"; }
my $r = "r"; rebuilt($r); print "$r\n";
sub neg { $_[-1] = "last" }
my ($u, $w) = ("u", "w"); neg($u, $w); print "$u $w\n";
our $g = "g0"; sub via { $_[0] = "via" } via($g); print "$g\n";
sub loop_alias { for (@_) { s/^\s+//; } }
my ($s1, $s2) = ("  a", " b"); loop_alias($s1, $s2); print "[$s1][$s2]\n";
sub inner { $_[0] .= "!" } sub outer { inner($_[0]); inner($_[1]) }
my ($x1, $x2) = ("a", "b"); outer($x1, $x2); print "$x1 $x2\n";
sub nested_inner { $_[0] = "inner" }
sub nested_outer { my $t = "t"; nested_inner($t); $_[0] = "outer:$t"; }
my $q = "q"; nested_outer($q); print "$q\n";
sub ret { $_[0] = 9; return 1 } my $z = 0; my $res = ret($z) + 1; print "$z $res\n";
sub cond { $_[0] = "c" if $_[1] } my $cv = "orig"; cond($cv, 0); print "$cv\n"; cond($cv, 1); print "$cv\n";
print dirname("/a/b"), "|", dirname("/a/b/"), "|", dirname("a"), "|", dirname("/"), "\n";
package Obj;
sub new { bless {}, shift }
sub meth { $_[1] = "m"; $_[2] .= "2" }
sub twice { my $self = shift; $self->meth($_[0], $_[1]) }
package main;
my $o = Obj->new; my ($mv, $mu) = ("v", "u"); $o->meth($mv, $mu); print "$mv $mu\n";
my ($tv, $tu) = ("v", "u"); $o->twice($tv, $tu); Obj->meth($tv, $tu); print "$tv $tu\n";
