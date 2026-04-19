# Test module for @INC / require / use (stryke)
package Trivial;
our @EXPORT = qw(trivial_answer);
our @EXPORT_OK = qw(trivial_answer);
sub trivial_answer { 42 }
