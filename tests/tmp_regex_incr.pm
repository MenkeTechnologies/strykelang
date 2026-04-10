package T;
use strict;
sub incr_parse {
    my ( $self, $coder, $text ) = @_;
    my $s;
    my $len = 1;
    my $p = 0;
    while ( $len > $p ) {
        $s = substr( $text, $p++, 1 );
        next if defined $s and $s =~ /[rueals]/;
        next if defined $s and $s =~ /[0-9eE.+\-]/;
    }
}
1;
