# Bare `shift` / `pop` at file scope operate on `@ARGV`, not `@_`.
#
# perlfunc: "If ARRAY is omitted, [shift] shifts the @_ array within a
# subroutine ... or the @ARGV array outside a subroutine." The empty-paren
# spellings `shift()` / `pop()` take the same default.
#
# The harness runs each case with no command-line arguments, so the script
# seeds @ARGV itself — perl treats an assigned @ARGV exactly like the one the
# interpreter builds.

@ARGV = ('a', 'b', 'c', 'd', 'e', 'f');

print "shift=",   shift,   "\n";
print "pop=",     pop,     "\n";
print "shift()=", shift(), "\n";
print "pop()=",   pop(),   "\n";
print "left=@ARGV\n";

# `@_` at file scope is empty and stays empty — the file-scope defaults never
# touch it.
print "under=", scalar(@_), "\n";

# The explicit forms still address the array they name.
my @own = (10, 20, 30);
print "shift-own=", shift(@own), " pop-own=", pop(@own), " own=@own\n";
print "shift-argv=", shift(@ARGV), "\n";
print "final=@ARGV\n";

# Draining past the end yields undef without touching @_.
@ARGV = ();
my $empty = shift;
print "drained=", defined($empty) ? "def" : "undef", "\n";
