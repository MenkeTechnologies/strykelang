# More than one heredoc may start on a single line. The bodies follow in
# source order after that line ends, and the code BETWEEN the tags is still
# ordinary code that has to parse.
my $joined = <<"E1" . <<"E2";
one
E1
two
E2
print $joined;

print <<A, "-mid-\n", <<B;
first
A
second
B

# Interpolating and non-interpolating tags stacked together.
my $who = "world";
print <<"INTERP", <<'LITERAL';
hello $who
INTERP
hello $who
LITERAL

# Indented (<<~) stacked with a plain one.
print <<~IND, <<PLAIN;
    indented
      more
    IND
plain
PLAIN

# Line accounting must survive all of the above: every skipped body line
# still counts toward __LINE__.
print __LINE__, "\n";

# A single heredoc followed by more code on the same line still works.
my $x = <<ONE . "tail\n";
body
ONE
print $x;
print __LINE__, "\n";
