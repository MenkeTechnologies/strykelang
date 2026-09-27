# Scalar::Util is XS in perl; under --compat stryke provides blessed, reftype,
# looks_like_number and refaddr natively (BUG-319).
use strict; use warnings;
use Scalar::Util qw(blessed reftype looks_like_number refaddr);
my $o = bless {}, "Foo";
print blessed($o), "\n";
print defined(blessed({})) ? "def" : "undef", "\n";
print defined(blessed("Foo")) ? "def" : "undef", "\n";
print reftype($o), " ", reftype([]), " ", reftype(\1), " ", reftype(sub {}), "\n";
print defined(reftype("x")) ? "def" : "undef", "\n";
print join(",", map { looks_like_number($_) ? 1 : 0 } (1, "1.5", "1e3", "abc", "", " 12 ", "0x10", "Inf", "nan", undef)), "\n";
my $r = [];
print refaddr($r) == refaddr($r) ? "same" : "diff", "\n";
print refaddr($r) == refaddr([]) ? "same" : "diff", "\n";
print defined(refaddr("x")) ? "def" : "undef", "\n";
print Scalar::Util::blessed($o), "\n";
