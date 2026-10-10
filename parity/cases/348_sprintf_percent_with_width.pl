# `%%` is a conversion like any other: width, `-` and `0` apply to it and it
# consumes no argument.

printf("[%5%]\n");
printf("[%-5%]\n");
printf("[%05%]\n");
printf("[%%]\n");
printf("[%3%|%s|%3%]\n", "x");
my $s = sprintf("%d%%|%4%|%-4%|", 50);
print "$s\n";
