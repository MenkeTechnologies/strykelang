use strict;
use warnings;
# <<'EOF' — single-quoted, NO interpolation. Variables stay literal.
my $name = "world";
print <<'EOF';
hello $name
$_ literal
EOF
print "after\n";
