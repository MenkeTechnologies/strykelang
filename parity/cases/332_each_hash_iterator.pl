use strict;
use warnings;
# Hand-authored parity case: `each` and the per-hash iterator it walks.
#
# The corpus reached hashes only through `keys` / `values` / `%h` in list
# context — all of which are stateless. `each` is the one hash builtin that
# carries state *inside the hash*, and every property below follows from that
# one iterator rather than from the hash's contents:
#
#   * it hands out each pair exactly once, then returns the empty list;
#   * returning the empty list also rewinds it, so the next loop starts over
#     (this is what makes back-to-back `while (each)` loops both work, and it
#     is the only reason such a loop terminates at all — the loop condition is
#     the *count* of the list assignment, not the truth of the key);
#   * `keys` and `values` rewind it as a side effect;
#   * a partially consumed iterator resumes where it stopped.
#
# Every print sorts or counts: perl randomizes hash order per process, so the
# ORDER `each` yields is deliberately not asserted here — only the set, the
# multiplicity, and the iterator state are.
our $PARITY_CASE = 'each_hash_iterator';

my %h = (alpha => 1, beta => 2, gamma => 3);

# ---- a full walk visits every pair exactly once ----
my @pairs;
while (my ($k, $v) = each %h) { push @pairs, "$k=$v" }
print "pass1:        ", join(',', sort @pairs), "\n";

# ---- exhaustion rewound the iterator, so a second walk sees the same set ----
my @again;
while (my ($k, $v) = each %h) { push @again, "$k=$v" }
print "pass2:        ", join(',', sort @again), "\n";

# ---- scalar context yields the key alone ----
my @keys;
while (defined(my $k = each %h)) { push @keys, $k }
print "scalar ctx:   ", join(',', sort @keys), "\n";

# ---- `keys` rewinds a partially consumed iterator ----
my ($consumed) = each %h;
my @after_keys = keys %h;
my $n_keys = 0;
while (my ($k, $v) = each %h) { $n_keys++ }
print "keys resets:  ", scalar(@after_keys), " then $n_keys\n";

# ---- `values` rewinds it too ----
each %h;
my @after_values = values %h;
my $n_values = 0;
while (my ($k, $v) = each %h) { $n_values++ }
print "vals reset:   ", scalar(@after_values), " then $n_values\n";

# ---- a partially consumed iterator resumes rather than restarting ----
my %p = (x => 1, y => 2, z => 3);
my ($first) = each %p;
my $rest = 0;
while (my ($k, $v) = each %p) { $rest++ }
print "resume:       consumed 1 then $rest\n";

# ---- an empty hash yields the empty list immediately ----
my %empty;
my @none = each %empty;
print "empty hash:   ", scalar(@none), "\n";

# ---- a one-entry hash: one pair, then the empty list ----
my %one = (only => 'value');
my @got = each %one;
my @done = each %one;
print "single entry: ", join('=', @got), " then ", scalar(@done), "\n";

# ---- the list-assignment count, not the key, is what ends the loop ----
# A key of "0" is false but the assignment still yields 2, so the loop must
# keep going. This is the case a `while ($k = each %h)` implementation gets
# wrong and a `while (($k,$v) = each %h)` one gets right.
my %falsy = ('0' => 'zero', '' => 'empty', 'x' => 'ex');
my $seen = 0;
while (my ($k, $v) = each %falsy) { $seen++ }
print "falsy keys:   $seen\n";

# ---- values are the hash's values, not copies of the keys ----
my %typed = (num => 42, str => 'text');
my @rendered;
while (my ($k, $v) = each %typed) { push @rendered, "$k:$v" }
print "values:       ", join(',', sort @rendered), "\n";
