# `(LIST)[...]` is a list slice. Two rules that a subscript on a parenthesised
# term must follow, and that an array-element parse would get wrong.

# 1. A parenthesised SCALAR is a one-element list, so `($s)[0]` is that scalar --
#    not element 0 of `@s`. Give `@scalar` different contents to prove which one
#    is being read.
our @scalar = (10, 20, 30);
my $scalar  = "the-scalar";
print "1: ", ($scalar)[0], "\n";
print "2: $scalar[0]\n";

# A reference stays a single element rather than flattening.
my @backing = (7, 8, 9);
my $ref = \@backing;
my @got = ($ref)[0];
print "3: ", ref($got[0]), " ", scalar(@got), "\n";

# 2. A slice of an EMPTY list is the empty list; a slice of a non-empty list
#    yields one element per index, undef past the end.
my @a = ()[0];
print "4: ", scalar(@a), "\n";
my @b = ()[0, 1];
print "5: ", scalar(@b), "\n";
my @c = (1, 2)[5];
print "6: ", scalar(@c), " ", (defined $c[0] ? "def" : "undef"), "\n";
my @d = (1, 2)[0, 5];
print "7: ", scalar(@d), " $d[0] ", (defined $d[1] ? "def" : "undef"), "\n";

# Emptiness is a RUN-TIME property, so an empty array or an empty sub return
# behaves the same as the literal `()`.
my @empty = ();
my @e = (@empty)[0];
print "8: ", scalar(@e), "\n";
my @f = (@empty)[0, 1];
print "9: ", scalar(@f), "\n";
sub nothing { return () }
my @g = (nothing())[0];
print "10: ", scalar(@g), "\n";

# Ordinary list slices still work.
my @h = (qw(a b c d))[1, 3];
print "11: @h\n";
print "12: ", (sort { $a <=> $b } 5, 3, 9)[0], "\n";
my @nonempty = (4, 5, 6);
print "13: ", (@nonempty)[2], "\n";

# Negative and repeated indices.
my @i = (1, 2, 3)[-1, 0, 0];
print "14: @i\n";

# Scalar context takes the last element of the slice.
my $last = (10, 20, 30)[0, 2];
print "15: $last\n";
