# `*name = VALUE` installs into the slot the value's kind selects, and the
# install is an ALIAS, not a copy: `*x = \@a` makes `@x` and `@a` one array.

our @arr = (1, 2);
our %map = (k => "v");
our $sca = "S";
sub code { 7 }

# `undef` clears nothing and is not an error.
*a_undef = undef;
print "1: ok\n";

# A reference fills the slot of its own type.
*a_ref = \@arr;   print "2: @a_ref\n";
*h_ref = \%map;   print "3: $h_ref{k}\n";
*s_ref = \$sca;   print "4: $s_ref\n";
*c_ref = \&code;  print "5: ", c_ref(), "\n";

# The alias is live in both directions.
push @arr, 3;
print "6: @a_ref\n";
push @a_ref, 4;
print "7: @arr\n";
$sca = "T";
print "8: $s_ref\n";
$h_ref{n} = 2;
print "9: ", join(",", map { "$_=$map{$_}" } sort keys %map), "\n";

# A glob aliases every slot at once.
*all = *arr;
print "10: @all\n";
push @arr, 5;
print "11: @all\n";

# Filling one slot leaves the others alone.
our @keep = ("A");
our %keep = (h => "B");
*mixed = \@keep;
*mixed = \%keep;
print "12: @mixed $mixed{h}\n";

# A plain string is a symbolic glob name.
{
    no strict 'refs';
    our @target = (9, 8);
    *by_name = "target";
    print "13: @by_name\n";
}

# Names beginning with a quote-like operator are still symbols after `*`.
our @y = ("wye");
our @q = ("cue");
our @s = ("ess");
our @tr = ("tee");
our @m = ("em");
*ay = *y;  *cu = *q;  *es = *s;  *te = *tr;  *em = *m;
print "14: @ay @cu @es @te @em\n";
