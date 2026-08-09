# Under --compat a user sub wins over a stryke extension builtin, and it takes
# an ordinary flattened Perl argument list. The count-family builtins
# (count/size/cnt/len/l/list_count/list_size) are dispatched with each syntactic
# argument preserved as one value, because the builtin dispatches on the
# operand's type. Applying that shape to a user sub of the same name hands it a
# single @_ element for what should be an empty list, so `cnt(@empty)` counted 1
# argument where perl counts 0.
#
# The same sub body under a non-colliding name is the control: it must agree
# with perl both before and after, which is what identifies the collision -- not
# the argument expression -- as the cause.
sub cnt   { return scalar(@_) }
sub len   { return scalar(@_) }
sub size  { return scalar(@_) }
sub count { return scalar(@_) }
sub tally { return scalar(@_) }
sub total { return scalar(@_) }

my @empty = ();
my @three = (1, 2, 3);
my $subject = "abc";

printf("cnt(empty)=%d\n",   cnt(@empty));
printf("len(empty)=%d\n",   len(@empty));
printf("size(empty)=%d\n",  size(@empty));
printf("count(empty)=%d\n", count(@empty));
printf("tally(empty)=%d\n", tally(@empty));
printf("total(empty)=%d\n", total(@empty));

printf("cnt(three)=%d\n",   cnt(@three));
printf("tally(three)=%d\n", tally(@three));

# A failed match is an empty list, so it must reach the sub as no arguments at
# all -- the observable that first surfaced this.
printf("cnt(failed match)=%d\n",   cnt($subject =~ /nope/));
printf("tally(failed match)=%d\n", tally($subject =~ /nope/));

# A successful match with no captures is the one-element list (1).
printf("cnt(ok match)=%d\n",   cnt($subject =~ /b/));
printf("tally(ok match)=%d\n", tally($subject =~ /b/));

# Two captures are two arguments.
printf("cnt(two captures)=%d\n",   cnt($subject =~ /(a)(b)/));
printf("tally(two captures)=%d\n", tally($subject =~ /(a)(b)/));

# Mixed operands still flatten as one Perl list.
printf("cnt(mixed)=%d\n",   cnt(@three, @empty, "x"));
printf("tally(mixed)=%d\n", tally(@three, @empty, "x"));

# The subs still return what a Perl sub returns, so the values agree too.
my @args = cnt(@three) == 3 ? ("ok") : ("wrong");
printf("roundtrip=%s\n", $args[0]);
