# Stub only: `List::Util` is implemented natively in stryke (`src/list_util.rs`) so every
# EXPORT_OK name matches Perl 5 without loading XS. This file exists for `%INC` / tooling.

package List::Util;

# Version must satisfy dual-life / JSON::PP checks (Scalar::Util compares against List::Util).
our $VERSION = '1.70';

1;
