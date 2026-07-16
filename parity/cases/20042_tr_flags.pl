# tr returns the match count.
my $t = "hello world";
my $c = ($t =~ tr/a-z//);
print "$c\n";
# /s squeezes repeats of the same replacement.
(my $e = "aabbcc") =~ tr/a-c//s;
print "$e\n";
# /r returns the result, leaving the target alone.
my $src = "hello";
my $r = $src =~ tr/a-y/b-z/r;
print "$r $src\n";
# /d deletes unreplaced chars.
(my $d = "hello world") =~ tr/a-z//cd;
print "$d\n";
