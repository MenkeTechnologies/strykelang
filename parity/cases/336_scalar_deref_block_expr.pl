# `${ EXPR }` with a general expression body (not just a variable name).
our $v = "value";

# Ref-of-expression body.
print "1: ", ${ \ "literal" }, "\n";
print "2: ", ${ \ ($v) }, "\n";

# Body containing braces of its own -- the old "read to the first }" lexing
# truncated these.
my %h = (cb => \ "hash-held");
print "3: ", ${ $h{cb} }, "\n";

my $rr = { inner => \ "nested" };
print "4: ", ${ $rr->{inner} }, "\n";

# Plain-name bodies keep their existing meaning.
print "5: ", ${v}, "\n";
my $name = "v";
{
    no strict 'refs';
    print "6: ", ${$name}, "\n";
}

# Assignment through a block deref.
my $x = 1;
${ \$x } = 9;
print "7: $x\n";

# A `->` chain inside the body, and a body whose braces nest.
my %deep = (a => { b => \ "deep" });
print "8: ", ${ $deep{a}{b} }, "\n";
