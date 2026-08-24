# perl's `ref` says REF, not SCALAR, whenever the referent is itself a
# reference (pp_ref checks SvROK on the referent). Every kind of referent
# counts: plain refs, blessed objects, qr//, coderefs.
my @a;
my %h;
my $obj = bless {}, "Foo";
my $qr  = qr/x/;
my $code = sub { 1 };
my $aref = [];
print join("|",
    ref(\\1),
    ref(\\@a),
    ref(\\%h),
    ref(\$obj),
    ref(\$qr),
    ref(\$code),
    ref(\$aref),
    ref(\\\1)), "\n";

# Referents that are NOT references stay SCALAR.
my $plain = 5;
print join("|", ref(\"s"), ref(\undef), ref(\$plain), ref(\1)), "\n";

# A ref taken from a package global behaves the same.
our $glob_ref = \1;
print ref(\$glob_ref), "\n";

# One indirection deeper: $r is a ref, so \$r is a REF while $r itself is SCALAR.
my $r = \$plain;
print ref($r), "|", ref(\$r), "\n";

# Same answer once the loop is hot enough to be JIT-compiled.
my $hits = 0;
for my $i (1 .. 50000) { $hits++ if ref(\$aref) eq "REF" }
print "hot:$hits\n";
