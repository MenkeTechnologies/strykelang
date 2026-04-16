use strict;
use warnings;
# `die EXPR` accepts strings, hashrefs, blessed refs — `$@` preserves the value.
eval { die "string error\n" };
print "ref: '", ref($@), "' val: $@";   # ref:'' val: string error\n

eval { die { code => 42, msg => "bad" } };
print "ref: ", ref($@), "\n";           # HASH
print "code=$@->{code} msg=$@->{msg}\n";

eval { die bless({ id => 7 }, "My::Err") };
print "ref: ", ref($@), "\n";           # My::Err
print "id=$@->{id}\n";
print "isa My::Err: ", ($@->isa("My::Err") ? "y" : "n"), "\n";

# eval { die } with no arg uses $@ as the message
$@ = "stored\n";
eval { die };
print "die no-arg: $@";
