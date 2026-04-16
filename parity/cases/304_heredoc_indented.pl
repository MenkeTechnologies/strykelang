use strict;
use warnings;
use feature 'say';
# <<~ — indented heredoc (Perl 5.26+). Leading whitespace common to all lines
# is stripped, so the terminator can be indented to match surrounding code.
my $msg = <<~END;
    line one
    line two
      extra indent here
    line four
    END
print $msg;
print "after\n";
