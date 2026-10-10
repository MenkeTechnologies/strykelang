//! Statement-level differential test against stock `perl(1)`.
//!
//! `diff_perl_gen` covers scalar expressions. This generator covers whole
//! statements: loops and loop control, `my` / `our` / `local` scoping,
//! `wantarray`, `sort` / `reverse` / `map` / `grep`, `sprintf` / `pack` /
//! `unpack`, regex features, hash and array slices, string operators and
//! heredocs.
//!
//! Each snippet is a template whose `§kind§` / `§kindN§` holes are filled from
//! a seeded RNG (the same hole name inside one snippet always gets the same
//! value). Every snippet runs as its own program under `perl` and
//! `stryke --compat`; stdout and success/failure must match. A divergence
//! prints the expanded snippet, so it reproduces directly as `perl FILE`.
//!
//! If `perl` is not on `$PATH` the tests no-op, like the other parity suites.

use std::collections::HashMap;
use std::fs;
use std::process::Command;

/// xorshift64* — deterministic, dependency-free.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len() as u64) as usize]
    }
}

const WORDS: &[&str] = &[
    "apple", "Banana", "cherry", "date", "Elder", "fig", "grape", "kiwi", "lime", "mango",
];

const TEXTS: &[&str] = &[
    "the quick brown fox",
    "a1b22c333",
    "key=value; k2=v2",
    "  leading and trailing  ",
    "2024-03-15T10:20:30",
    "CamelCaseWordsHere",
    "one,two,,four,,",
    "x",
    "aaa bbb aaa ccc",
    "foo.bar.baz",
];

const REGEXES: &[&str] = &[
    "(\\w+)",
    "(\\d+)",
    "([a-c])(\\d)",
    "\\b(\\w)",
    "(a+)",
    "([A-Z][a-z]+)",
    "(\\w+)=(\\w+)",
    "(?<=a)(.)",
    "(.)\\1",
    "(\\s+)",
    "(o+?)",
];

const FORMATS: &[&str] = &[
    "%5d|", "%-5d|", "%05d", "%+d", "%x", "%X", "%#x", "%o", "%#o", "%b", "%#b", "%e", "%.3e",
    "%g", "%.2f", "%8.3f|", "%-8.2f|", "%s", "%10s|", "%-10s|", "%.2s", "%c", "%5.1f%%", "%03b",
];

/// Fill one `§kind§` hole.
fn hole(rng: &mut Rng, kind: &str) -> String {
    match kind {
        "n" => (1 + rng.below(9)).to_string(),
        "m" => rng.below(41).to_string(),
        "z" => (rng.below(19) as i64 - 9).to_string(),
        "w" => rng.pick(WORDS).to_string(),
        "W" => format!(
            "qw({} {} {} {})",
            rng.pick(WORDS),
            rng.pick(WORDS),
            rng.pick(WORDS),
            rng.pick(WORDS)
        ),
        "L" => (0..2 + rng.below(5))
            .map(|_| (rng.below(40) as i64 - 10).to_string())
            .collect::<Vec<_>>()
            .join(", "),
        "T" => format!("\"{}\"", rng.pick(TEXTS)),
        "R" => rng.pick(REGEXES).to_string(),
        "F" => rng.pick(FORMATS).to_string(),
        "c" => rng.pick(&["a", "b", "c", "e", "o", "1"]).to_string(),
        _ => panic!("unknown hole kind {kind}"),
    }
}

fn expand(rng: &mut Rng, template: &str) -> String {
    let mut out = String::new();
    let mut cache: HashMap<String, String> = HashMap::new();
    let mut parts = template.split('§');
    out.push_str(parts.next().unwrap_or(""));
    let mut in_hole = true;
    for part in parts {
        if in_hole {
            let kind: String = part
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            let value = cache
                .entry(part.to_string())
                .or_insert_with(|| hole(rng, &kind))
                .clone();
            out.push_str(&value);
        } else {
            out.push_str(part);
        }
        in_hole = !in_hole;
    }
    out
}

