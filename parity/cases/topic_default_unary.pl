use strict;
use warnings;
use feature 'say';
# Hand-authored parity case: topic-default unary builtins must accept both
# bare form (`f`) and empty-parens form (`f()`), defaulting to `$_` either way.
# Perl 5 docs (perldoc -f length): "If EXPR is omitted, returns the length of $_."
# Same applies to: uc, lc, ucfirst, lcfirst, abs, ord, chr, hex, oct, defined,
# ref, chomp, chop, length.
our $PARITY_CASE = 'topic_default_unary';

# ---- length ----
$_ = "hello";
print "length bare:    ", length, "\n";
print "length parens:  ", length(), "\n";
print "length expl:    ", length($_), "\n";

# ---- uc / lc ----
$_ = "Mixed";
print "uc bare:        ", uc, "\n";
print "uc parens:      ", uc(), "\n";
print "lc bare:        ", lc, "\n";
print "lc parens:      ", lc(), "\n";

# ---- ucfirst / lcfirst ----
$_ = "hello world";
print "ucfirst bare:   ", ucfirst, "\n";
print "ucfirst parens: ", ucfirst(), "\n";
$_ = "HELLO WORLD";
print "lcfirst bare:   ", lcfirst, "\n";
print "lcfirst parens: ", lcfirst(), "\n";

# ---- abs ----
$_ = -42;
print "abs bare:       ", abs, "\n";
print "abs parens:     ", abs(), "\n";

# ---- ord / chr ----
$_ = "A";
print "ord bare:       ", ord, "\n";
print "ord parens:     ", ord(), "\n";
$_ = 65;
print "chr bare:       ", chr, "\n";
print "chr parens:     ", chr(), "\n";

# ---- hex / oct ----
$_ = "ff";
print "hex bare:       ", hex, "\n";
print "hex parens:     ", hex(), "\n";
$_ = "0755";
print "oct bare:       ", oct, "\n";
print "oct parens:     ", oct(), "\n";

# ---- defined ----
$_ = 0;
print "defined parens (0):     ", (defined() ? "y" : "n"), "\n";
$_ = undef;
print "defined parens (undef): ", (defined() ? "y" : "n"), "\n";
$_ = "x";
print "defined bare (x):       ", (defined ? "y" : "n"), "\n";

# ---- ref ----
$_ = [];
print "ref bare (aref):    ", ref, "\n";
print "ref parens (aref):  ", ref(), "\n";
$_ = {};
print "ref parens (href):  ", ref(), "\n";
$_ = "scalar";
print "ref parens (str):   ", "[", ref(), "]\n";

# ---- chomp / chop ----
$_ = "trailing\n";
chomp;
print "chomp bare:     [$_]\n";
$_ = "trailing\n";
chomp();
print "chomp parens:   [$_]\n";
$_ = "abcdef";
chop;
print "chop bare:      [$_]\n";
$_ = "abcdef";
chop();
print "chop parens:    [$_]\n";

# ---- print / say / printf (variadic) — both bare and `()` default to `$_` ----
# `perldoc -f print`: "If no arguments are given, prints $_."
$_ = "topic\n";
print;          # prints $_ (no newline added)
print();        # also prints $_  (Perl convention, NOT no-op)
$_ = "say-topic";
say;            # say + newline
say();          # also $_ + newline
$_ = "printf-topic";
printf;         # uses $_ as format string → prints "printf-topic"
printf();       # same — empty parens default to $_

# ---- combined: function composition with empty parens ----
$_ = "  Hello  ";
my $s = uc(lc());           # lc()→"  hello  ", uc(...)→"  HELLO  "
print "compose:        [$s]\n";
