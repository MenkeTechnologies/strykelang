#!/usr/bin/env stryke
# Text processing demo — common Perl one-liner patterns

use strict;
use warnings;

my $text = "The quick brown fox jumps over the lazy dog";

# Case operations
print "upper: ", uc($text), "\n";
print "lower: ", lc($text), "\n";
print "ucfirst: ", ucfirst($text), "\n";

# String operations
print "length: ", length($text), "\n";
print "substr(0,9): ", substr($text, 0, 9), "\n";
print "index(fox): ", index($text, "fox"), "\n";
print "reverse: ", join("", reverse split("", $text)), "\n";

# Split and join
my @words = split(" ", $text);
print "word count: ", scalar @words, "\n";
print "words: ", join(" | ", @words), "\n";

# Regex
my $copy = $text;
$copy =~ s/fox/cat/;
print "substituted: $copy\n";

if ($text =~ /(\w+)\s+(\w+)$/) {
    print "last two words: $1 $2\n";
}

# Map and grep
my @long_words = grep { length($_) > 3 } @words;
print "long words: ", join(", ", @long_words), "\n";

my @lengths = map { length($_) } @words;
print "word lengths: ", join(", ", @lengths), "\n";
