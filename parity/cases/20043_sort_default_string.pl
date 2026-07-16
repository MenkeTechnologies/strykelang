# Default sort is string-wise, so 100 sorts before 2.
print join(",", sort 10, 9, 100, 2), "\n";
print join(",", sort { $a <=> $b } 10, 9, 100, 2), "\n";
print join(",", sort "b", "A", "a", "B"), "\n";
print join(",", reverse sort 10, 9, 100, 2), "\n";