/// Statement templates. Output must be deterministic: hashes are always
/// walked through `sort keys`.
const TEMPLATES: &[&str] = &[
    // ── loops ──
    r#"my @r; for (my $i = §z1§; $i < §m1§; $i += §n1§) { next if $i % §n2§ == 0; last if $i > §m2§; push @r, $i } print "@r\n";"#,
    r#"my $s = 0; foreach my $x (§z1§ .. §m1§) { $s += $x * §n1§ } print "$s\n";"#,
    r#"my $n = §m1§ + 1; my @t; while ($n > 0) { push @t, $n; $n = int($n / §n1§ - 1) if $n > 1; last if @t > 12; $n-- } print join(",", @t), "\n";"#,
    r#"my $c = 0; OUTER: for my $i (1 .. §n1§) { for my $j (1 .. §n2§) { next OUTER if $j > $i; $c += $j; last OUTER if $c > §m1§ } } print "$c\n";"#,
    r#"my $i = 0; my @r; do { push @r, $i; $i += §n1§ } while ($i < §m1§); print "@r\n";"#,
    r#"my $i = §m1§; my @r; until ($i <= 0) { push @r, $i; $i -= §n1§ + 1 } print "@r\n";"#,
    r#"my @a = (§L1§); $_ *= §n1§ for @a; print "@a\n";"#,
    r#"my @a = (§L1§); for my $x (@a) { $x = $x > 0 ? $x : -$x } print "@a\n";"#,
    r#"my @r; push @r, $_ * §n1§ for grep { $_ % 2 } 1 .. §n2§ + 5; print "@r\n";"#,
    r#"my %h = map { $_ => length $_ } §W1§; for my $k (sort keys %h) { print "$k=$h{$k};" } print "\n";"#,
    r#"my @r; for my $i (reverse 1 .. §n1§) { push @r, $i; } my $j = 0; $j++ while $j < §m1§; print "@r $j\n";"#,
    r#"my @a = (§L1§); my $i = 0; my $out = ""; while (defined(my $x = shift @a)) { $i++; next if $x < 0; $out .= "$i:$x "; } print "$out\n";"#,
    r#"my $out = ""; for my $i (1 .. §n1§) { for my $j (1 .. §n2§) { $out .= $i * $j; $out .= $j == §n2§ ? ";" : "," } } print "$out\n";"#,
    r#"my @a = (1 .. §n1§ + 3); my @b; while (my ($i, $v) = each @a) { push @b, $i + $v } print "@b\n";"#,
    r#"my $k = 0; { $k++; redo if $k < §n1§; } print "$k\n";"#,
    r#"my @r; for my $i (1 .. 20) { next unless $i % §n1§ == 0; push @r, $i; last if @r >= §n2§ } print scalar(@r), ":@r\n";"#,
    r#"my $x = ""; for (my ($i, $j) = (0, §n1§); $i < $j; $i++, $j--) { $x .= "$i$j," } print "$x\n";"#,
    // ── scoping ──
    r#"our $g = §n1§; sub show { return $g } { local $g = §n2§; print show(), "\n"; } print show(), "\n";"#,
    r#"my $x = §n1§; { my $x = §n2§; $x++; print "$x\n" } print "$x\n";"#,
    r#"my @subs; for my $i (1 .. §n1§) { push @subs, sub { return $i * $_[0] } } print join(",", map { $_->(§n2§) } @subs), "\n";"#,
    r#"sub mk { my $n = shift; return sub { return $n++ } } my $c = mk(§n1§); my @r = ($c->(), $c->(), $c->()); my $d = mk(§n2§); print "@r ", $d->(), "\n";"#,
    r#"our @arr = (1, 2, 3); sub total { my $t = 0; $t += $_ for @arr; $t } { local @arr = (§L1§); print total(), "\n"; } print total(), "\n";"#,
    r#"our %cfg = (a => 1, b => 2); sub dump_cfg { join ",", map { "$_=$cfg{$_}" } sort keys %cfg } { local $cfg{a} = §n1§; local $cfg{c} = §n2§; print dump_cfg(), "\n"; } print dump_cfg(), "\n";"#,
    r#"our @l = (§L1§); sub lst { "@l" } { local $l[1] = 99; print lst(), "\n"; } print lst(), "\n";"#,
    r#"{ local $, = "-"; local $\ = "!\n"; print "a", "b", §n1§; } print "x\n";"#,
    r#"my @a = (1, 2, 3); { local $" = ":"; print "@a\n"; } print "@a\n";"#,
    r#"our $v = "outer"; sub inner { $v } sub wrap { local $v = "wrapped"; inner() } print wrap(), " ", inner(), "\n";"#,
    r#"my $x = §n1§; my $f = sub { $x * 2 }; $x = §n2§; print $f->(), "\n";"#,
    r#"my ($a1, $b1) = (§n1§, §n2§); ($a1, $b1) = ($b1, $a1); print "$a1 $b1\n";"#,
    r#"my $s = 0; for my $i (1 .. §n1§) { my $s2 = $s; $s2 += $i; $s = $s2 } print "$s\n";"#,
    r#"package Foo; our $name = "foo"; sub get { $name } package main; print Foo::get(), " ", $Foo::name, "\n";"#,
    r#"my %h = (a => 1); { local $h{a}; print defined $h{a} ? "def" : "undef", "\n"; } print "$h{a}\n";"#,
    r#"our $x = 1; { local $x = $x + §n1§; { local $x = $x * §n2§; print "$x\n"; } print "$x\n"; } print "$x\n";"#,
    // ── wantarray ──
    r#"sub ctx { return wantarray ? "list" : defined(wantarray) ? "scalar" : "void" } my @l = ctx(); my $s = ctx(); my ($f) = ctx(); print "$l[0] $s $f ", scalar(ctx()), "\n";"#,
    r#"sub lst { return (§L1§) } my $c = lst(); my ($f) = lst(); my $n = () = lst(); my @a = lst(); print "$c $f $n ", scalar(@a), "\n";"#,
    r#"sub arr { my @a = (§L1§); return @a } my $c = arr(); my @b = arr(); print "$c ", scalar(@b), "\n";"#,
    r#"sub w { return wantarray } my @a = (w(), w()); my %h = (k => w()); my $s = w() ? "t" : "f"; print scalar(@a), " $a[0] ", $h{k}, " $s\n";"#,
    r#"sub ctx { wantarray ? "L" : defined wantarray ? "S" : "V" } print "[", ctx(), "][", scalar(ctx()), "]\n"; my $x = [ctx()]; my $y = {a => ctx()}; print "$x->[0] $y->{a}\n";"#,
    r#"sub pass { return inner_ctx() } sub inner_ctx { wantarray ? "list" : "scalar" } my @a = pass(); my $s = pass(); print "$a[0] $s\n";"#,
    r#"sub cnt { my @r = (§L1§); return @r[0 .. 1] } my $c = cnt(); my @l = cnt(); print "$c @l\n";"#,
    r#"sub e { return } my @a = e(); my $s = e(); print scalar(@a), defined($s) ? "def" : "undef", "\n";"#,
    r#"sub last_stmt { my @a = (§L1§); @a } my $n = last_stmt(); my @b = last_stmt(); print "$n ", scalar(@b), "\n";"#,
    r#"sub h { my %h = (a => 1, b => 2); return %h } my %g = h(); print join(",", map {"$_$g{$_}"} sort keys %g), "\n";"#,
    // ── sort / reverse / map / grep ──
    r#"print join(",", sort (§L1§)), "\n";"#,
    r#"print join(",", sort { $b <=> $a } (§L1§)), "\n";"#,
    r#"print join(" ", sort { length($a) <=> length($b) or $a cmp $b } §W1§), "\n";"#,
    r#"print join(" ", reverse sort { lc($a) cmp lc($b) } §W1§), "\n";"#,
    r#"print join(",", map { ($_, $_ * 2) } (§L1§)), "\n";"#,
    r#"my @p = map { [$_ % 3, $_] } (§L1§); print join(",", map { "$_->[0]:$_->[1]" } sort { $a->[0] <=> $b->[0] } @p), "\n";"#,
    r#"my @w = grep { /a/ } §W1§; my $n = grep { /e/i } §W1§; my ($first) = grep { length > 3 } §W1§; print scalar(@w), " $n ", $first // "none", "\n";"#,
    r#"my %h = map { $_ => 1 } §W1§; print join(",", sort keys %h), " ", scalar(keys %h), "\n";"#,
    r#"my %h = (a => 3, b => 1, c => 2); print join(",", sort { $h{$a} <=> $h{$b} } keys %h), "\n";"#,
    r#"my @s = sort { $a->{n} <=> $b->{n} } map { { n => $_ } } (§L1§); print join(",", map { $_->{n} } @s), "\n";"#,
    r#"print scalar reverse("ab", "cd"), "|", join(",", reverse 1 .. §n1§), "|", scalar(reverse("hello")), "\n";"#,
    r#"my @a = (§L1§); my @i = sort { $a[$a] <=> $a[$b] } 0 .. $#a; print "@i\n";"#,
    r#"my @a = (§L1§); my ($min) = sort { $a <=> $b } @a; my ($max) = reverse sort { $a <=> $b } @a; print "$min $max\n";"#,
    r#"sub by_len { length($a) <=> length($b) } print join(" ", sort by_len §W1§), "\n";"#,
    r#"my $cmp = sub { $b cmp $a }; print join(" ", sort $cmp §W1§), "\n";"#,
    r#"print join(",", map { sprintf "%02d", $_ } grep { $_ & 1 } 1 .. §n1§ + 5), "\n";"#,
    r#"my @a = map { $_ * $_ } grep { $_ % 2 == 0 } 1 .. §m1§; print scalar(@a), " ", (@a ? $a[-1] : "none"), "\n";"#,
    r#"my @nested = map { [ map { $_ * §n1§ } 1 .. $_ ] } 1 .. 3; print join("|", map { join(",", @$_) } @nested), "\n";"#,
    r#"my @u = do { my %s; grep { !$s{$_}++ } (§L1§, §L1§) }; print "@u\n";"#,
    // ── sprintf / pack / unpack ──
    r#"printf "%s\n", join ",", map { sprintf "§F1§", $_ } (§n1§, §z1§, §m1§);"#,
    r#"print sprintf("%-*s|%*d|%.*f", §n1§ + 3, "ab", §n2§ + 3, §m1§, §n3§ % 4, 3.14159265), "\n";"#,
    r#"print sprintf('%2$s %1$s %2$s', "a", "b"), sprintf(" %vd", "1.22.333"), "\n";"#,
    r#"print unpack("H*", pack("N", §m1§ * 1000)), " ", unpack("H*", pack("n", §m1§ * 100)), " ", unpack("H*", pack("v", §m1§ * 100)), "\n";"#,
    r#"print join(",", unpack("C*", §T1§)), "\n";"#,
    r#"my $p = pack("A3 a3 Z4", "ab", "cd", "efghij"); print length($p), ":", unpack("H*", $p), "\n";"#,
    r#"my @u = unpack("A2 x A2 a*", "ab-cdefgh"); print join("|", @u), "\n";"#,
    r#"print unpack("%32C*", §T1§) % 65535, " ", unpack("%8b*", pack("C", §m1§)), "\n";"#,
    r#"print join(",", unpack("(A2)*", "aabbccd")), " ", unpack("H*", pack("w", §m1§ * 300)), "\n";"#,
    r#"my $p = pack("N/a*", §T1§); print unpack("H*", $p), " ", join("|", unpack("N/a*", $p)), "\n";"#,
    r#"print unpack("H*", pack("s< l> q", -§n1§, §m1§, §m2§)), "\n";"#,
    r#"my @a = (§L1§); print unpack("H*", pack("n*", @a)), " ", join(",", unpack("n*", pack("n*", @a))), "\n";"#,
    r#"print join(",", unpack("U*", "\x{263A}ab")), " ", length(pack("U", 0x263A)), "\n";"#,
    r#"print join(",", map { sprintf "%03o", $_ } unpack("C*", pack("B8", "1010101" . (§n1§ % 2)))), "\n";"#,
    r#"print unpack("H*", pack("f", 1.5)), " ", unpack("H*", pack("d>", -2.25)), " ", unpack("f", pack("f", 0.5)), "\n";"#,
    r#"my $s = sprintf("%s-%s", "a" x §n1§, join(":", 1 .. §n2§)); print length($s), " $s\n";"#,
    // ── regex ──
    r#"my $s = §T1§; my @m = $s =~ /§R1§/g; print scalar(@m), ":", join("|", @m), "\n";"#,
    r#"my $s = §T1§; if ($s =~ /§R1§/) { print "pre[$`] m[$&] post[$']\n"; } else { print "no\n"; }"#,
    r#"my $s = §T1§; (my $t = $s) =~ s/§R1§/<$1>/g; print "$t\n";"#,
    r#"my $s = §T1§; my $n = () = $s =~ /§R1§/g; my $c = ($s =~ s/§R1§/X/g); print "$n ", $c || 0, " $s\n";"#,
    r#"my $s = "a1b22c333"; (my $t = $s) =~ s/(\d+)/$1 * 2/ge; print "$t\n";"#,
    r#"my $s = §T1§; print join("|", split(/[ ,;=]+/, $s)), "\n";"#,
    r#"my $s = §T1§; print scalar(my @p = split(/,/, $s)), " ", scalar(my @q = split(/,/, $s, -1)), " ", join("|", split(/,/, $s, 2)), "\n";"#,
    r#"print join("|", split(//, "abcd", 3)), " ", join("|", split(" ", "  a  b c ")), " ", join("|", split(/(,)/, "a,b")), "\n";"#,
    r#"my $s = §T1§; my @pos; while ($s =~ /§R1§/g) { push @pos, pos($s) } print "@pos\n";"#,
    r#"my $s = "hello world"; (my $t = $s) =~ tr/a-y/b-z/; my $c = ($s =~ tr/o//); print "$t $c\n";"#,
    r#"my $s = §T1§; my $r = $s =~ s/(\w+)/\u$1/gr; print "$r|$s\n";"#,
    r#"if ("2024-03-15" =~ /^(?<y>\d+)-(?<m>\d+)-(?<d>\d+)$/) { print join(",", map { "$_=$+{$_}" } sort keys %+), "\n"; }"#,
    r#"my $s = "aXbXc"; my @f = split /X/, $s; my $j = join "-", @f; $j =~ s/-/+/; print "$j ", scalar(@f), "\n";"#,
    r#"my $s = "foo bar baz"; my ($a1, $b1) = $s =~ /(\w+) (\w+)/; print "$a1,$b1 $-[0] $+[0] $-[2] $+[2]\n";"#,
    r#"my $s = "abcabc"; my $n = 0; $n++ while $s =~ /b/g; my @all = ("abcabc" =~ /(?=(bc))/g); print "$n @all\n";"#,
    r#"my $re = qr/(\d+)/i; my @a = "a1 b22" =~ /$re/g; my $s = "x" . ("ab" =~ /^a/ ? "y" : "n"); print "@a $s $re\n";"#,
    r#"my $s = "line1\nline2\nline3"; my @l = $s =~ /^(\w+)$/mg; my $c = () = $s =~ /./sg; print scalar(@l), " $c\n";"#,
    r#"my $s = "a.b.c"; my @p = split /\./, $s; my $q = quotemeta($s); print scalar(@p), " $q\n";"#,
    r#"my $s = "aaa"; (my $t = $s) =~ s/a/b/; (my $u = $s) =~ s/a*?/X/; print "$t $u\n";"#,
    r#"my $s = "The Cat"; print lc($s) =~ /cat/ ? "y" : "n", $s =~ /CAT/i ? "y" : "n", $s =~ /^\s*the\s+/i ? "y" : "n", "\n";"#,
    r#"my $str = "x=1,y=2,z=3"; my %h = $str =~ /(\w)=(\d)/g; print join(",", map { "$_$h{$_}" } sort keys %h), "\n";"#,
    r#"my $s = "abc"; $s =~ s/(?<first>.)(.)/$2$+{first}/; print "$s\n"; my $t = "a b"; $t =~ s/(\w) (\w)/$2 $1/; print "$t\n";"#,
    r#"$_ = §T1§; my $n = tr/a-z//; my $m = s/\s+//g; print "$n ", $m || 0, " $_\n";"#,
    // ── hash and array slices ──
    r#"my %h; @h{§W1§} = (1 .. 4); my @v = @h{(sort keys %h)[0, 1]}; print join(",", map { "$_=$h{$_}" } sort keys %h), " @v\n";"#,
    r#"my %h = (a => 1, b => 2, c => 3, d => 4); delete @h{qw(a c)}; print join(",", map { "$_=$h{$_}" } sort keys %h), "\n";"#,
    r#"my %h = (a => 1, b => 2, c => 3); my %s = %h{'a', 'b'}; print join(",", map { "$_=$s{$_}" } sort keys %s), "\n";"#,
    r#"my %h = (a => 1, b => 2, c => 3); my ($x, $y) = @h{qw(a b)}; my $r = \%h; my @z = @{$r}{qw(b c)}; my @w = @$r{qw(a c)}; print "$x $y @z @w\n";"#,
    r#"my $r = { a => 1, b => 2, c => 3 }; my @v = $r->@{qw(a b)}; my %s = $r->%{qw(c)}; print "@v ", join(",", %s), "\n";"#,
    r#"my %h = (a => [1, 2], b => { c => 3 }); print "$h{a}[1] $h{b}{c} ", exists $h{b}{d} ? "y" : "n", " ", scalar(@{$h{a}}), "\n";"#,
    r#"my @a = (§L1§); my @s = @a[1 .. $#a]; my @n = @a[-2, -1]; my ($first, @rest) = @a; print scalar(@s), " @n $first ", scalar(@rest), "\n";"#,
    r#"my @a = (1 .. 10); my @r = splice(@a, 2, 3); splice(@a, 1, 0, 'x', 'y'); splice(@a, -2); print "@r | @a\n";"#,
    r#"my @a = (1 .. 5); @a[0, 1] = @a[1, 0]; my @b = (@a) x 2; $#a = 2; print "@a | ", scalar(@b), " | $#a\n";"#,
    r#"my %h = (a => 1, b => 2); my %inv = reverse %h; my @k = sort keys %inv; my $cnt = keys %h; print "@k $cnt\n";"#,
    r#"my %h; $h{$_}++ for qw(a b a c a b); print join(",", map { "$_$h{$_}" } sort { $h{$b} <=> $h{$a} || $a cmp $b } keys %h), "\n";"#,
    r#"my %h = (x => 1); my $v = $h{y}{z}; print exists $h{y} ? "autoviv" : "none", " ", join(",", sort keys %h), "\n";"#,
    r#"my @a = (1 .. 3); my %h; @h{@a} = map { $_ * $_ } @a; my @k = sort { $a <=> $b } keys %h; print "@k @h{@k}\n";"#,
    r#"my %h = (a => 1, b => 2, c => 3); my @pairs; while (my ($k, $v) = each %h) { push @pairs, "$k$v" } print join(",", sort @pairs), "\n";"#,
    r#"my @a = (3, 1, 2); my ($mn, $mx) = (sort { $a <=> $b } @a)[0, -1]; my $last = (§L1§)[-1]; print "$mn $mx $last\n";"#,
    r#"my %h = map { $_ => 1 } 1 .. 5; delete @h{grep { $_ % 2 } keys %h}; print join(",", sort keys %h), "\n";"#,
    r#"my @aoa = ([1, 2], [3, 4]); push @{$aoa[2]}, 5; $aoa[0][2] = 9; print scalar(@aoa), " ", join(",", map { scalar @$_ } @aoa), " $aoa[-1][0]\n";"#,
    r#"our %g = (a => 1, b => 2); sub f { delete local $g{a}; return join ",", sort keys %g } print f(), " ", join(",", sort keys %g), "\n";"#,
    r#"our %g = (a => 1, b => 2, c => 3); sub f { local @g{qw(a b)} = (10, 20); return join ",", map { "$_$g{$_}" } sort keys %g } print f(), " ", join(",", map { "$_$g{$_}" } sort keys %g), "\n";"#,
    r#"our @g = (1, 2, 3, 4); sub f { local @g[0, 2] = (9, 8); return "@g" } print f(), " @g\n";"#,
    // ── string operators ──
    r#"my $s = "abcdefgh"; substr($s, §n1§ % 4, 2) = "XYZ"; print "$s\n"; my $t = "abcdef"; substr($t, 1, 2, "--"); print "$t ", substr($t, -§n2§ % 3 - 1), "\n";"#,
    r#"my $s = §T1§; print index($s, "§c1§"), " ", rindex($s, "§c1§"), " ", index($s, "§c1§", §n1§), " ", rindex($s, "§c1§", §n2§), "\n";"#,
    r#"my $s = "az"; my @r; for (1 .. 4) { $s++; push @r, $s } my $t = "Zz"; $t++; my $u = "a9"; $u++; print "@r $t $u\n";"#,
    r#"print join(",", "aa" .. "ad"), " ", join(",", "x" .. "ab"), " ", join(",", "08" .. "11"), "\n";"#,
    r#"my @a = (1, 2) x §n1§; my $s = "-" x §n2§; my @e = (0) x 0; print scalar(@a), " $s ", scalar(@e), "\n";"#,
    r#"my $s = "Hello, World"; print lc $s, "|", uc $s, "|", ucfirst(lc $s), "|", lcfirst($s), "|", length $s, "\n";"#,
    r#"my $s = "line\n"; my $r = chomp($s); my $t = "word"; my $c = chop($t); print "$r [$s] $c [$t]\n";"#,
    r#"my $s = join "", map { chr(ord("a") + $_) } 0 .. §n1§; print "$s ", ord($s), " ", sprintf("%vd", $s), "\n";"#,
    r#"my $s = "abc"; my $r = reverse $s; my $u = ucfirst reverse $s; print "$r $u ", "abc" lt "abd" ? "lt" : "ge", " ", "10" <=> "9", " ", "10" cmp "9", "\n";"#,
    r#"my ($a1, $b1) = ("5 apples", "3.5kg"); { no warnings; print $a1 + $b1, " ", "abc" + 1, " ", "0x10" + 0, " ", "1e2" + 0, " ", ".5" + 0, "\n"; }"#,
    r#"my $s = "x"; $s .= "y" x §n1§; $s x= 2; my $l = length $s; print "$s $l\n";"#,
    r#"my @w = split ' ', §T1§; print scalar(@w), " ", join("_", map { ucfirst } @w), " ", join("", map { substr($_, 0, 1) } @w), "\n";"#,
    r#"my $s = sprintf("%s", 1e15) . "|" . 1e16 . "|" . 0.1 + 0.2 . "|" . 1/3 . "|" . 1e-5 . "|" . -0.0; print "$s\n";"#,
    r#"print "a" . 1 + 2, " ", "3" + "4" . "5", " ", 2 ** 3 ** 2, " ", -2 ** 2, " ", 7 <=> 3, " ", !!1, " [", !!0, "]\n";"#,
    r#"my $s = "abc"; print ucfirst(join(",", map { $_ x 2 } split //, $s)), " ", lc("ÀB"), " ", sprintf("%s", join "", reverse split //, $s), "\n";"#,
    r#"my $str = "a,b;c d"; my @t = split /[,; ]/, $str; print scalar(@t), " ", join("", map { uc } @t), " ", join("+", sort { $b cmp $a } @t), "\n";"#,
    r#"my $s = "abc"; print substr($s, 1), substr($s, 0, -1), substr($s, -2, 1), " ", defined(substr($s, 5)) ? "d" : "u", "\n";"#,
    r#"my $n = §m1§ * 1234567; 1 while $n =~ s/^(\d+)(\d{3})/$1,$2/; print "$n\n";"#,
    r#"print "ok\n" if "abc" =~ /b/ && "abc" !~ /d/; print "10" == 10.0 ? "eq" : "ne", " ", "abc" x 2.7, " ", "1_000" + 0, "\n";"#,
    // ── heredocs ──
    "my $x = §n1§; my @a = (1, 2); print <<\"EOT\";\nvalue: $x @{[ $x * 2 ]} @a $a[1] \\$x\nEOT\nprint \"after\\n\";",
    "my $x = §n1§; print <<'EOT';\nraw: $x @a \\n \\\\\nEOT\n",
    "my $x = \"§w1§\"; print <<~EOT;\n    indented $x\n      more\n    done\n    EOT\n",
    "my $x = §n1§; print <<A, <<B;\nfirst $x\nA\nsecond $x\nB\nprint \"z\\n\";",
    "my $s = lc(<<EOT) . \"tail\\n\";\nHELLO World\nEOT\nprint $s;",
    "my %h = (text => <<EOT, other => 2);\nbody line\nEOT\nprint $h{text}, $h{other}, \"\\n\";",
    "sub f { return join \"|\", @_ } print f(<<X, \"mid\", <<Y), \"\\n\";\nxx\nX\nyy\nY\n",
    "my $n = 3; print <<EOT;\n${\\ ($n + 1)} and @{[ map { $_ * 2 } 1 .. $n ]}\nEOT\n",
    "print <<\"EOT\" . \"end\\n\";\nline one\n\nline three\nEOT\n",
];

