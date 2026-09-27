# caller: package and line of the call site, qualified sub names, `(eval)`
# frames, the empty list past the outermost frame, scalar context = package.
package Foo;
sub inner {
    my @bare = caller;
    my @c0 = caller(0);
    my @c1 = caller(1);
    print scalar(@bare), " [@bare]\n";
    print "[@c0[0..3]]\n";
    print "[@c1[0..3]]\n";
    print scalar(caller), "\n";
    my @none = caller(5);
    print scalar(@none), "\n";
}
sub outer { inner() }
package main;
Foo::outer();
sub me { (caller(0))[3] }
print me(), "\n";
sub ev { eval { print((caller(0))[3], " ", (caller(1))[3], "\n") } }
ev();
sub ctx { my $w = (caller(0))[5]; print defined $w ? "[$w]" : "undef", "\n" }
my @l = ctx();
my $s = ctx();
print defined(caller) ? "d" : "u", "\n";
my @top = caller;
print scalar(@top), "\n";
