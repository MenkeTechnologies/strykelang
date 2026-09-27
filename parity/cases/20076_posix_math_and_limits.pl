# POSIX is XS in perl; under --compat stryke provides its math functions and
# <limits.h>/<float.h> constants natively (BUG-319). floor/ceil return NVs,
# fmod keeps the dividend's sign, and the constants take no arguments.
use strict; use warnings;
use POSIX qw(floor ceil fmod pow INT_MAX INT_MIN DBL_MAX UINT_MAX);
print floor(2.5), " ", floor(-2.5), " ", ceil(2.1), " ", ceil(-2.1), "\n";
print fmod(7, 3), " ", fmod(-7, 3), " ", fmod(7.5, 2), "\n";
print pow(2, 10), " ", pow(2, 0.5), "\n";
print INT_MAX, " ", INT_MIN, " ", UINT_MAX, "\n";
print DBL_MAX, "\n";
print POSIX::floor(-0.5), "\n";
print floor(1e20), " ", INT_MAX + 1, " ", POSIX::EXIT_FAILURE, "\n";
use POSIX ();
print POSIX::fabs(-2.5), " ", POSIX::round(2.5), " ", POSIX::round(-2.5), " ", POSIX::trunc(-2.7), "\n";
print POSIX::isnan(9**9**9 / 9**9**9) ? 1 : 0, POSIX::isinf(9**9**9) ? 1 : 0, "\n";
