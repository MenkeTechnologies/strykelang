# `-t` takes a FILEHANDLE, not a path, so it neither stats nor opens its
# operand. A handle operand is therefore always defined -- 1 for a terminal and
# the empty-string false otherwise -- while a path names no handle at all and is
# undef. Testing the operand as a path gets both halves backwards: it makes
# `-t STDIN` undef (the stat of a file named "STDIN" fails) and `-t "/etc/hosts"`
# a defined false (that stat succeeds).
#
# length() is printed for every result because it is the only thing that tells
# the empty-string false apart from a literal "0" -- the distinction the rest of
# this corpus was blind to.
my @label;
my @result;
my $v;

$v = -t STDIN;
push @label, 'bareword STDIN';
push @result, $v;

$v = -t STDOUT;
push @label, 'bareword STDOUT';
push @result, $v;

my $named = "STDERR";
$v = -t $named;
push @label, 'string STDERR';
push @result, $v;

# A bare descriptor number is a handle; an unopened one is still defined false.
my $fd_zero = "0";
$v = -t $fd_zero;
push @label, 'descriptor 0';
push @result, $v;

my $fd_unused = "99";
$v = -t $fd_unused;
push @label, 'descriptor 99';
push @result, $v;

# Paths are not handles, however terminal-ish they look.
my $script = $0;
$v = -t $script;
push @label, 'path to this script';
push @result, $v;

my $dev_stdin = "/dev/stdin";
$v = -t $dev_stdin;
push @label, 'path /dev/stdin';
push @result, $v;

my $dev_fd = "/dev/fd/1";
$v = -t $dev_fd;
push @label, 'path /dev/fd/1';
push @result, $v;

# The descriptor form is strict: only a canonical non-negative integer counts.
my $padded = "007";
$v = -t $padded;
push @label, 'non-canonical 007';
push @result, $v;

my $spaced = " 1";
$v = -t $spaced;
push @label, 'leading-space 1';
push @result, $v;

my $negative = "-1";
$v = -t $negative;
push @label, 'negative -1';
push @result, $v;

my $unknown = "NOPEFH";
$v = -t $unknown;
push @label, 'unknown handle name';
push @result, $v;

my $empty = "";
$v = -t $empty;
push @label, 'empty string';
push @result, $v;

my $i = 0;
while ($i <= $#label) {
    my $r = $result[$i];
    printf(
        "%-22s defined=%d length=%d value=[%s]\n",
        $label[$i],
        defined($r) ? 1                : 0,
        defined($r) ? length($r)       : -1,
        defined($r) ? $r               : 'undef'
    );
    $i++;
}

# An open handle is a handle no matter how it was made, and a regular file is
# never a terminal, so this is a defined false rather than undef.
open(my $fh, '<', $0) or die "open: $!";
my $on_handle = -t $fh;
printf(
    "lexical filehandle     defined=%d length=%d value=[%s]\n",
    defined($on_handle) ? 1          : 0,
    defined($on_handle) ? length($on_handle) : -1,
    defined($on_handle) ? $on_handle : 'undef'
);
close($fh);
