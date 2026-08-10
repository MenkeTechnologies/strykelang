use strict;
use warnings;
# Hand-authored parity case: autovivification of intermediate levels in a
# subscript chain.
#
# Perl 5 rule (perldoc perlref, "Using References"): any expression used as a
# container — i.e. anything to the LEFT of a subscript — springs into existence
# as the reference kind that subscript requires. Only the FINAL subscript is
# context-dependent: an assignment creates it, an rvalue read or `exists` does
# not. That single rule covers lvalue chains, rvalue chains, chains through a
# reference, and `exists`, which is why they are all exercised together here.
#
# The corpus previously had no case where a chain's intermediate level was
# missing: every nested subscript in it was either a slice of an already-built
# anonymous list (`@{[...]}[0,2]`) or indexed a container built literally. A
# construct present only in that role cannot detect a missing-autovivification
# bug, so this case supplies the role that was absent.
our $PARITY_CASE = 'autovivification';

# ---- lvalue: nested hash ----
my %h;
$h{a}{b} = 1;
print "hash lvalue:      ", ref $h{a}, " ", $h{a}{b}, "\n";

# ---- lvalue: hash of array ----
my %g;
$g{a}[0] = 5;
print "hash-of-array:    ", ref $g{a}, " ", $g{a}[0], "\n";

# ---- lvalue: mixed three-level chain ----
# Each level takes the kind of the subscript that FOLLOWS it, so `$m{a}` is an
# ARRAY (next subscript is `[0]`) while `$m{a}[0]` is a HASH (next is `{c}`).
my %m;
$m{a}[0]{c} = 7;
print "mixed 3-level:    ", ref $m{a}, " ", ref $m{a}[0], " ", $m{a}[0]{c}, "\n";

# ---- lvalue: array of arrays ----
my @A;
$A[0][1] = 3;
print "array-of-array:   ", ref $A[0], " ", $A[0][1], "\n";

# ---- lvalue: through an existing reference ----
my $r = {};
$r->{a}{b} = 2;
print "through ref:      ", ref $r->{a}, " ", $r->{a}{b}, "\n";

# ---- lvalue: the reference variable itself springs into existence ----
my $fresh;
$fresh->{k} = 'v';
print "undef scalar:     ", ref $fresh, " ", $fresh->{k}, "\n";
my $fresh_a;
$fresh_a->[2] = 'z';
print "undef scalar arr: ", ref $fresh_a, " ", scalar(@$fresh_a), " ", $fresh_a->[2], "\n";

# ---- rvalue: intermediates ARE created, the final level is NOT ----
my %rv;
my $read = $rv{a}{b};
printf "rvalue read:      a=%s ref=%s b_exists=%d val=%s\n",
    (exists $rv{a} ? 'yes' : 'no'),
    (ref $rv{a} or 'none'),
    ((exists $rv{a} and exists $rv{a}{b}) ? 1 : 0),
    (defined $read ? $read : 'undef');

# ---- rvalue: a plain one-level read does NOT create the key ----
my %single;
my $miss = $single{a};
print "rvalue 1-level:   exists=", (exists $single{a} ? 1 : 0), "\n";

# ---- rvalue: deref of an undef scalar still vivifies the scalar ----
my $ru;
my $ru_read = $ru->{a};
print "rvalue undef ref: ", (defined $ru ? ref $ru : 'undef'), "\n";

# ---- rvalue: three-level chain builds both intermediates ----
my %d;
my $deep = $d{a}{b}{c};
printf "rvalue 3-level:   a=%s b=%s c_exists=%d\n",
    (ref $d{a} or 'none'),
    (ref $d{a}{b} or 'none'),
    ((exists $d{a} and exists $d{a}{b} and exists $d{a}{b}{c}) ? 1 : 0);

# ---- exists: vivifies every intermediate, never the final key ----
my %e;
my $q = exists $e{a}{b};
printf "exists 2-level:   a=%s ref=%s result=%d\n",
    (exists $e{a} ? 'yes' : 'no'),
    (ref $e{a} or 'none'),
    ($q ? 1 : 0);

my %e3;
my $q3 = exists $e3{x}{y}{z};
printf "exists 3-level:   x=%s y=%s result=%d\n",
    (ref $e3{x} or 'none'),
    (ref $e3{x}{y} or 'none'),
    ($q3 ? 1 : 0);

my $er;
my $qr = exists $er->{a}{b};
printf "exists thru ref:  r=%s a=%s result=%d\n",
    (ref $er or 'none'),
    (defined $er->{a} ? ref $er->{a} : 'undef'),
    ($qr ? 1 : 0);

# ---- an existing value is never clobbered by a later chain write ----
my %keep;
$keep{a}{first} = 'one';
$keep{a}{second} = 'two';
print "no clobber:       ", join(',', map { "$_=$keep{a}{$_}" } sort keys %{ $keep{a} }), "\n";

# ---- vivification extends an array with undef gaps, like any element write ----
my %gap;
$gap{list}[3] = 'end';
print "gap fill:         len=", scalar @{ $gap{list} },
      " defined0=", (defined $gap{list}[0] ? 1 : 0),
      " last=", $gap{list}[3], "\n";

# ---- deeper mixed chain, built entirely by one assignment ----
my %deepmix;
$deepmix{a}[1]{b}[0] = 'leaf';
print "deep mixed:       ", ref $deepmix{a}, " ", ref $deepmix{a}[1],
      " ", ref $deepmix{a}[1]{b}, " ", $deepmix{a}[1]{b}[0], "\n";
