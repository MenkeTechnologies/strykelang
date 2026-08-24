# prototype() must return the prototype exactly as written. `$$` is the one
# sigil pair the lexer reads as a single token (it is also the PID variable),
# so an off-by-one there turns sub f($$) into the prototype "$$$".
sub p0()      { }
sub p1($)     { }
sub p2($$)    { }
sub p3($$$)   { }
sub p4($$$$)  { }
sub psemi($;$){ }
sub pslurp(\@\@) { }
sub pblock(&;@)  { }
sub pglob(*)     { }
sub pmix($$;@)   { }
for my $c (\&p0, \&p1, \&p2, \&p3, \&p4, \&psemi, \&pslurp, \&pblock, \&pglob, \&pmix) {
    my $p = prototype($c);
    print defined($p) ? "[$p]" : "[undef]", "\n";
}
