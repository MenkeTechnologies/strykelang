  Bugs Documented/Worked Around (not fixable without major changes)

  The following are known limitations that tests work around:

  1. Arrays don't share state in closures - Arrays captured by closures are copied, not shared. Use
      arrayref ($tokens = []) instead.


  2. [@arr] as implicit function return doesn't work - Assigning to a variable first (my $ref =
     [@arr]; $ref) works as a workaround.


  3. Hash parameters to functions don't work - fn foo(%h) { } receives empty hash. Use hashref or
     extract values before passing.


  4. //= in some contexts causes parse errors - Use unless exists pattern instead.


  5. Complex deref like @{$hash{key}} causes parse issues - Extract to variable first.