fn run(interp: &[&str], script: &std::path::Path) -> Option<(String, bool)> {
    let (bin, flags) = interp.split_first()?;
    let out = Command::new(bin)
        .args(flags)
        .arg(script)
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    Some((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    ))
}

fn check_seed(seed: u64, snippets: usize) {
    if Command::new("perl").arg("-e").arg("1").output().is_err() {
        eprintln!("skip: perl(1) not available");
        return;
    }
    let st = env!("CARGO_BIN_EXE_st");
    let dir = std::env::temp_dir();
    let mut rng = Rng::new(seed);
    let mut diffs = Vec::new();
    for i in 0..snippets {
        let template = TEMPLATES[rng.below(TEMPLATES.len() as u64) as usize];
        let body = expand(&mut rng, template);
        let program = format!("use strict;\nuse warnings;\nno warnings;\n{body}\n");
        let script = dir.join(format!(
            "stryke_stmt_gen_{}_{seed}_{i}.pl",
            std::process::id()
        ));
        fs::write(&script, &program).expect("write snippet");
        let perl = run(&["perl"], &script).expect("run perl");
        let got = run(&[st, "--compat"], &script).expect("run st");
        let _ = fs::remove_file(&script);
        if perl != got {
            diffs.push(format!(
                "----\n{body}\n  perl (ok={}): {:?}\n  st   (ok={}): {:?}",
                perl.1, perl.0, got.1, got.0
            ));
        }
    }
    assert!(
        diffs.is_empty(),
        "seed {seed}: {} of {snippets} snippets diverge from perl:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

/// Every template, once, independent of the RNG's template choice.
#[test]
fn every_statement_template_matches_perl() {
    if Command::new("perl").arg("-e").arg("1").output().is_err() {
        eprintln!("skip: perl(1) not available");
        return;
    }
    let st = env!("CARGO_BIN_EXE_st");
    let dir = std::env::temp_dir();
    let mut rng = Rng::new(0xC0FFEE);
    let mut diffs = Vec::new();
    for (i, template) in TEMPLATES.iter().enumerate() {
        let body = expand(&mut rng, template);
        let program = format!("use strict;\nuse warnings;\nno warnings;\n{body}\n");
        let script = dir.join(format!("stryke_stmt_all_{}_{i}.pl", std::process::id()));
        fs::write(&script, &program).expect("write snippet");
        let perl = run(&["perl"], &script).expect("run perl");
        let got = run(&[st, "--compat"], &script).expect("run st");
        let _ = fs::remove_file(&script);
        if perl != got {
            diffs.push(format!(
                "----\n{body}\n  perl (ok={}): {:?}\n  st   (ok={}): {:?}",
                perl.1, perl.0, got.1, got.0
            ));
        }
    }
    assert!(
        diffs.is_empty(),
        "{} of {} templates diverge from perl:\n{}",
        diffs.len(),
        TEMPLATES.len(),
        diffs.join("\n")
    );
}

#[test]
fn generated_statements_match_perl_seed_1() {
    check_seed(1, 80);
}

#[test]
fn generated_statements_match_perl_seed_2() {
    check_seed(2, 80);
}

#[test]
fn generated_statements_match_perl_seed_3() {
    check_seed(3, 80);
}
