# `(EXPR)[...]` slices a LIST, and any single value is a one-element list.
# The slice used to require its source to already be a list, so a scalar
# source died with "Can't use arrow deref on non-array-ref".

# A bare scalar literal is a one-element list.
print( ("solo")[0] , "\n");

# A sub that returns a plain scalar in list context.
sub ctx { wantarray ? "L" : "S" }
print( (ctx())[0], "\n");
my @from_list = (ctx())[0];
print "$from_list[0]\n";

# A hash flattens to key/value pairs, exactly as in a list literal.
my %h = (a => 1);
my @kv = (%h)[0, 1];
print "@kv\n";

# A reference stays ONE element — it must not be flattened or dereferenced.
# (Written through a sub call: `($aref)[0]` on a bare scalar variable is
# still parsed as an implicit-arrow deref, a separate parser gap.)
sub aref { return [1, 2, 3] }
my $got = (aref())[0];
print ref($got) || "NOREF", "\n";

# Real lists are unchanged.
print join(",", (10, 20, 30)[0, 2]), "\n";
print( (1, 2, 3)[1], "\n");
sub three { return (10, 20, 30) }
print join(",", (three())[0, 2]), "\n";
print join(",", (reverse(1, 2, 3))[0, 1]), "\n";
print( (sort { $a <=> $b } 3, 1, 2)[0], "\n");

# The idioms that make list slices worth having.
print join(",", (localtime(0))[5, 4, 3]), "\n";
print( (split /,/, "a,b,c")[1], "\n");
print( (getpwnam("root"))[2], "\n");
print( ((stat("/etc/hosts"))[7] > 0) ? "sized\n" : "empty\n");

# Out-of-range indices fill with undef and keep the slice's width.
my @oor = (1, 2)[0, 5];
print scalar(@oor), ":", join(",", map { defined $_ ? $_ : "undef" } @oor), "\n";
my $one = (1, 2)[5];
print defined($one) ? "def\n" : "undef\n";
