my %h = (a => 1);
delete $h{a};
printf "%d\n", exists $h{a} ? 1 : 0;
