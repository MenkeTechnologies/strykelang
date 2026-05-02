cat README.md and NAMESPACE_FUNCTIONS.md in this directory.  They are symlinks to the main files at root of repo.

the rosetta code list is in list.txt.  Find missing ones in list.txt.

then implement missing rosetta code examples in t/ following the patterns established for idiomatic stryke code.  Use |> and ~> as much as possible.  You must have tests in your solution.

update list.txt when ur done.  mark the item as done with [x].

any new FILENAME must be run with stryke --no-interop test t/FILENAME.  Your solutions must pass all tests or it must be deleted.

FILENAME is the name of your new rosetta code example.

do not litter $PWD with temp files, put them in /tmp

You can not modify any current tests in t/.
