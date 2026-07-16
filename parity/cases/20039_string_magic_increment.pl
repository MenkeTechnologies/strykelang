# ++ on a string matching /^[a-zA-Z]*[0-9]*$/ increments with carry, not numify.
for my $x ("az", "Az", "zz", "a9", "Zz", "aa9", "Zz9") {
    my $y = $x;
    $y++;
    print "$x $y\n";
}
