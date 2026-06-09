# Multi-line statement-modifier `if` / `unless` / `while` / `until` /
# `for` / `foreach`. Stock Perl is whitespace-insensitive between a
# statement and its postfix modifier — only `;` terminates. Stryke
# native treats newline as an implicit statement terminator (so the
# modifier would start a new `if (...)` statement, fail to find `(`,
# and bail). `--compat` disables the newline rule and accepts the
# multi-line shape, matching Perl 5.
#
# This shape is pervasive in CPAN: e.g. Test.pm:181-182
#   print $TESTOUT "# Win32::BuildNumber ", &Win32::BuildNumber(), "\n"
#     if defined(&Win32::BuildNumber) and defined &Win32::BuildNumber();

# 1. postfix `if` on a new line, trailing semicolon.
print "if-true\n"
  if 1;
print "if-false-skipped\n"
  if 0;

# 2. postfix `unless`.
print "unless-true\n"
  unless 0;
print "unless-false-skipped\n"
  unless 1;

# 3. multi-line `if` with a chained `and`/`or` condition (the Test.pm shape).
sub have_one { 1 }
sub have_zero { 0 }
print "chained-and\n"
  if have_one() and have_one();
print "chained-or\n"
  if have_zero() or have_one();

# 4. continued print expression (comma list wrapped) + multi-line `if`.
print "list-",
      "comma-",
      "wrap\n"
  if 1;

# 5. multi-line `while` (executes block until cond is false).
my $i = 0;
print $i++, "\n"
  while $i < 3;

# 6. multi-line `until` (inverse of while).
my $j = 0;
print "j=$j\n"
  until ++$j >= 2;

# 7. multi-line `for` over a list.
print "for=$_\n"
  for (10, 20, 30);

# 8. multi-line `foreach` (synonym for `for`).
print "fe=$_\n"
  foreach (qw(a b c));
