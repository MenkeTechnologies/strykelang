# `use File::Basename;` with no import list imports its @EXPORT (fileparse,
# fileparse_set_fstype, basename, dirname). Under --compat the parser reads the
# list from File/Basename.pm, so `basename`/`dirname` call the module's subs
# instead of being rejected as stryke extensions.
use strict; use warnings;
use File::Basename;
print basename("/a/b/c.txt"), " ", dirname("/a/b/c.txt"), "\n";
print basename("/a/b/c.txt", ".txt"), " ", dirname("/a/b/"), " ", dirname("c"), "\n";
my ($name, $dir, $suffix) = fileparse("/x/y.tar.gz", qr/\.[^.]*/);
print "$name|$dir|$suffix\n";
