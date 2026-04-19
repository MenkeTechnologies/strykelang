package Sub::Util;

# Stub only: `set_subname` / `subname` are implemented natively in stryke (`install_sub_util`).
# CPAN `Sub::Util` uses XS; when `-I …/local/lib/perl5` precedes `vendor/perl`, `require`
# can load the XS shim and leave `set_subname` undefined under stryke — see `run_cpan_topn.sh`.

use strict;
use warnings;

require Exporter;
our @ISA       = qw(Exporter);
our @EXPORT_OK = qw( set_subname subname );
our $VERSION   = "1.62";

1;
