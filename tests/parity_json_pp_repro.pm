package JSON::PP;

# JSON-2.0

use 5.008;
use strict;

# use Exporter ();
# BEGIN { our @ISA = ('Exporter') }

# use overload ();
# use JSON::PP::Boolean;

use Carp ();
# use Scalar::Util qw(blessed reftype refaddr);

our $VERSION = '4.16';

our @EXPORT = qw(encode_json decode_json from_json to_json);

use constant USE_B => $ENV{PERL_JSON_PP_USE_B} || 0;
# use constant CORE_BOOL => defined &builtin::is_bool;

my $invalid_char_re;

BEGIN {
    $invalid_char_re = "[";
    for my $i (0 .. 0x01F, 0x22, 0x5c) {
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

my $JSON;

sub encode_json ($) {
    ($JSON ||= __PACKAGE__->new->utf8)->encode(@_);
}

1;
