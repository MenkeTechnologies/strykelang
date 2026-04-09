# Pure-Perl List::Util for perlrs (core Perl’s List/Util.pm is XS-based).
# API is a practical subset of Perl 5’s List::Util; see perldoc List::Util for full semantics.

package List::Util;

our $VERSION = '1.68';

sub uniq {
    my @out;
    my $prev;
    my $have = 0;
    for my $x (@_) {
        if ( !$have || $prev ne $x ) {
            push @out, $x;
            $prev = $x;
            $have = 1;
        }
    }
    return @out;
}

sub sum {
    if ( !@_ ) {
        return;
    }
    my $s = 0;
    for my $x (@_) {
        $s += $x;
    }
    return $s;
}

sub sum0 {
    my $s = 0;
    for my $x (@_) {
        $s += $x;
    }
    return $s;
}

sub min {
    if ( !@_ ) {
        return;
    }
    my $m = shift;
    for my $x (@_) {
        $m = $x if $x < $m;
    }
    return $m;
}

sub max {
    if ( !@_ ) {
        return;
    }
    my $m = shift;
    for my $x (@_) {
        $m = $x if $x > $m;
    }
    return $m;
}

sub minstr {
    if ( !@_ ) {
        return;
    }
    my $m = shift;
    for my $x (@_) {
        $m = $x if $x lt $m;
    }
    return $m;
}

sub maxstr {
    if ( !@_ ) {
        return;
    }
    my $m = shift;
    for my $x (@_) {
        $m = $x if $x gt $m;
    }
    return $m;
}

sub product {
    my $p = 1;
    for my $x (@_) {
        $p *= $x;
    }
    return $p;
}

sub shuffle {
    my @a = @_;
    my $n = @a;
    for ( my $i = 0 ; $i < $n ; $i = $i + 1 ) {
        my $j = int( rand( $i + 1 ) );
        my $t = $a[$i];
        $a[$i] = $a[$j];
        $a[$j] = $t;
    }
    return @a;
}

1;
