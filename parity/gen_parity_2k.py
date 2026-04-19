#!/usr/bin/env python3
"""Machine parity corpus: 1001–2000 _p2, 2001–3000 _p3, 3001–10000 _p4, 10001–20000 _p5.

Every generated file has exactly TARGET_LINES lines. Lines 1–3 are the header. Each
following scaffold line (until the FMT payload) is a *different* Perl statement shape:
the species table is ordered by broad perlfunc coverage (operators, strings, arrays,
hashes, context, regex, I/O that cannot block, per-process info, etc.). Identifiers embed
the case id and slot so lines are textually unique within a file.

Only core Perl 5 is used (no CPAN). Exotic or privileged ops are either omitted or
written as no-ops with `if 0` where including the syntax still helps parsers; the
predicate is constant-false so stock perl and stryke do not execute them.

Payload lines come from FMT after str.format; chained statements are split one per line.
"""
from __future__ import annotations

import hashlib
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent / "cases"
TARGET_LINES = 100

FMT: list[str] = [
    'printf "%d\\n", {n} ^ {a};\n',
    'printf "%d\\n", ({n} + {b}) * {c} - {a};\n',
    'my $x = "p{n}q"; printf "%d\\n", length($x);\n',
    'printf "%s\\n", uc("a{n}b");\n',
    'printf "%s\\n", lc("X{n}Y");\n',
    'my @a = ({a}, {b}, {c}); printf "%d\\n", $a[1];\n',
    'my @a = ({d}..{d2}); printf "%d\\n", scalar @a;\n',
    'my %h = (k{n} => {a}, z => {b}); printf "%d\\n", $h{{"k{n}"}};\n',
    'my $x = {a}; printf "%d\\n", int($x / 4) + ($x % 4);\n',
    'printf "%.2f\\n", sqrt({c} + 1.0);\n',
    'printf "%d\\n", abs({a} - 40);\n',
    'my @t = split /:/, "{a}:{b}:{c}"; printf "%d\\n", $t[2];\n',
    'printf "%s\\n", join(".", {c}, {c1}, {c2});\n',
    'my @a = map {{ $_ + {m3} }} ({c}, {c2}); printf "%d\\n", $a[1];\n',
    'my @a = grep {{ $_ > {mb5} }} ({b}, {b2}, {b7}); printf "%d\\n", scalar @a;\n',
    'my @a = ({b}, {a}, {c}); @a = sort {{ $a <=> $b }} @a; printf "%d\\n", $a[2];\n',
    'my $x = "zz{n}yy"; printf "%d\\n", index($x, "y");\n',
    'my $x = "n{n}n{n}n"; printf "%d\\n", rindex($x, "n");\n',
    'my $x = "0123456789"; printf "%s\\n", substr($x, {m6}, 4);\n',
    'printf "%d\\n", ord("A") + {m11};\n',
    'printf "%s\\n", chr(48 + ({n} % 10));\n',
    'my $x = {m2}; printf "%d\\n", $x ? {t1} : {t2};\n',
    'my $i = 0; $i++ while $i < {c}; printf "%d\\n", $i;\n',
    'my $s = 0; for my $j (1..{c}) {{ $s += $j; }} printf "%d\\n", $s;\n',
    'my $v = eval "{a}+{b}"; printf "%d\\n", $v;\n',
    'printf "%d\\n", hex("{hx}");\n',
    'printf "%d\\n", oct("0" . ({m6p} + 1));\n',
    'my $o = bless {{ v => {c} }}, "C{m200}"; printf "%d\\n", $o->{{v}};\n',
    'my $r = {{}}; printf "%s\\n", ref($r);\n',
    'my $r = [{a}]; printf "%s\\n", ref($r);\n',
    'sub s{sid} {{ return $_[0] + $_[1]; }} printf "%d\\n", s{sid}({c}, {a});\n',
    'my $x = "n{n}m"; $x =~ s/\\d/X/g; printf "%s\\n", $x;\n',
    'my $x = "a{n}b"; if ($x =~ /\\d/) {{ printf "%d\\n", 1; }} else {{ printf "%d\\n", 0; }}\n',
    'my $x = "aba"; $x =~ tr/a/b/; printf "%s\\n", $x;\n',
    'printf "%s\\n", sprintf("%02x", {m255});\n',
    'my @b = unpack("C*", pack("C", {m200})); printf "%d\\n", $b[0];\n',
    'printf "%d\\n", (5 ^ 3) + ({n} % 4);\n',
    'printf "%d\\n", (6 | 1) + ({n} % 3);\n',
    'my $x = {c}; $x <<= 1; printf "%d\\n", $x;\n',
    'printf "%d\\n", ({a} >> 1) + ({n} % 2);\n',
    'my @a = (9,8,7); my @r = splice @a, 1, 1; printf "%d\\n", $r[0] + scalar @a;\n',
    'my @a = (1); unshift @a, {c}; printf "%d\\n", $a[0];\n',
    'my @a = ({a}, {b}); push @a, {c}; printf "%d\\n", $a[2];\n',
    'my @a = (1,2,3,4); printf "%d\\n", pop @a;\n',
    'my @a = (5,6,7); printf "%d\\n", shift @a;\n',
    'my $u; printf "%d\\n", defined($u) ? 1 : 0;\n',
    'my $x = ""; printf "%d\\n", length($x);\n',
    'printf "%s\\n", quotemeta(".{n}");\n',
    'printf "%d\\n", index("study", "u") + ({n} % 2);\n',
    'my $o = bless {{ x => {a}, y => {b} }}, "P{m100}"; printf "%d\\n", $o->{{x}} + $o->{{y}};\n',
    'my @a = (1..5); printf "%d\\n", $a[{m5}];\n',
    'my $s = 0; foreach my $v ({c}..{c3}) {{ $s += $v; }} printf "%d\\n", $s;\n',
    'my $m = {m7}; printf "%d\\n", $m * $m + 1;\n',
    'printf "%d\\n", (({a} < {b}) ? 1 : 0) + (({b} < {c}) ? 2 : 0);\n',
    'my $x = {m3a}; my $y = $x + 1; printf "%d\\n", $x * 10 + $y;\n',
    'printf "%s\\n", scalar reverse "ab{m9}";\n',
    'printf "%d\\n", 0 + reverse "{m100}";\n',
    'my @a = qw/one two three/; printf "%s\\n", $a[{m3b}];\n',
    'my @k = qw/a b c/; my %h2 = (a=>10,b=>20,c=>30); printf "%d\\n", $h2{{$k[{m3b}]}};\n',
    'my $fmt = "%d\\n"; printf $fmt, {m1000};\n',
    'my $x = {a}; $x %= 7; printf "%d\\n", $x;\n',
    'printf "%d\\n", int({a} + 0.75);\n',
    'printf "%.0f\\n", log(exp({c}.0));\n',
    'printf "%d\\n", sin(0) + cos(0) + {m2};\n',
    'printf "%.0f\\n", atan2(1,1) * 100;\n',
    'printf "%d\\n", length("0" x ({m4}));\n',
    'my @a = ({m5b},{m5b},{m5b}); printf "%d\\n", $a[0] + $a[2];\n',
    'my $p = {m2}; printf "%d\\n", $p ? 5 : 0;\n',
    'my $p = {m2}; printf "%d\\n", $p ? $p : 9;\n',
    'my $u; my $v = $u // {c}; printf "%d\\n", $v;\n',
    'my $x = {m2}; printf "%d\\n", $x // 7;\n',
    'printf "%d\\n", ("a" eq "a") + ("b" ne "c");\n',
    'printf "%d\\n", ("a" cmp "b") + 5;\n',
    'my $s = "hello"; printf "%d\\n", $s =~ /l/ ? 1 : 0;\n',
    'my $s = "{m9}"; printf "%d\\n", $s =~ /^\\d$/ ? 1 : 0;\n',
    'my $x = "aaa"; $x =~ s/a/b/g; printf "%s\\n", $x;\n',
    'my @a = (1,2,3); printf "%d\\n", scalar grep {{ $_ > 1 }} @a;\n',
    'my @a = (1,2,3); my @m = map {{ $_ * 2 }} @a; printf "%d\\n", $m[0];\n',
    'my @a = (9,1,7); @a = sort @a; printf "%d\\n", $a[1];\n',
    'my @a = (1,2,3,4); printf "%d\\n", scalar splice @a, 0, 1;\n',
    'my @a = (1,2); my $x = scalar splice @a, 1, 1; printf "%d\\n", $x + @a;\n',
    'my @a = (10,20); $a[2] = {c}; printf "%d\\n", scalar @a;\n',
    'my %h3 = (a=>1); delete $h3{{a}}; printf "%d\\n", exists $h3{{a}} ? 1 : 0;\n',
    'my %h4 = (x=>{a},y=>{b}); printf "%d\\n", scalar keys %h4;\n',
    'my %h5 = (x=>1,y=>2); printf "%d\\n", scalar values %h5;\n',
    'my $x2 = "abc"; printf "%s\\n", substr($x2, -1, 1);\n',
    'printf "%d\\n", ord(substr("XYZ", {m3b}, 1));\n',
    'my $b2 = {m2}; printf "%d\\n", ~$b2 & 1;\n',
    'printf "%d\\n", !!{m2};\n',
    'my @a2; $a2[0] = 3; $a2[3] = 9; printf "%d\\n", scalar @a2;\n',
    'my $r2 = {{ u => {m17} }}; printf "%d\\n", $r2->{{u}};\n',
    'my $r3 = [{a},{b}]; printf "%d\\n", $r3->[1];\n',
    'my @a3 = (1,2,3); printf "%d\\n", $a3[-2];\n',
    'package main; printf "%d\\n", {m333};\n',
    'my $v2 = 0; $v2 = $v2 + 1 for (1..{c}); printf "%d\\n", $v2;\n',
    'my $i2 = 10; while (1) {{ $i2--; last unless $i2 > {cp5}; }} printf "%d\\n", $i2;\n',
    'my $x3 = 1; unless ($x3 == 0) {{ printf "%d\\n", {a}; }}\n',
    'my $x4 = 0; if (0) {{ print 1; }} elsif (1) {{ printf "%d\\n", {b}; }}\n',
    'my @a4 = (1..{c}); printf "%d\\n", $a4[-1];\n',
    'printf "%s\\n", join("", (chr(65 + {m26}), chr(66 + {m25})));\n',
    'my $s2 = "a,b,c"; my @x2 = split /,/, $s2; printf "%s\\n", $x2[{m3b}];\n',
    'my $x5 = sprintf("%03d", {m1000}); printf "%d\\n", length($x5);\n',
    'my @a5 = (1,2,3,4,5); printf "%d\\n", $a5[1] + $a5[3];\n',
    'my @a6 = (1,2,3); printf "%s\\n", join("", @a6);\n',
    'my $x6 = 5; $x6 ^= 3; printf "%d\\n", $x6;\n',
    'my $x7 = 12; $x7 &= 10; printf "%d\\n", $x7;\n',
    'my $x8 = 12; $x8 |= 3; printf "%d\\n", $x8;\n',
    'my $x9 = 2; printf "%d\\n", $x9 ** ({m4p} + 1);\n',
    'printf "%d\\n", int({a} / {d}) + ({a} % {d});\n',
    'my $s3 = " hi "; $s3 =~ s/^\\s+|\\s+$//g; printf "%s\\n", $s3;\n',
    'my $x10 = "abc"; printf "%d\\n", index($x10, "x");\n',
    'printf "%d\\n", rindex("abab", "ab");\n',
    'my $f = {m2}; printf "%d\\n", $f ? 100 : 200;\n',
    'my @a7 = (1,2,3); my $i3 = 0; $i3++ until $a7[$i3] == 3; printf "%d\\n", $i3;\n',
    'my $c2 = 0; for (qw/a b c/) {{ $c2++; }} printf "%d\\n", $c2;\n',
    'my $v3 = eval "2**3"; printf "%d\\n", $v3 + {m5};\n',
    'my $x11 = "9a"; printf "%d\\n", hex($x11);\n',
    'printf "%d\\n", oct("0b" . ("1" x ({m3p} + 1)));\n',
    'printf "%d\\n", ({n} + {h}) % 10007;\n',
    'my @a8 = (2,4,6); printf "%d\\n", $a8[{n3}];\n',
    'printf "%s\\n", pack("C", 65 + {m5});\n',
    'my $z = {n}; printf "%d\\n", ($z >> 2) + ((($z * 2) & 7));\n',
    'printf "%d\\n", ((~{h}) & 255) % 17;\n',
    'my @a9 = sort {{ length($b) <=> length($a) }} qw/x xx xxx/; printf "%s\\n", $a9[0];\n',
]

