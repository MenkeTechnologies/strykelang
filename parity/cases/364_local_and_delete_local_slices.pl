# local on hash/array slices and `delete local` restore the saved elements on
# scope exit, including elements that did not exist before.
our %h = (a => 1, b => 2, c => 3);
our @a = (10, 20, 30, 40);

sub show_h { join ",", map { "$_=$h{$_}" } sort keys %h }
sub show_a { join ",", map { defined $_ ? $_ : "undef" } @a }

{
    local @h{qw(a b)} = (100, 200);
    print "in: ", show_h(), "\n";
}
print "out: ", show_h(), "\n";

{
    local @h{qw(a z)} = (7, 8);
    print "in: ", show_h(), "\n";
}
print "out: ", show_h(), "\n";

{
    local @h{qw(a b)};
    print "in: ", join(",", map { defined $h{$_} ? $h{$_} : "undef" } sort keys %h), "\n";
}
print "out: ", show_h(), "\n";

{
    local @a[0, 2] = (1, 3);
    print "in: ", show_a(), "\n";
}
print "out: ", show_a(), "\n";

sub gone { delete local $h{a}; return join ",", sort keys %h }
print "gone: ", gone(), "\n";
print "back: ", show_h(), "\n";

sub gone_idx { my $v = delete local $a[1]; return "$v/" . (defined $a[1] ? $a[1] : "undef") }
print "idx: ", gone_idx(), "\n";
print "idx back: ", show_a(), "\n";

sub nested {
    local @h{qw(a b)} = (0, 0);
    {
        local $h{a} = 9;
        print "nested in: ", show_h(), "\n";
    }
    print "nested mid: ", show_h(), "\n";
}
nested();
print "nested out: ", show_h(), "\n";
