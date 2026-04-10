package JSON::PP;

# JSON-2.0

use 5.008;
use strict;

use Exporter ();
BEGIN { our @ISA = ('Exporter') }

use overload ();
use JSON::PP::Boolean;

use Carp ();
use Scalar::Util qw(blessed reftype refaddr);
#use Devel::Peek;

our $VERSION = '4.16';

our @EXPORT = qw(encode_json decode_json from_json to_json);

# instead of hash-access, i tried index-access for speed.
# but this method is not faster than what i expected. so it will be changed.

use constant P_ASCII                => 0;
use constant P_LATIN1               => 1;
use constant P_UTF8                 => 2;
use constant P_INDENT               => 3;
use constant P_CANONICAL            => 4;
use constant P_SPACE_BEFORE         => 5;
use constant P_SPACE_AFTER          => 6;
use constant P_ALLOW_NONREF         => 7;
use constant P_SHRINK               => 8;
use constant P_ALLOW_BLESSED        => 9;
use constant P_CONVERT_BLESSED      => 10;
use constant P_RELAXED              => 11;

use constant P_LOOSE                => 12;
use constant P_ALLOW_BIGNUM         => 13;
use constant P_ALLOW_BAREKEY        => 14;
use constant P_ALLOW_SINGLEQUOTE    => 15;
use constant P_ESCAPE_SLASH         => 16;
use constant P_AS_NONBLESSED        => 17;

use constant P_ALLOW_UNKNOWN        => 18;
use constant P_ALLOW_TAGS           => 19;

use constant USE_B => $ENV{PERL_JSON_PP_USE_B} || 0;
use constant CORE_BOOL => defined &builtin::is_bool;

my $invalid_char_re;

BEGIN {
    $invalid_char_re = "[";
    for my $i (0 .. 0x01F, 0x22, 0x5c) { # '/' is ok
        $invalid_char_re .= quotemeta chr utf8::unicode_to_native($i);
    }

    $invalid_char_re = qr/$invalid_char_re]/;
}

BEGIN {
    if (USE_B) {
        require B;
    }
}

BEGIN {
    my @xs_compati_bit_properties = qw(
            latin1 ascii utf8 indent canonical space_before space_after allow_nonref shrink
            allow_blessed convert_blessed relaxed allow_unknown
            allow_tags
    );
    my @pp_bit_properties = qw(
            allow_singlequote allow_bignum loose
            allow_barekey escape_slash as_nonblessed
    );

    for my $name (@xs_compati_bit_properties, @pp_bit_properties) {
        my $property_id = 'P_' . uc($name);

        eval qq/
            sub $name {
                my \$enable = defined \$_[1] ? \$_[1] : 1;

                if (\$enable) {
                    \$_[0]->{PROPS}->[$property_id] = 1;
                }
                else {
                    \$_[0]->{PROPS}->[$property_id] = 0;
                }

                \$_[0];
            }

            sub get_$name {
                \$_[0]->{PROPS}->[$property_id] ? 1 : '';
            }
        /;
    }

}



# Functions

my $JSON; # cache

sub encode_json ($) { # encode
    ($JSON ||= __PACKAGE__->new->utf8)->encode(@_);
}


sub decode_json { # decode
    ($JSON ||= __PACKAGE__->new->utf8)->decode(@_);
}

# Obsoleted

sub to_json($) {
   Carp::croak ("JSON::PP::to_json has been renamed to encode_json.");
}


sub from_json($) {
   Carp::croak ("JSON::PP::from_json has been renamed to decode_json.");
}


1;