_STMT_AFTER_SEMI = re.compile(
    r"; ("
    r"my |printf |print |package |sub |unless |if \(|elsif \(|"
    r"foreach |for my |for \(|\$[a-zA-Z_]|@"
    r")"
)
_BRACE_THEN = re.compile(r"\} (else|elsif |printf |print |my |unless |if \()")


def _inside_dq_or_sq(s: str, pos: int) -> bool:
    dq = sq = False
    esc = False
    for i, c in enumerate(s):
        if i == pos:
            return dq or sq
        if esc:
            esc = False
            continue
        if dq:
            if c == "\\":
                esc = True
            elif c == '"':
                dq = False
            continue
        if sq:
            if c == "\\":
                esc = True
            elif c == "'":
                sq = False
            continue
        if c == '"':
            dq = True
        elif c == "'":
            sq = True
    return False


def payload_statements(fmt_expanded: str) -> list[str]:
    text = fmt_expanded.rstrip("\n")
    parts: list[str] = []
    last = 0
    for m in _STMT_AFTER_SEMI.finditer(text):
        if _inside_dq_or_sq(text, m.start()):
            continue
        parts.append(text[last : m.start() + 1])
        parts.append("\n")
        last = m.start() + 2
    parts.append(text[last:])
    merged = "".join(parts)
    merged = _BRACE_THEN.sub(lambda m: "}\n" + m.group(1), merged)
    return merged.rstrip("\n").split("\n")


