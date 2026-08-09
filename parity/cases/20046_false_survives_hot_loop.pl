# A predicate whose value escapes a JIT-compiled subroutine must still be
# Perl's empty-string false. The native tiers carry plain i64 and re-box with
# StrykeValue::integer, so a comparison that only becomes wrong after the sub
# crosses its JIT invocation threshold is invisible to any test that calls it
# a handful of times. Every sub below is called far past that threshold.
my $ITERS = 400;

sub p_lt { my ($a, $b) = @_; return $a < $b; }
sub p_eq { my ($a, $b) = @_; return $a == $b; }
sub p_ne { my ($a, $b) = @_; return $a != $b; }
sub p_ge { my ($a, $b) = @_; return $a >= $b; }
sub p_streq { my ($a, $b) = @_; return $a eq $b; }
sub p_not { my ($x) = @_; return !$x; }

my ($lt_f, $eq_f, $ne_f, $ge_f, $st_f, $no_f);
my ($lt_t, $eq_t);
for my $i (1 .. $ITERS) {
    $lt_f = p_lt(9, 2);
    $eq_f = p_eq(1, 2);
    $ne_f = p_ne(3, 3);
    $ge_f = p_ge(1, 5);
    $st_f = p_streq("a", "b");
    $no_f = p_not(1);
    $lt_t = p_lt(2, 9);
    $eq_t = p_eq(4, 4);
}
printf "hot_lt_false    [%s] len=%d\n", $lt_f, length($lt_f);
printf "hot_eq_false    [%s] len=%d\n", $eq_f, length($eq_f);
printf "hot_ne_false    [%s] len=%d\n", $ne_f, length($ne_f);
printf "hot_ge_false    [%s] len=%d\n", $ge_f, length($ge_f);
printf "hot_streq_false [%s] len=%d\n", $st_f, length($st_f);
printf "hot_not_false   [%s] len=%d\n", $no_f, length($no_f);
printf "hot_lt_true     [%s] len=%d\n", $lt_t, length($lt_t);
printf "hot_eq_true     [%s] len=%d\n", $eq_t, length($eq_t);

# The same value written into a variable inside a hot loop body, rather than
# returned across the call boundary.
my $inline;
for my $i (1 .. $ITERS) { $inline = ($i > $ITERS * 2); }
printf "hot_inline      [%s] len=%d\n", $inline, length($inline);

# A predicate still has to work as a condition after all that.
my $taken = 0;
for my $i (1 .. $ITERS) { $taken++ if p_lt($i, $ITERS); }
print "hot_condition   $taken\n";

# <=> keeps its integer 0 even when hot.
sub p_cmp { my ($a, $b) = @_; return $a <=> $b; }
my $sp;
for my $i (1 .. $ITERS) { $sp = p_cmp(7, 7); }
printf "hot_spaceship   [%s] len=%d\n", $sp, length($sp);
