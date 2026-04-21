  Bugs Documented/Worked Around (not fixable without major changes)

  1. Arrays don't share state in closures - Arrays captured by closures are copied, not shared. Use
      arrayref ($tokens = []) instead.