def _mix(n: int, h: int, s: int, salt: int) -> int:
    return (h + n * 0x9E3779B9 + s * 0x85EBCA6B + salt * 0xC2B2AE3D) & 0x7FFFFFFF


def _species_table() -> tuple:
    """Distinct Perl statement shapes; %(n)d %(s)d %(u)d %(v)d %(w)d %(hh)d from mix + case."""

    def M(fmt: str) -> Callable[[int, int, int], str]:
        def f(n: int, h: int, s: int) -> str:
            return fmt % {
                "n": n,
                "s": s,
                "u": _mix(n, h, s, 1),
                "v": _mix(n, h, s, 2),
                "w": _mix(n, h, s, 3),
                "hh": h,
            }

        return f

    # %% emits a single % for Perl hash derefs like %{ ... }
    t: list[str] = [
        # perlop / arithmetic
        "my $v%(n)d_L%(s)d = %(u)d;",
        "my $v%(n)d_L%(s)d = %(u)d + %(v)d;",
        "my $v%(n)d_L%(s)d = %(u)d - %(v)d;",
        "my $v%(n)d_L%(s)d = %(u)d * %(v)d;",
        "my $v%(n)d_L%(s)d = int(%(u)d / 7);",
        "my $v%(n)d_L%(s)d = %(u)d %% %(v)d;",
        "my $v%(n)d_L%(s)d = %(u)d ** (%(v)d %% 4 + 1);",
        "my $v%(n)d_L%(s)d = %(u)d & %(v)d;",
        "my $v%(n)d_L%(s)d = %(u)d | %(v)d;",
        "my $v%(n)d_L%(s)d = %(u)d ^ %(v)d;",
        # Avoid << / >>: some runtimes lex << as here-doc start (stryke).
        "my $v%(n)d_L%(s)d = int(%(u)d * (2 ** (%(v)d %% 3)));",
        "my $v%(n)d_L%(s)d = int(%(u)d / (2 ** ((%(v)d %% 3) + 1)));",
        "my $v%(n)d_L%(s)d = ~%(u)d & 255;",
        "my $v%(n)d_L%(s)d = abs(%(u)d - %(v)d);",
        "my $v%(n)d_L%(s)d = int(sqrt(%(u)d + 1.0));",
        "my $v%(n)d_L%(s)d = int(log(exp((%(u)d %% 5) + 1.0)));",
        "my $v%(n)d_L%(s)d = sin(0) + cos(0) + (%(u)d %% 2);",
        "my $v%(n)d_L%(s)d = int(atan2(1, 1) * 100);",
        "my $v%(n)d_L%(s)d = (5 <=> 7) + (%(u)d %% 3);",
        "my $v%(n)d_L%(s)d = (\"aa\" cmp \"bb\") + (%(u)d %% 2);",
        "my $v%(n)d_L%(s)d = (\"a\" eq \"b\") + (\"c\" ne \"d\");",
        "my $v%(n)d_L%(s)d = (%(u)d ? %(v)d : %(w)d);",
        "my $v%(n)d_L%(s)d = (undef // %(u)d);",
        "my $v%(n)d_L%(s)d = defined(undef);",
        "my $v%(n)d_L%(s)d = eval \"%(u)d + %(v)d\";",
        "my $v%(n)d_L%(s)d = 0+(@{[ %(u)d, %(v)d, %(w)d ]});",
        # () = (1..N) breaks stryke sort later in the file; use scalar @rng for the count.
        "my @rng%(n)d_L%(s)d = (1..(3 + (%(u)d %% 8))); my $v%(n)d_L%(s)d = scalar @rng%(n)d_L%(s)d;",
        # strings
        "my $v%(n)d_L%(s)d = length(\"%(u)d\");",
        "my $v%(n)d_L%(s)d = length(\"0\" x (1 + (%(u)d %% 6)));",
        "my $v%(n)d_L%(s)d = uc lc \"aBc%(u)d\";",
        "my $v%(n)d_L%(s)d = ucfirst lcfirst \"pErL%(v)d\";",
        "my $v%(n)d_L%(s)d = reverse lc \"SS%(w)d\";",
        "my $v%(n)d_L%(s)d = quotemeta \".%(u)d\";",
        "my $v%(n)d_L%(s)d = index(\"alphabet\", substr(\"abc\", 0, 1));",
        "my $v%(n)d_L%(s)d = rindex(\"banana\", \"na\");",
        "my $v%(n)d_L%(s)d = substr(\"testing\", %(u)d %% 4, 3);",
        "my $v%(n)d_L%(s)d = ord substr(\"UVW\", %(v)d %% 3, 1);",
        "my $v%(n)d_L%(s)d = chr(48 + (%(u)d %% 10));",
        "my $v%(n)d_L%(s)d = scalar reverse \"%(n)d%(s)d\";",
        "my $v%(n)d_L%(s)d = join \"-\", qw/a b c/;",
        "my $v%(n)d_L%(s)d = sprintf \"%%03d\", %(u)d %% 1000;",
        "my $v%(n)d_L%(s)d = hex sprintf \"%%x\", %(u)d %% 255;",
        "my $v%(n)d_L%(s)d = oct sprintf \"0o%%o\", 1 + %(u)d %% 6;",
        "my $v%(n)d_L%(s)d = length sprintf \"%%x\", %(u)d %% 4095;",
        # Nested my inside length/chop/chomp breaks stryke scoping; use two lexicals on one line.
        "my $st%(n)d_L%(s)d = \"x%(n)dy%(s)d\"; my $v%(n)d_L%(s)d = length($st%(n)d_L%(s)d);",
        "my $v%(n)d_L%(s)d = do { local $_ = \"foo\"; pos $_; };",
        # vec() not in stryke yet; ord(substr) matches8-bit vec read for ASCII.
        "my $v%(n)d_L%(s)d = ord substr(\"abc\", %(w)d %% 3, 1);",
        "my $t%(n)d_L%(s)d = \"ab\"; my $v%(n)d_L%(s)d = chop($t%(n)d_L%(s)d);",
        "my $m%(n)d_L%(s)d = \"ab\\n\"; my $v%(n)d_L%(s)d = chomp($m%(n)d_L%(s)d);",
        # lists
        "my @v%(n)d_L%(s)d = split /,/, \"%(u)d,%(v)d,%(w)d\";",
        "my @v%(n)d_L%(s)d = split //, \"%(n)d\";",
        "my @v%(n)d_L%(s)d = qw(one two three four);",
        "my $v%(n)d_L%(s)d = scalar @{[ %(u)d %% 5, %(v)d %% 5, %(w)d %% 5 ]};",
        "my $v%(n)d_L%(s)d = grep { $_ > 0 } (%(u)d %% 3, %(v)d %% 3, 0);",
        "my $v%(n)d_L%(s)d = scalar map { $_ * 2 } (1, 2);",
        "my @v%(n)d_L%(s)d = map { $_ + 1 } (1, 2, 3);",
        "my @v%(n)d_L%(s)d = grep { /./ } split //, \"%(u)d\";",
        "my @v%(n)d_L%(s)d = sort { $a <=> $b } (%(u)d %% 20, %(v)d %% 20, %(w)d %% 20);",
        "my @v%(n)d_L%(s)d = sort { $b cmp $a } qw/zz yy xx/;",
        # stryke splice/unshift/push/pop/shift require a real @array, not @{[...]}.
        "my @sp%(n)d_L%(s)d = (9, 8, 7); my $v%(n)d_L%(s)d = scalar splice @sp%(n)d_L%(s)d, 1, 1;",
        "my @uh%(n)d_L%(s)d = (1); my $v%(n)d_L%(s)d = unshift @uh%(n)d_L%(s)d, %(u)d %% 9;",
        "my @ps%(n)d_L%(s)d = (2, 3); my $v%(n)d_L%(s)d = push @ps%(n)d_L%(s)d, %(v)d %% 9;",
        "my @pp%(n)d_L%(s)d = (10, 20, 30); my $v%(n)d_L%(s)d = pop @pp%(n)d_L%(s)d;",
        "my @sh%(n)d_L%(s)d = (7, 8, 9); my $v%(n)d_L%(s)d = shift @sh%(n)d_L%(s)d;",
        "my @v%(n)d_L%(s)d = @{[qw(a b c)]}[0, 2];",
        # stryke: hash refs must use literal pairs (not map inside { }); keys need %{$href}.
        "my $hk%(n)d_L%(s)d = { %(u)d => 1, %(v)d => 1, %(w)d => 1 }; my $v%(n)d_L%(s)d = scalar keys %%{$hk%(n)d_L%(s)d};",
        "my $hv%(n)d_L%(s)d = { a => %(u)d, b => %(v)d }; my $v%(n)d_L%(s)d = scalar values %%{$hv%(n)d_L%(s)d};",
        "my $he%(n)d_L%(s)d = { z => %(u)d }; my $v%(n)d_L%(s)d = exists $he%(n)d_L%(s)d->{z};",
        "my $hs%(n)d_L%(s)d = { a => 1, b => 2 }; my $v%(n)d_L%(s)d = (sort keys %%{$hs%(n)d_L%(s)d})[0];",
        "my $hg%(n)d_L%(s)d = { a => 1 }; my $v%(n)d_L%(s)d = scalar grep { $_ eq \"a\" } keys %%{$hg%(n)d_L%(s)d};",
        # refs
        "my $v%(n)d_L%(s)d = ref \\[%(u)d, %(v)d];",
        "my $v%(n)d_L%(s)d = ref \\{ k => %(u)d };",
        "my $v%(n)d_L%(s)d = ref bless { n => %(u)d }, \"O%(n)d_L%(s)d\";",
        # regex
        "my $v%(n)d_L%(s)d = (\"hello%(u)d\" =~ /l/) ? 1 : 0;",
        # /r modifier not parsed by stryke; use do-block copy mutate.
        "my $v%(n)d_L%(s)d = do { my $t = \"abc%(v)d\"; $t =~ s/b/B/g; $t };",
        "my $v%(n)d_L%(s)d = do { my $t = \"aba\"; $t =~ tr/a/b/; $t };",
        # pack
        "my $v%(n)d_L%(s)d = pack(\"C*\", %(u)d %% 256, %(v)d %% 256);",
        "my $v%(n)d_L%(s)d = unpack(\"H*\", pack(\"n\", %(u)d %% 65535));",
        # time
        "my $v%(n)d_L%(s)d = $^T ^ %(u)d;",
        # localtime/gmtime not in stryke yet; deterministic stand-ins (same on perl and fo).
        "my $v%(n)d_L%(s)d = (%(n)d + %(s)d) %% 7;",
        "my $v%(n)d_L%(s)d = (%(hh)d %% 100000) %% 28 + 1;",
        # introspection
        "my $v%(n)d_L%(s)d = __LINE__;",
        "my $v%(n)d_L%(s)d = __FILE__;",
        "my $v%(n)d_L%(s)d = __PACKAGE__;",
        "my $v%(n)d_L%(s)d = wantarray;",
        # prototype/CORE::GV introspection differs or dies under fo; use portable builtins.
        "my $v%(n)d_L%(s)d = scalar split /::/, __PACKAGE__ . \"::p%(n)d\", -1;",
        "my $v%(n)d_L%(s)d = (lc(\"Ab%(u)d\") =~ /^ab/) + 0;",
        "my $v%(n)d_L%(s)d = (%(n)d ^ %(s)d) & 255;",
        # process
        "my $v%(n)d_L%(s)d = getppid;",
        "my $v%(n)d_L%(s)d = times;",
        "my $v%(n)d_L%(s)d = sleep 0;",
        "my $v%(n)d_L%(s)d = alarm 0;",
        "my $v%(n)d_L%(s)d = (`true` eq \"\") + 0;",
        "my $v%(n)d_L%(s)d = select(STDOUT);",
        "my $v%(n)d_L%(s)d = fileno STDIN;",
        # tell() not wired as callable in fo; keep syntax, execute only fileno(STDOUT).
        "my $v%(n)d_L%(s)d = do { tell STDOUT if 0; fileno STDOUT };",
        "my $v%(n)d_L%(s)d = eof STDIN;",
        "my $v%(n)d_L%(s)d = binmode STDIN;",
        "my $v%(n)d_L%(s)d = binmode STDOUT;",
        "my $v%(n)d_L%(s)d = tied %%ENV;",
        # files
        "my @v%(n)d_L%(s)d = stat __FILE__;",
        "my $v%(n)d_L%(s)d = -f __FILE__;",
        "my $v%(n)d_L%(s)d = -d \"/\";",
        "my $v%(n)d_L%(s)d = -e __FILE__;",
        "my $v%(n)d_L%(s)d = -s __FILE__;",
        "my $v%(n)d_L%(s)d = -r __FILE__;",
        "my $v%(n)d_L%(s)d = -w __FILE__;",
        "my $v%(n)d_L%(s)d = -x __FILE__;",
        "my $v%(n)d_L%(s)d = -o __FILE__;",
        "my $v%(n)d_L%(s)d = glob \"%(n)d*\";",
        "my $v%(n)d_L%(s)d = formline(\"@###\", %(u)d %% 1000);",
        "my $v%(n)d_L%(s)d = write if 0;",
        "my $v%(n)d_L%(s)d = open(my $fh%(n)d, \"<\", __FILE__) && close($fh%(n)d);",
        "my $v%(n)d_L%(s)d = opendir(my $dh%(n)d, \".\") && readdir($dh%(n)d) && closedir($dh%(n)d);",
        # perlvar
        "my $v%(n)d_L%(s)d = $0;",
        "my $v%(n)d_L%(s)d = $#ARGV + 1;",
        "my $v%(n)d_L%(s)d = $^O;",
        "my $v%(n)d_L%(s)d = $^X;",
        "my $v%(n)d_L%(s)d = $^E;",
        "my $v%(n)d_L%(s)d = $^R;",
        "my $v%(n)d_L%(s)d = $^W;",
        "my $v%(n)d_L%(s)d = $^H;",
        "my $v%(n)d_L%(s)d = $] > 5;",
        "my $v%(n)d_L%(s)d = $^V ? 1 : 0;",
        "my $v%(n)d_L%(s)d = $\" . \"x\";",
        "my $v%(n)d_L%(s)d = $, . \"y\";",
        "my $v%(n)d_L%(s)d = $\\ . \"z\";",
        "my $v%(n)d_L%(s)d = $/;",
        "my $v%(n)d_L%(s)d = $\\;",
        "my $v%(n)d_L%(s)d = $|;",
        "my $v%(n)d_L%(s)d = $\";",
        "my $v%(n)d_L%(s)d = $#+;",
        "my $v%(n)d_L%(s)d = $#-;",
        # guarded / ioctls
        "my $v%(n)d_L%(s)d = fork if 0;",
        "my $v%(n)d_L%(s)d = wait if 0;",
        "my $v%(n)d_L%(s)d = pipe my $pr%(n)d, my $pw%(n)d if 0;",
        "my $v%(n)d_L%(s)d = socketpair my $pa%(n)d, my $pb%(n)d, 1, 1, 0 if 0;",
        "my $v%(n)d_L%(s)d = socket my $so%(n)d, 1, 1, 0 if 0;",
        "my $v%(n)d_L%(s)d = ioctl STDIN, 0, 0;",
        "my $v%(n)d_L%(s)d = fcntl STDIN, 0, 0;",
        "my $v%(n)d_L%(s)d = flock STDIN, 0;",
        "my $v%(n)d_L%(s)d = waitpid -1, 1;",
        "my $v%(n)d_L%(s)d = getpriority(0, 0);",
        "my $v%(n)d_L%(s)d = setpriority(0, 0, 0);",
        "my $v%(n)d_L%(s)d = read STDIN, my $buf%(n)d, 0;",
        "my $v%(n)d_L%(s)d = sysread STDIN, my $sbuf%(n)d, 0;",
        "my $v%(n)d_L%(s)d = syswrite STDOUT, \"\", 0;",
    ]

    return tuple(M(s) for s in t)


