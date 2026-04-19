package Scalar::Util;

# Minimal stub — core `Scalar/Util.pm` pulls XS and `builtin::` ops that stryke does not parse yet.
# Subroutines are registered natively by `list_util::install_scalar_util` (see interpreter startup).

use strict;
use warnings;

require Exporter;
our @ISA       = qw(Exporter);
our @EXPORT_OK = qw(
  blessed refaddr reftype weaken unweaken isweak

  dualvar isdual isvstring looks_like_number openhandle readonly set_prototype
  tainted
);
our $VERSION = "1.68";

1;
