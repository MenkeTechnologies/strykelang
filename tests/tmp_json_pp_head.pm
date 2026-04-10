package JSON::PP;
use strict;
use Carp ();

my $JSON;

sub encode_json ($) {
    ($JSON ||= __PACKAGE__->new->utf8)->encode(@_);
}

sub decode_json {
    ($JSON ||= __PACKAGE__->new->utf8)->decode(@_);
}

sub to_json($) {
   Carp::croak ("JSON::PP::to_json has been renamed to encode_json.");
}

sub from_json($) {
   Carp::croak ("JSON::PP::from_json has been renamed to decode_json.");
}

1;