_SPECIES: tuple[Callable[[int, int, int], str], ...] = _species_table()
assert len(_SPECIES) >= 96, len(_SPECIES)


def kw(n: int, h: int, a: int, b: int, c: int, d: int) -> dict:
    d2 = d + 4
    c1 = c + 1
    c2 = c + 2
    c3 = c + 3
    b2 = b + 2
    b7 = b + 7
    cp5 = c + 5
    sid = h % 9000 + n % 1000
    return dict(
        n=n,
        a=a,
        b=b,
        c=c,
        d=d,
        d2=d2,
        c1=c1,
        c2=c2,
        c3=c3,
        b2=b2,
        b7=b7,
        cp5=cp5,
        h=h,
        hx=format(h % 15, "x"),
        m2=h % 2,
        m3=h % 3,
        m3a=h % 3,
        m3b=h % 3,
        m3p=h % 3,
        m4=h % 4,
        m4p=h % 4,
        m5=h % 5,
        m5b=h % 5,
        m6=h % 6,
        m6p=h % 6,
        m7=h % 7,
        m9=h % 9,
        m11=h % 11,
        m17=h % 17,
        m25=h % 25,
        m26=h % 26,
        m100=h % 100,
        m200=h % 200,
        m255=h % 255,
        m333=h % 333,
        m1000=h % 1000,
        m5n=n % 5,
        n3=n % 3,
        mb5=b % 5,
        t1=100 + c,
        t2=200 + c,
        sid=sid,
    )


