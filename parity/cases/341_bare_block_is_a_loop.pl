# A bare block is a loop that runs once: `last` and `next` leave it, `redo`
# re-enters it. `Carp::short_error_loc` walks caller frames with exactly this.

my $n = 0;
{ $n++; print "1: pass $n\n"; redo if $n < 3; }

{ print "2: before\n"; last; print "2: unreachable\n"; }

{ print "3: before\n"; next; print "3: unreachable\n"; }

# A label names the block for `last`/`next`/`redo`.
my $i = 0;
STEP: { $i++; print "4: step $i\n"; redo STEP if $i < 2; }

# The block's own lexicals go away on every exit path, including `redo`.
my $seen = 0;
{ my $x = ++$seen; print "5: x=$x\n"; redo if $seen < 2; }

# `last` inside a bare block leaves the block, not the enclosing loop.
for my $k (1, 2) {
    { print "6: inner $k\n"; last; }
    print "7: after inner $k\n";
}

# An inner bare block does not capture an outer loop's `last`.
OUTER: for my $k (1, 2, 3) {
    { last OUTER if $k == 2; }
    print "8: k=$k\n";
}

# `redo` re-runs the block from the top, so the frame is fresh each time.
my @log;
my $c = 0;
{ my $local = "iter" . ++$c; push @log, $local; redo if $c < 3; }
print "9: @log\n";
