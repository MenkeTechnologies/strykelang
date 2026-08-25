# `sort Pkg::cmp LIST` — a package-qualified comparator name.
#
# The lexer splits a qualified name into Ident/PackageSep/Ident, and the
# `sort SUBNAME LIST` parse only accepted a single Ident, so this was a hard
# compile error rather than a sort.
#
# $a/$b are package variables of the package the sort is COMPILED in, so the
# comparator has to live in that same package to see them — which is how
# qualified comparators are used in practice.
package Sorters;
sub by_len  { length($a) <=> length($b) }
sub by_num  { $a <=> $b }
sub go {
    print join(",", sort Sorters::by_len qw(aaa a aa)), "\n";
    print join(",", sort Sorters::by_num (10, 2, 33, 4)), "\n";
    # No list at all.
    my @empty = sort Sorters::by_num ();
    print scalar(@empty), "\n";
    # Comparator name reached through a deeper package path.
    print join(",", sort Sorters::by_num (3, 1, 2)), "\n";
}

package Deep::Pkg;
sub cmp_desc { $b <=> $a }
sub run { print join(",", sort Deep::Pkg::cmp_desc (1, 5, 3)), "\n" }

package main;
Sorters::go();
Deep::Pkg::run();
