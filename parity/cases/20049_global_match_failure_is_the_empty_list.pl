# A `/g` match that finds nothing is a failed match, so in list context it is
# the empty list and in scalar context it is Perl false. Returning the scalar 0
# instead puts a one-element list holding a false value into the caller's hands,
# which silently inverts every `my @m = $s =~ /.../g` guard: the array has one
# element, so the guard takes its true branch on a match that did not happen.
#
# Non-`/g` list-context failure was already the empty list; only `/g` was not,
# so both forms are checked here to keep them from drifting apart again.
my $subject = "abc";

my @miss = ($subject =~ /z/g);
printf("g miss no capture   count=%d guard=%s\n",
    scalar(@miss), @miss ? 'true' : 'false');

my @miss_capture = ($subject =~ /(z)/g);
printf("g miss with capture count=%d guard=%s\n",
    scalar(@miss_capture), @miss_capture ? 'true' : 'false');

my @miss_flags = ($subject =~ /Z/gi);
printf("g miss case-folded  count=%d guard=%s\n",
    scalar(@miss_flags), @miss_flags ? 'true' : 'false');

my $empty_subject = "";
my @miss_empty = ($empty_subject =~ /x/g);
printf("g miss empty target count=%d guard=%s\n",
    scalar(@miss_empty), @miss_empty ? 'true' : 'false');

# The success paths must keep returning what they already did.
my @hit = ($subject =~ /./g);
printf("g hit no capture    count=%d [%s]\n", scalar(@hit), join(',', @hit));

my @hit_capture = ($subject =~ /(.)/g);
printf("g hit with capture  count=%d [%s]\n",
    scalar(@hit_capture), join(',', @hit_capture));

# The same failed match in scalar context is Perl's empty-string false, which
# only length() can tell apart from a literal "0".
my $scalar_miss = ($subject =~ /z/g);
printf("g miss in scalar    defined=%d length=%d value=[%s]\n",
    defined($scalar_miss) ? 1 : 0,
    defined($scalar_miss) ? length($scalar_miss) : -1,
    defined($scalar_miss) ? $scalar_miss : 'undef');

# Non-global failure, for the same three observations.
my @plain_miss = ($subject =~ /z/);
printf("plain miss          count=%d guard=%s\n",
    scalar(@plain_miss), @plain_miss ? 'true' : 'false');

my @plain_miss_capture = ($subject =~ /(z)(y)/);
printf("plain miss capture  count=%d guard=%s\n",
    scalar(@plain_miss_capture), @plain_miss_capture ? 'true' : 'false');

my $plain_scalar_miss = ($subject =~ /z/);
printf("plain miss scalar   defined=%d length=%d value=[%s]\n",
    defined($plain_scalar_miss) ? 1 : 0,
    defined($plain_scalar_miss) ? length($plain_scalar_miss) : -1,
    defined($plain_scalar_miss) ? $plain_scalar_miss : 'undef');
