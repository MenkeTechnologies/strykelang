my %h = (a=>1,b=>2);
my $x = delete $h{b};
print $x, ",", scalar(keys %h), "\n";
