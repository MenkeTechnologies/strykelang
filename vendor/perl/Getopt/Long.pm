# Minimal Getopt::Long for common `GetOptions( 'opt|o' => \$x, ... )` forms used by zpwr scripts.
package Getopt::Long;

use strict;
use warnings;

sub GetOptions {
    my %cfg = @_;
    my @remaining;
    foreach my $arg (@ARGV) {
        my $matched = 0;
        KEYS: foreach my $key ( keys %cfg ) {
            my $ref = $cfg{$key};
            my ( $primary, @alts ) = split( /\|/, $key );
            if ( $arg eq '--' . $primary ) {
                $$ref = 1;
                $matched = 1;
                last KEYS;
            }
            for my $a (@alts) {
                if ( $arg eq '-' . $a ) {
                    $$ref = 1;
                    $matched = 1;
                    last KEYS;
                }
            }
        }
        push @remaining, $arg unless $matched;
    }
    @ARGV = @remaining;
    return 1;
}

1;
