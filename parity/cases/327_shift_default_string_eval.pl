# A string `eval` compiles a fresh program, so its top level is "outside a
# subroutine" — bare `shift` there takes from `@ARGV` even when the `eval` is
# executed from inside a sub whose `@_` is non-empty. Only a `sub { }` written
# *within* the eval'd text gets `@_` back.
#
# This is the one case where the enclosing runtime frame and the enclosing
# compilation unit disagree, and perl follows the compilation unit.

@ARGV = ('e1', 'e2', 'e3', 'e4');

my $top = eval 'shift';
print "file-scope-eval=", defined($top) ? $top : 'undef', "\n";

sub inside {
    print "sub-direct-shift=", shift, "\n";
    my $v = eval 'shift';
    print "sub-string-eval=", defined($v) ? $v : 'undef', "\n";
    # `@_` is still visible inside the eval'd text — it is only the *default*
    # operand that follows the compilation unit.
    my $n = eval 'scalar(@_)';
    print "sub-eval-sees-under=$n\n";
    my $a = eval 'my $c = sub { shift }; $c->("from-anon")';
    print "sub-eval-anon=", defined($a) ? $a : 'undef', "\n";
    print "sub-left=@_\n";
}
inside('u1', 'u2', 'u3');

print "argv-left=@ARGV\n";

# An eval'd text that defines and calls a named sub: the sub body is a sub body.
my $r = eval 'sub eval_named { shift } eval_named("named-arg")';
print "eval-named=", defined($r) ? $r : 'undef', "\n";
print "argv-final=@ARGV\n";
