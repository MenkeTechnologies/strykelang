# m//g in SCALAR or VOID context is the iterating form: it advances pos()
# and yields a boolean. Only LIST context returns the matches.
#
# Recognising only the boolean-condition sites (`while ($s =~ /x/g)`) left
# pos() undef after a bare statement or a scalar assignment, which makes \G
# unusable.

# Void context — a bare statement.
my $s = "aaa";
$s =~ /a/g;
print defined(pos($s)) ? pos($s) : "undef", "\n";
$s =~ /a/g;
print defined(pos($s)) ? pos($s) : "undef", "\n";

# Scalar context — the result is a boolean, not the matched text.
my $t = "aaabbb";
my $r = ($t =~ /a+/g);
print "r=[$r] pos=", (defined pos($t) ? pos($t) : "undef"), "\n";

# \G anchors at pos, and /c keeps pos when the match finally fails.
while ($t =~ /\G(b)/gc) { print "G:$1\n" }
print defined(pos($t)) ? pos($t) : "undef", "\n";

# Without /c a failed iterating match resets pos.
my $u = "ab";
$u =~ /a/g;
print pos($u), "\n";
$u =~ /z/g;
print defined(pos($u)) ? pos($u) : "undef", "\n";

# A /gc failure at the very start leaves pos alone (still undef).
my $v = "ab";
$v =~ /zz/gc;
print defined(pos($v)) ? pos($v) : "undef", "\n";

# The while form still walks, and exhausting it resets pos.
my $w = "abcabc";
while ($w =~ /a/g) { print "at ", pos($w), "\n" }
print defined(pos($w)) ? pos($w) : "undef", "\n";

# LIST context is unchanged: all matches, captures flattened.
my $x = "a1b2";
my @m = ($x =~ /(\w)(\d)/g);
print "@m\n";
my $count = () = $x =~ /\d/g;
print "$count\n";

# Destructuring a non-/g match still binds captures.
my ($k, $val) = ("key=42" =~ /^(\w+)=(\d+)/);
print "$k/$val\n";
