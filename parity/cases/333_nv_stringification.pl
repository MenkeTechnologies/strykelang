use strict;
use warnings;
# Hand-authored parity case: how a floating-point scalar reaches the screen.
#
# Two perl rules interlock here, and each one is invisible without the other:
#
#   1. An NV stringifies through `Gconvert`, i.e. `sprintf "%.15g"`. `%g`
#      switches to scientific notation once the decimal exponent reaches the
#      precision, so the changeover is at 1e15 — `1e14` prints its digits,
#      `1e15` does not.
#   2. `pp_add` / `pp_subtract` / `pp_multiply` call `SvIV_please_nomg` on
#      their operands first, so an NV that happens to be a whole number inside
#      the IV range is added as an integer and the result comes back an IV,
#      which prints its digits. `pp_divide` has no such step.
#
# So `1e15` prints `1e+15` but `1e15 + 0` prints `1000000000000000`, and
# `1e15 / 1` prints `1e+15` again. Neither rule alone predicts all three, which
# is why they are pinned together.
our $PARITY_CASE = 'nv_stringification';

# ---- the %.15g changeover ----
print "1e14:         ", 1e14, "\n";
print "1e15:         ", 1e15, "\n";
print "1e16:         ", 1e16, "\n";
print "999999999999999.0: ", 999999999999999.0, "\n";
print "1234567890123456.0: ", 1234567890123456.0, "\n";
print "1e20:         ", 1e20, "\n";
print "1e-6:         ", 0.000001, "\n";
print "1e-7:         ", 0.0000001, "\n";

# ---- 15 significant digits, and the rounding that comes with them ----
print "0.1+0.2:      ", 0.1 + 0.2, "\n";
print "1/3:          ", 1 / 3, "\n";
print "2/3:          ", 2 / 3, "\n";
print "1e15 through a variable: ", do { my $x = 1e15; $x }, "\n";

# ---- negative zero stringifies as plain 0, unlike C's %.15g ----
my $negzero = -0.0;
print "-0.0:         $negzero\n";

# ---- SvIV_please: integral NV operands make + - * yield integers ----
print "1e15+1:       ", 1e15 + 1, "\n";
print "1e15+0:       ", 1e15 + 0, "\n";
print "0+1e15:       ", 0 + 1e15, "\n";
print "1e15-0:       ", 1e15 - 0, "\n";
print "1e15*1:       ", 1e15 * 1, "\n";
my $acc = 1e15;
$acc += 0;
print "1e15 via +=:  $acc\n";

# ---- ...but division has no such step ----
print "1e15/1:       ", 1e15 / 1, "\n";

# ---- and a non-integral operand keeps the whole expression floating ----
print "2.0+1:        ", 2.0 + 1, "\n";
print "0.5+0.5:      ", 0.5 + 0.5, "\n";
print "1.5+1.5:      ", 1.5 + 1.5, "\n";
print "2.5*2:        ", 2.5 * 2, "\n";
print "3.14*2:       ", 3.14 * 2, "\n";

# ---- concatenation stringifies without any of the arithmetic promotion ----
print "1e15 . '':    ", 1e15 . '', "\n";

# ---- overflow to infinity, and how the specials print ----
print "1e300*1e300:  ", 1e300 * 1e300, "\n";
print "-(1e300*1e300): ", -(1e300 * 1e300), "\n";
my $inf = 1e300 * 1e300;
my $nan = $inf - $inf;
print "nan != nan:   ", ($nan != $nan ? 1 : 0), "\n";
