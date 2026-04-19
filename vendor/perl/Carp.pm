package Carp;

# Minimal pure-Perl stub — core `Carp.pm` uses `use warnings`, `${^WARNING_BITS}`, and
# `$::{pkg}` introspection that stryke does not parse yet. CPAN modules (e.g. JSON::PP)
# only need croak/carp/confess/cluck entry points.

use strict;
use warnings;

our $VERSION = '1.50';
our $CarpLevel = 0;

sub croak {
    die join('', @_);
}

sub carp {
    warn join('', @_);
}

sub confess {
    croak(@_);
}

sub cluck {
    carp(@_);
}

1;
