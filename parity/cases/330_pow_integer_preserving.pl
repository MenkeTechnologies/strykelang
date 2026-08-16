use strict;
use warnings;
# Hand-authored parity case: Perl's integer-preserving `**`.
#
# `pp_pow` (perl `pp.c`, the PERL_PRESERVE_IVUV block) does NOT decide
# "integer or NV" by attempting the multiplication and watching for overflow.
# It decides up front from the *bit width* of the base: with `highbit` the
# smallest `h` where `|base| < 2**h`, integer arithmetic runs only when
# `power * highbit <= 64`. Everything else goes to C `pow()` and comes back an
# NV, which then stringifies through `%.15g`.
#
# That rule is the whole point of this case: it is why `10**16` prints its
# exact digits (highbit 4, 4*16 == 64) while `2**53` — a smaller number, and
# exactly representable — prints in scientific notation (highbit 2, 2*53 ==
# 106). A corpus that only exercised small powers cannot tell the two apart,
# and an implementation that "promotes to a bigint when it overflows" agrees
# with perl on every small case and diverges on every large one.
our $PARITY_CASE = 'pow_integer_preserving';

# ---- inside the integer window: exact digits ----
print "2**10:      ", 2**10, "\n";
print "3**5:       ", 3**5, "\n";
print "2**31:      ", 2**31, "\n";
print "2**32:      ", 2**32, "\n";
print "10**16:     ", 10**16, "\n";

# ---- outside it: C pow(), so `%.15g` scientific form ----
# 2**53 is an exact power of two well inside the IV range; only the highbit
# rule explains why it is not printed as 9007199254740992.
print "2**53:      ", 2**53, "\n";
print "2**62:      ", 2**62, "\n";
print "2**64:      ", 2**64, "\n";
print "10**17:     ", 10**17, "\n";
print "10**20:     ", 10**20, "\n";
print "7**22:      ", 7**22, "\n";

# ---- negative base: the sign rides on whether the power is odd ----
my $neg2 = -2;
my $neg3 = -3;
print "(-2)**3:    ", $neg2**3, "\n";
print "(-2)**2:    ", $neg2**2, "\n";
print "(-2)**63:   ", $neg2**63, "\n";
print "(-2)**64:   ", $neg2**64, "\n";
print "(-3)**41:   ", $neg3**41, "\n";

# ---- zero base and zero power ----
print "0**0:       ", 0**0, "\n";
print "0**5:       ", 0**5, "\n";
print "5**0:       ", 5**0, "\n";

# ---- non-integer or negative exponent: always floating point ----
print "2**-2:      ", 2**-2, "\n";
print "2**0.5:     ", 2**0.5, "\n";
print "4**0.5:     ", 4**0.5, "\n";

# ---- right associativity, and the overflow that must reach Inf, not a bigint ----
# 9**9**9 is 9**387420489. Computing that exactly would be a multi-hundred-
# megabyte integer; perl hands it to pow() and gets Inf immediately.
print "2**3**2:    ", 2**3**2, "\n";
print "9**9**9:    ", 9**9**9, "\n";
print "-(9**9**9): ", -(9**9**9), "\n";
