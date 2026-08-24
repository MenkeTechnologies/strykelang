# getpw*/getgr* return perl's exact field list.
#
# perl's pp_gpwent always pushes TEN elements —
# (name, passwd, uid, gid, quota, comment, gcos, dir, shell, expire) —
# and getgr* pushes FOUR: (name, passwd, gid, members).
# Contents are machine-specific, so this prints shapes and cross-entry
# agreement rather than literal names; both interpreters see the same
# passwd/group database, so any real divergence still shows up.
setpwent();
my @ent = getpwent();
endpwent();
print "pwent_fields:", scalar(@ent), "\n";

my @by_name = getpwnam($ent[0]);
print "pwnam_fields:", scalar(@by_name), "\n";
print "pwnam_agrees:", (join("\x1f", map { defined($_) ? $_ : "\0" } @by_name) eq
                        join("\x1f", map { defined($_) ? $_ : "\0" } @ent) ? 1 : 0), "\n";

# The passwd field is the shadow placeholder, never a literal "x" invented
# by the implementation.
print "pw_passwd_is_shadowed:", ($ent[1] =~ /\A[*x]?\z/ ? 1 : 0), "\n";
print "quota_defined:", (defined($ent[4]) ? 1 : 0), "\n";
print "comment_defined:", (defined($ent[5]) ? 1 : 0), "\n";
print "expire_defined:", (defined($ent[9]) ? 1 : 0), "\n";
print "uid_numeric:", ($ent[2] =~ /\A-?\d+\z/ ? 1 : 0), "\n";

my @byuid = getpwuid($ent[2]);
print "pwuid_fields:", scalar(@byuid), "\n";

setgrent();
my @gr = getgrent();
endgrent();
print "grent_fields:", scalar(@gr), "\n";
print "gr_passwd_is_shadowed:", ($gr[1] =~ /\A[*x]?\z/ ? 1 : 0), "\n";
print "gr_gid_numeric:", ($gr[2] =~ /\A-?\d+\z/ ? 1 : 0), "\n";

my @grnam = getgrnam($gr[0]);
print "grnam_fields:", scalar(@grnam), "\n";
print "grnam_agrees:", (join("\x1f", @grnam) eq join("\x1f", @gr) ? 1 : 0), "\n";
