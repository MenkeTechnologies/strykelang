# Package-stash resolution for aggregates, `our`, `local` and `__PACKAGE__`.
#
# A bare `%h` assigned inside a package must read back through the same stash it was
# written to, and the names Perl keeps in `main` (`%ENV`, `@ARGV`, `$_`, `$1`, ...) must
# stay in `main` no matter which package is in effect.

# Write and read of a package hash agree inside the package that owns it.
package Foo;
%map = (k => "v", n => 2);
print "1: $map{k} $map{n}\n";
package main;
print "2: $Foo::map{k}\n";

# Arrays behave the same way.
package Foo;
@list = (7, 8, 9);
print "3: @list ", scalar(@list), "\n";
package main;
print "4: @Foo::list\n";

# Two packages keep separate aggregates under the same bare name.
package P1; %c = (n => 10);
package P2; %c = (n => 20);
package main;
print "5: $P1::c{n} $P2::c{n}\n";

# `%ENV` / `@ARGV` inside a package are still `main`'s, not the package's.
package Q;
sub env_and_argv {
    return (defined $ENV{PATH} ? "env" : "noenv") . " " . scalar(@ARGV);
}
# `local $_` localises `$main::_`, so the topic is visible inside the package.
sub topic { local $_ = "topic"; return $_ }
# Capture groups are `main`'s magic variables regardless of package.
sub cap { "abc42" =~ /(\d+)/; return $1 }
package main;
print "6: ", Q::env_and_argv(), "\n";
print "7: ", Q::topic(), "\n";
print "8: ", Q::cap(), "\n";

# `my` inside a package sub stays lexical for every binding form.
package R;
sub lex {
    my $a1 = shift;
    my ($a2) = @_;
    my @a3 = (1, 2);
    my %a4 = (x => 3);
    return "$a1 $a2 " . scalar(@a3) . " $a4{x}";
}
package main;
print "9: ", R::lex("one", "two"), "\n";

# `our` in a package writes the package stash and stays lexically visible.
package Bar;
our $shared = "bar-value";
sub read_shared { return $shared }
package main;
print "10: ", Bar::read_shared(), " $Bar::shared\n";

# `local` on a package global restores on scope exit; `__PACKAGE__` tracks the package.
package T;
our $g = "orig";
sub with_local { local $g = "temp"; return inner() }
sub inner { return $g }
sub who { return __PACKAGE__ }
package main;
print "11: ", T::with_local(), " $T::g\n";
print "12: ", T::who(), " ", __PACKAGE__, "\n";
