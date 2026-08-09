# `sort SUBNAME LIST` — a bareword directly after `sort` is always the
# comparator's name. It is never the head of the list, and never the head of a
# blockless comparator expression: perl commits to SUBNAME on sight, then
# parses everything after it as LIST.
#
# The `sort getlist()` row is the one that pins the rule rather than merely
# exercising it — perl reads `getlist` as the comparator and `()` as an empty
# list, so the answer is empty, not the list `getlist` would have returned.

sub bynum  { $a <=> $b }
sub bystr  { $a cmp $b }
sub bylen  { length($a) <=> length($b) }
sub getlist { return (5, 4, 6) }

my @l = (3, 1, 2);

print "array:",   join(",", sort bynum @l),        "\n";
print "parens:",  join(",", sort bynum (3, 1, 2)), "\n";
print "commas:",  join(",", sort bynum 3, 1, 2),   "\n";
print "call:",    join(",", sort bynum getlist()), "\n";
print "strings:", join(",", sort bynum "10", "9"), "\n";
print "cmp:",     join(",", sort bystr "10", "9"), "\n";
print "bylen:",   join(",", sort bylen "ccc", "a", "bb"), "\n";

# SUBNAME with a list that is itself empty.
my @empty = ();
print "empty:", join(",", sort bynum @empty), "|\n";

# The rule's sharp edge: `getlist` becomes the comparator, so the list is `()`.
print "subname-wins:", join(",", sort getlist()), "|\n";

# … and it keeps winning when what follows looks like a comparator expression:
# SUBNAME is `bynum`, LIST is `9 <=> 4, 5, 3` — so `9 <=> 4` contributes its
# value (1) as a list element and the result is `1,3,5`. Read as a comparator
# expression instead, the `1` could not appear at all.
print "expr-is-list:", join(",", sort bynum 9 <=> 4, 5, 3), "\n";

# A block comparator and a coderef comparator are unaffected.
my $cr = \&bynum;
print "block:",   join(",", sort { $a <=> $b } (3, 1, 2)), "\n";
print "coderef:", join(",", sort $cr (3, 1, 2)),           "\n";
print "plain:",   join(",", sort (3, 1, 2)),               "\n";

# Nested: the inner sort's list is the outer sort's operand.
print "nested:", join(",", sort bynum (sort bystr "2", "10", "1")), "\n";
