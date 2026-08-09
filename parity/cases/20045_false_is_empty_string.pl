# Perl's false is PL_sv_no: the empty string in string context, 0 in numeric
# context. Every predicate must yield it, so a false comparison prints nothing
# and has length 0 — printing "0" is the divergence this case pins.
my @probe = (
    [ 'numeq', (1 == 2) ], [ 'numne', (1 != 1) ],
    [ 'numlt', (2 <  1) ], [ 'numgt', (1 >  2) ],
    [ 'numle', (2 <= 1) ], [ 'numge', (1 >= 2) ],
    [ 'streq', ("a" eq "b") ], [ 'strne', ("a" ne "a") ],
    [ 'strlt', ("b" lt "a") ], [ 'strgt', ("a" gt "b") ],
    [ 'strle', ("b" le "a") ], [ 'strge', ("a" ge "b") ],
    [ 'lognot', !1 ],
    [ 'defined', defined(undef) ],
);
for my $p (@probe) {
    printf "%-8s [%s] len=%d num=%d true=%s\n",
        $p->[0], $p->[1], length($p->[1]), $p->[1] + 0,
        ($p->[1] ? "T" : "F");
}

# True stays the integer 1 on both sides.
printf "true     [%s] len=%d\n", (1 == 1), length(1 == 1);

# The false value survives interpolation, concatenation and join unchanged.
my $f = (1 == 2);
print "interp   [$f]\n";
print "concat   [", "x" . $f . "y", "]\n";
print "join     [", join(",", (1 == 2), (2 == 2), (3 == 4)), "]\n";

# Numeric use of the false value must not warn or become "0".
print "sum      ", (1 == 2) + (2 == 2) + (3 == 3), "\n";

# `exists` on a missing key is false, not 0.
my %h = (present => 1);
printf "exists   [%s] len=%d\n", exists $h{absent}, length(exists $h{absent});
printf "exists_t [%s]\n", exists $h{present};

# A scalar-context match failure is the same false.
my $m = ("abc" =~ /zzz/);
printf "match    [%s] len=%d\n", $m, length($m);
my $nm = ("abc" !~ /a/);
printf "notmatch [%s] len=%d\n", $nm, length($nm);

# <=> and cmp are NOT booleans — their 0 means "equal" and stays an integer.
printf "spaceship [%s][%s][%s]\n", (5 <=> 3), (3 <=> 5), (3 <=> 3);
printf "strcmp    [%s][%s][%s]\n", ("b" cmp "a"), ("a" cmp "b"), ("a" cmp "a");

# UNIVERSAL::isa / DOES report the same empty-string false.
my $obj = bless {}, 'Probe::Class';
printf "isa_no   [%s] len=%d\n", $obj->isa('Nope'), length($obj->isa('Nope'));
printf "isa_yes  [%s]\n", $obj->isa('Probe::Class');
