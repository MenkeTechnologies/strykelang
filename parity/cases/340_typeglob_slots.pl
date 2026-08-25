# Typeglobs are first-class values with named slots.
#
# A glob names a symbol-table entry rather than snapshotting it, so every slot
# selector and every deref sees the live variable. `Carp::trusts_directly` walks
# exactly this shape: `ref \$stash->{$var} eq 'GLOB' && *{$stash->{$var}}{ARRAY}`.

our @list = (1, 2, 3);
our %map  = (a => 1);
our $val  = 9;
sub code { 42 }

# A glob stringifies with its sigil and package.
print "1: ", "" . *main::list, "\n";

# `ref` on a globref is GLOB; the glob itself is not a reference.
my $gr = \*main::list;
print "2: ", ref($gr), " [", ref(*main::list), "]\n";
print "3: ", ("$gr" =~ /^GLOB\(0x[0-9a-f]+\)$/ ? "globstr" : "other"), "\n";

# `*NAME{THING}` selects one slot and hands back a reference to it.
print "4: ", ref(*main::list{ARRAY}), " ", ref(*main::map{HASH}), "\n";
print "5: ", ref(*main::val{SCALAR}), " ", ref(*main::code{CODE}), "\n";
print "6: ", join(",", @{ *main::list{ARRAY} }), "\n";
print "7: ", ${ *main::val{SCALAR} }, " ", *main::code{CODE}->(), "\n";

# An empty slot is undef, and asking never creates the variable.
print "8: ", (defined *main::never_set{ARRAY} ? "def" : "undef"), "\n";
print "9: ", (defined *main::never_set{HASH}  ? "def" : "undef"), "\n";

# NAME / PACKAGE are plain strings.
print "10: ", *main::list{NAME}, " ", *main::list{PACKAGE}, "\n";

# Dereferencing a glob reaches the matching slot by name, whether or not the
# slot was ever filled.
print "11: ", join(",", @{ *main::list }), " ", ${ *main::val }, "\n";
print "12: ", join(",", map { "$_=$map{$_}" } sort keys %{ *main::map }), "\n";

# The same through a globref.
print "13: ", join(",", @{ *$gr{ARRAY} }), "\n";

# Slots track later writes — a glob is the entry, not a copy.
push @list, 4;
print "15: ", join(",", @{ *main::list{ARRAY} }), "\n";

# Package globs work the same way, and a package's own name qualifies a bare glob.
package Other;
our @things = ("x", "y");
sub which { return *things{PACKAGE} }
package main;
print "16: ", join(",", @{ *Other::things{ARRAY} }), " ", Other::which(), "\n";

# The stash holds globs, which is what makes `$stash->{NAME}` usable as one.
my $stash = \%Other::;
print "17: ", ref(\$stash->{things}), "\n";
print "18: ", (*{ $stash->{things} }{ARRAY} ? "hasarray" : "noarray"), "\n";
print "19: ", join(",", @{ $stash->{things} }), "\n";
