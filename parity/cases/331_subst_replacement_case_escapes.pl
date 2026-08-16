use strict;
use warnings;
# Hand-authored parity case: case escapes and `$&` inside an `s///` replacement.
#
# Case 320 already covers `\U`/`\L`/`\u`/`\l`/`\Q` inside an ordinary
# interpolated string, where the escape and the text it acts on are both known
# before the program runs. In a substitution replacement neither is: the escape
# has to be applied to the text `$1` expanded to, separately for every match.
# `s/(\w+)/\u$1/g` on "foo bar" is the smallest construct that can tell the two
# apart — a template-level implementation emits a literal `\u`, and a
# whole-result implementation capitalizes only the first match.
#
# `$&` is here for the same reason: it has no spelling in the Rust `regex`
# crate's expander, so it only survives if the replacement layer rewrites it.
# The `\\` cases pin the other side of that: a literal backslash followed by
# `U` must NOT be read as a case escape.
our $PARITY_CASE = 'subst_replacement_case_escapes';

# ---- one-shot `\u` / `\l`, applied per match ----
my $a = "foo bar baz";
(my $a1 = $a) =~ s/(\w+)/\u$1/g;
print "ucfirst each:  $a1\n";
my $b = "FOO BAR";
(my $b1 = $b) =~ s/(\w+)/\l$1/g;
print "lcfirst each:  $b1\n";

# ---- ranged `\U` / `\L`, and `\E` ending the range mid-replacement ----
my $c = "foo bar";
(my $c1 = $c) =~ s/(\w+)/\U$1/g;
print "uc each:       $c1\n";
(my $c2 = $c) =~ s/(\w+)/\U$1\E!/g;
print "uc then plain: $c2\n";
my $d = "a-b";
(my $d1 = $d) =~ s/(\w)-(\w)/\U$1\E-\u$2/;
print "mixed escapes: $d1\n";

# ---- only the first match is touched without /g ----
my $e = "one two";
(my $e1 = $e) =~ s/(\w+)/\U$1/;
print "no /g:         $e1\n";

# ---- `$&` — the whole match ----
my $f = "xy";
(my $f1 = $f) =~ s/x/[$&]/;
print "whole match:   $f1\n";
(my $f2 = $f) =~ s/x/\U$&/;
print "uc whole:      $f2\n";

# ---- a literal backslash is not a case escape ----
my $g = "x";
(my $g1 = $g) =~ s/x/a\\Ub/;
print "literal bs+U:  $g1\n";
(my $g2 = $g) =~ s/x/a\\b/;
print "literal bs:    $g2\n";
(my $g3 = $g) =~ s/x/\\/;
print "lone bs:       $g3\n";
(my $g4 = $g) =~ s/x/\\n/;
print "bs then n:     $g4\n";
(my $g5 = $g) =~ s{x}{a\\Ub};
print "braced delim:  $g5\n";

# ---- numbered back-references still work alongside all of the above ----
my $h = "ab";
(my $h1 = $h) =~ s/(a)(b)/$2$1/;
print "swap:          $h1\n";
{
    # `\1` in a replacement is a numbered back-reference like `$1`; perl warns
    # "better written as $1" under `use warnings`, and that diagnostic is a
    # different surface from what this case pins.
    no warnings 'syntax';
    (my $h2 = $h) =~ s/(a)/\1\1/;
    print "backslash ref: $h2\n";
}
my $i = "foo";
(my $i1 = $i) =~ s/(f)(o)(o)/${3}${2}${1}/;
print "braced refs:   $i1\n";

# ---- /r returns the modified copy and leaves the target alone ----
my $j = "hello world";
my $j1 = $j =~ s/(\w+)/\u$1/gr;
print "r flag:        $j1 | $j\n";

# ---- /e evaluates code, so escapes there are the code's business ----
my $k = "abc";
(my $k1 = $k) =~ s/b/"<" . uc($&) . ">"/e;
print "e flag:        $k1\n";

# ---- the return value is still the match count ----
my $l = "aaa";
my $n = ($l =~ s/(a)/\u$1/g);
print "count:         $n $l\n";