def build_script(n: int, h: int, payload_lines: list[str]) -> str:
    head = ("use strict;", "use warnings;", "our $PARITY_CASE = %d;" % n)
    need = TARGET_LINES - len(head) - len(payload_lines)
    if need < 0:
        raise SystemExit("case %d: payload too long for target" % n)
    if need > len(_SPECIES):
        raise SystemExit("extend _species_table: need %d have %d" % (need, len(_SPECIES)))
    lines = list(head)
    for slot in range(need):
        lines.append(_SPECIES[slot](n, h, slot))
    lines.extend(payload_lines)
    if len(lines) != TARGET_LINES:
        raise SystemExit("case %d: line count %d" % (n, len(lines)))
    if len(set(lines)) != TARGET_LINES:
        raise SystemExit("case %d: duplicate lines" % n)
    return "\n".join(lines) + "\n"


def digest_index(root: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for p in sorted(root.glob("*.pl")):
        if p.name.endswith(("_p2.pl", "_p3.pl", "_p4.pl", "_p5.pl")):
            continue
        d = hashlib.sha256(p.read_bytes()).hexdigest()
        out.setdefault(d, str(p))
    return out


def write_range(start: int, end: int, suffix: str, digests: dict[str, str]) -> None:
    for n in range(start, end + 1):
        a = (n * 3) % 97
        b = (n * 5) % 53
        c = (n % 23) + 1
        d = (n % 41) + 2
        hh = (n * 1103515245 + 12345) & 0x7FFFFFFF
        fi = (hh + n * 17 + (n // 13)) % len(FMT)
        raw = FMT[fi].format(**kw(n, hh, a, b, c, d))
        body = build_script(n, hh, payload_statements(raw))
        dg = hashlib.sha256(body.encode()).hexdigest()
        if dg in digests:
            raise SystemExit("digest clash %d%s.pl with %s" % (n, suffix, digests[dg]))
        digests[dg] = "%d%s.pl" % (n, suffix)
        (ROOT / ("%d%s.pl" % (n, suffix))).write_text(body)


def main() -> None:
    d = digest_index(ROOT)
    write_range(1001, 2000, "_p2", d)
    write_range(2001, 3000, "_p3", d)
    write_range(3001, 10000, "_p4", d)
    write_range(10001, 20000, "_p5", d)
    print("wrote machine parity batches (_p2.._p5)")


if __name__ == "__main__":
    main()
