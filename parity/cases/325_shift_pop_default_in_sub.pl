# Inside a subroutine body, bare `shift` / `pop` operate on `@_` — and so do
# the plain blocks nested in that body (`sort`, `map`, `grep`, `eval {}`,
# `do {}`, bare, `for`). Perl picks the default array by enclosing *subroutine*,
# not by enclosing block, so a block inside a sub keeps using `@_`.
#
# @ARGV is seeded with values that are visibly distinct from every sub's
# arguments, so any leak in either direction shows up in the output.

@ARGV = ('V1', 'V2', 'V3', 'V4');

sub named {
    print "named-shift=", shift, "\n";
    print "named-pop=",   pop,   "\n";
    print "named-rest=@_\n";
}
named('n1', 'n2', 'n3', 'n4');

sub with_blocks {
    my @m = map { shift } (1);
    print "map-in-sub=@m\n";
    my @g = grep { shift; 1 } (1);
    print "grep-in-sub-left=", scalar(@_), "\n";
    my @s = sort { shift; 0 } (1, 2);
    print "sort-in-sub-left=", scalar(@_), "\n";
    my $e = eval { shift };
    print "eval-block-in-sub=$e\n";
    my $d = do { shift };
    print "do-block-in-sub=$d\n";
    { my $b = shift; print "bare-block-in-sub=$b\n"; }
    for (1) { my $f = shift; print "for-block-in-sub=$f\n"; }
    print "with-blocks-left=", scalar(@_), "\n";
}
with_blocks('b1', 'b2', 'b3', 'b4', 'b5', 'b6', 'b7', 'b8');

# A named sub declared inside another sub is still its own subroutine.
sub outer_holder {
    sub inner_named { print "inner-named-shift=", shift, "\n"; }
    inner_named('i1', 'i2');
    my $anon = sub { print "anon-in-sub-shift=", shift, " pop=", pop, "\n"; };
    $anon->('a1', 'a2', 'a3');
    print "holder-shift=", shift, "\n";
}
outer_holder('h1', 'h2');

# Anonymous sub written at file scope: its body is still a sub body.
my $top_anon = sub { print "anon-at-file-scope-shift=", shift, "\n"; };
$top_anon->('t1', 't2');

# Nothing above consumed @ARGV.
print "argv-untouched=@ARGV\n";
