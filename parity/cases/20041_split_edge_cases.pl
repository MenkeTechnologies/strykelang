# Captures in the pattern are returned as fields.
print join("|", split /(,)/, "a,b,c"), "\n";
# Empty pattern splits every character.
print join("|", split //, "abc"), "\n";
# Trailing empty fields are dropped without a limit...
my @t = split /,/, "a,b,,,";
print scalar(@t), "\n";
# ...but a negative limit keeps them.
print join("|", split(/,/, "a,b,,,", -1)), "\n";
# Leading empty fields are kept.
print join("|", split /,/, ",a,b"), "\n";
# ' ' is magic: strips leading whitespace, splits on runs.
print join("|", split ' ', "  a  b  c "), "\n";
