# A list assignment in scalar context yields the RHS element count.
my $n = () = (1, 2, 3, 4);
print "$n\n";
my $z = () = ();
print "$z\n";
my $m = () = ("a" =~ /a/);
print "$m\n";
my $g = () = ("aXbXc" =~ /X/g);
print "$g\n";
