# Legacy `sub NAME (PROTO)` prototypes that mix mandatory + optional
# arg markers. The semicolon (`;`) starts the optional-args region; the
# chars after it (`$`, `$$`, `@`, `%`, `\@`, `\%`, `&`, `*`) describe
# what kind of arg can appear there.
#
# Pre-fix stryke saw `$;` as the special variable `$;` (multi-dim
# subscript separator) and tried to parse the prototype as a modern
# signature, blowing up on the next sigil. With the dispatcher fixed to
# require an identifier name before committing to signature mode, every
# shape below routes to `parse_legacy_sub_prototype_tail` and runs.
#
# This is Test.pm:390's exact shape (`sub ok ($;$$)`), plus the common
# Try::Tiny prototype (`sub try (&;@)`).

# 1. `($;$$)` — Test.pm's `sub ok`.
sub ok_proto ($;$$) {
    "args=" . scalar(@_);
}
print ok_proto(1), "\n";
print ok_proto(1, 2), "\n";
print ok_proto(1, 2, 3), "\n";

# 2. `($;$)` — one mandatory, one optional scalar.
sub one_or_two ($;$) {
    my ($a, $b) = @_;
    defined($b) ? "$a/$b" : "$a/-";
}
print one_or_two(10), "\n";
print one_or_two(10, 20), "\n";

# 3. `($;@)` — mandatory scalar plus optional rest-array.
sub head_and_tail ($;@) {
    my ($h, @t) = @_;
    "head=$h tail=" . scalar(@t);
}
print head_and_tail("x"), "\n";
print head_and_tail("x", 1, 2, 3), "\n";

# 4. `($;%)` — mandatory scalar plus optional rest-hash.
sub kv_after ($;%) {
    my ($name, %opts) = @_;
    "name=$name keys=" . join(",", sort keys %opts);
}
print kv_after("k"), "\n";
print kv_after("k", a => 1, b => 2), "\n";

# 5. `(&;@)` — code block then optional list (Try::Tiny's `sub try`).
sub run_block (&;@) {
    my $code = shift;
    my @rest = @_;
    "ret=" . $code->() . " rest=" . scalar(@rest);
}
print run_block(sub { 99 }), "\n";
print run_block(sub { "z" }, "a", "b"), "\n";
