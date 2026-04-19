# Minimal Getopt::Long for common `GetOptions( 'opt|o' => \$x, ... )` forms used by zpwr scripts.
# Supports: boolean (`help|h`), string (`regex=s` → `--regex=…` or `--regex` + next arg).
package Getopt::Long;

use strict;
use warnings;

sub GetOptions {
    my %cfg = @_;
    my @remaining;
    my @args = @ARGV;
    while (@args) {
        my $arg = shift @args;
        my $matched = 0;
      KEYS: foreach my $key ( keys %cfg ) {
            my $ref = $cfg{$key};
            if ( $key =~ /^(.+)=(s)$/ ) {
                my $namespec = $1;
                my @names = split /\|/, $namespec;
                for my $n (@names) {
                    # String prefix (forge regex does not implement \Q...\E like Perl 5).
                    my $prefix = "--$n=";
                    if ( length($arg) >= length($prefix)
                        && substr( $arg, 0, length($prefix) ) eq $prefix )
                    {
                        $$ref = substr( $arg, length($prefix) );
                        $matched = 1;
                        last KEYS;
                    }
                }
                for my $n (@names) {
                    if ( $arg eq "--$n" ) {
                        $$ref = defined $args[0] ? shift @args : '';
                        $matched = 1;
                        last KEYS;
                    }
                }
                next KEYS;
            }
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
