# A `my` declared earlier in the compilation unit is already a pad entry when a
# later BEGIN block is compiled and run, so the block writes the very slot the
# declaration introduces. The run-time introduction only registers a scope-exit
# clear, so the BEGIN's write survives into the main body.
my $set;
BEGIN { $set = "from BEGIN" }
print "1: $set\n";

# An initializer still runs in the main body and overwrites the BEGIN value.
my $init = "main";
BEGIN { $init = "begin" }
print "2: $init\n";

# `our` (package global) is unaffected by the pad rule.
our $pkg;
BEGIN { $pkg = "our-begin" }
print "3: $pkg\n";

# Only lexicals declared ABOVE the block are visible; a BEGIN after two
# declarations sees both.
my $a1;
my $a2;
BEGIN { $a1 = "one"; $a2 = "two" }
print "4: $a1 $a2\n";

# A sub defined inside BEGIN closes over the pad entry.
my $captured;
BEGIN { $captured = 41; }
sub bump { return $captured + 1 }
print "5: ", bump(), "\n";

# `use strict` does not change any of it.
use strict;
my $strict_ok;
BEGIN { $strict_ok = "strict fine" }
print "6: $strict_ok\n";
