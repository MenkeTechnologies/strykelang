#!/usr/bin/env perl
# Observable cross-context parity probe: stock perl(1) vs `st --compat`.
#
# Why this exists (and why it is not gen_parity_2k.py):
#
#   gen_parity_2k.py documents its own blind spot at its top: "Only the single
#   printf payload is observable — the ~96 species lines assign to `my $v...`
#   and are never printed, so they exercise the parser and the op but pin no
#   value. A species line can only fail parity by dying or warning."
#
# So the 1000-file generated range pins one value per file. This probe takes the
# same perlfunc ground and makes every construct OBSERVABLE — the bound value is
# rendered and printed — then multiplies it by an execution CONTEXT matrix, so
# the same construct is checked at top level, inside a named sub, inside a
# closure, inside a loop body, and in tail (implicit-return) position.
#
# Scoring rules this harness enforces, which a plain `cmp` harness does not:
#
#   * The oracle must have SUCCEEDED. A probe where perl itself exits non-zero,
#     times out, or emits nothing is reported SKIPPED — never PASSED. Two
#     agreeing failures must not read as a pass.
#   * The oracle must be DETERMINISTIC. Every probe is run under perl twice; if
#     the two runs disagree the probe is SKIPPED as nondeterministic rather than
#     scored against a moving target.
#   * Every execution path is scored separately. `st --compat` (JIT on) and
#     `st --compat --no-jit` (interpreter) are compared against the oracle
#     independently, so a construct that is correct interpreted and wrong under
#     the JIT is reported as its own divergence class rather than averaged away.
#   * Everything is under a timeout. A hang is a TIMEOUT, counted and reported,
#     never silently scored as agreement.
#
# Usage:  perl parity/probe_observable.pl [options]
#   --st PATH        path to the st binary (default: target/debug/st)
#   --perl PATH      oracle perl (default: perl)
#   --timeout SECS   per-run timeout (default: 10)
#   --filter REGEX   only run probes whose id matches
#   --contexts LIST  comma-separated subset of: top,sub,closure,loop,tail,hot
#   --fail-log PATH  write divergence detail here (default: parity/probe_failures.log)
#   --json PATH      write a JSON summary here
#   --keep-dir DIR   keep generated probe scripts in DIR instead of a temp dir
#   --quiet          suppress per-divergence stderr lines
#
# Exit status is 1 when any divergence was scored, 0 otherwise. SKIPPED probes
# do not fail the run, but they are always reported — a skip is a hole in
# coverage, not a pass.

use strict;
use warnings;
use File::Temp qw(tempdir);
use File::Path qw(make_path);
use Getopt::Long qw(GetOptions);
use POSIX qw(WNOHANG);

my $ST       = 'target/debug/st';
my $PERL     = 'perl';
my $TIMEOUT  = 10;
my $FILTER   = '';
my $CONTEXTS = 'top,sub,closure,loop,tail,hot';

# Invocations the `hot` context performs, chosen to clear stryke's subroutine
# JIT threshold (`STRYKE_JIT_SUB_INVOKES`, default 50) with room to spare.
my $HOT_ITERS = 400;
my $FAIL_LOG = 'parity/probe_failures.log';
my $JSON_OUT = '';
my $KEEP_DIR = '';
my $QUIET    = 0;

GetOptions(
    'st=s'       => \$ST,
    'perl=s'     => \$PERL,
    'timeout=i'  => \$TIMEOUT,
    'filter=s'   => \$FILTER,
    'contexts=s' => \$CONTEXTS,
    'fail-log=s' => \$FAIL_LOG,
    'json=s'     => \$JSON_OUT,
    'keep-dir=s' => \$KEEP_DIR,
    'quiet'      => \$QUIET,
) or die "probe: bad options\n";

die "probe: no executable st at '$ST'\n" unless -x $ST;

# ── Probe corpus ─────────────────────────────────────────────────────────────
#
# Each entry is [ id, kind, statement ]. The statement must bind $V (kind 's')
# or @V (kind 'a'). Rendering is uniform so the printed form pins the value,
# its definedness, and — for lists — the element count.
#
# Only deterministic constructs belong here: nothing that reads the clock, the
# pid, the process table, the filesystem layout, or hash order without a sort.
# Anything that slips through is caught by the double-oracle determinism gate.

my @PROBES = (
    # ── perlop / arithmetic ──────────────────────────────────────────────────
    [ 'arith_add',        's', 'my $V = 17 + 25;' ],
    [ 'arith_sub',        's', 'my $V = 17 - 25;' ],
    [ 'arith_mul',        's', 'my $V = 17 * 25;' ],
    [ 'arith_intdiv',     's', 'my $V = int(97 / 7);' ],
    [ 'arith_mod',        's', 'my $V = 97 % 7;' ],
    [ 'arith_mod_neg',    's', 'my $V = -97 % 7;' ],
    [ 'arith_mod_negrhs', 's', 'my $V = 97 % -7;' ],
    [ 'arith_pow',        's', 'my $V = 3 ** 4;' ],
    [ 'arith_pow_frac',   's', 'my $V = 2 ** 0.5;' ],
    [ 'arith_pow_rassoc', 's', 'my $V = 2 ** 3 ** 2;' ],
    [ 'arith_unary_pow',  's', 'my $V = -2 ** 2;' ],
    [ 'bit_and',          's', 'my $V = 12345 & 6789;' ],
    [ 'bit_or',           's', 'my $V = 12345 | 6789;' ],
    [ 'bit_xor',          's', 'my $V = 12345 ^ 6789;' ],
    [ 'bit_not',          's', 'my $V = ~255 & 65535;' ],
    [ 'bit_shl',          's', 'my $V = 3 << 5;' ],
    [ 'bit_shr',          's', 'my $V = 1024 >> 3;' ],
    [ 'bit_str_and',      's', 'my $V = "abc" & "ABC";' ],
    [ 'bit_str_or',       's', 'my $V = "abc" | "   ";' ],
    [ 'num_abs',          's', 'my $V = abs(-42.5);' ],
    [ 'num_int_trunc',    's', 'my $V = int(-7.9);' ],
    [ 'num_sqrt',         's', 'my $V = sqrt(2);' ],
    [ 'num_exp_log',      's', 'my $V = log(exp(3));' ],
    [ 'num_atan2',        's', 'my $V = atan2(1, 1);' ],
    [ 'num_sin_cos',      's', 'my $V = sin(0) + cos(0);' ],
    [ 'num_hex',          's', 'my $V = hex("ff");' ],
    [ 'num_hex_0x',       's', 'my $V = hex("0x1F");' ],
    [ 'num_oct',          's', 'my $V = oct("755");' ],
    [ 'num_oct_0x',       's', 'my $V = oct("0x20");' ],
    [ 'num_oct_0b',       's', 'my $V = oct("0b1011");' ],
    [ 'cmp_spaceship',    's', 'my $V = (5 <=> 7) . (7 <=> 5) . (5 <=> 5);' ],
    [ 'cmp_strcmp',       's', 'my $V = ("aa" cmp "bb") . ("bb" cmp "aa");' ],
    [ 'cmp_chain_eq',     's', 'my $V = ("a" eq "b") . "|" . ("c" ne "d");' ],

    # ── Perl's false is PL_sv_no ─────────────────────────────────────────────
    # The empty string in string context, 0 in numeric context. Rendering the
    # value alone would let `""` and `"0"` be told apart, but pinning length()
    # as well states the difference numerically, so a value that merely prints
    # blank (undef, say) cannot be mistaken for a correct false.
    [ 'false_numeq',      's', 'my $V = (1 == 2) . ":" . length(1 == 2);' ],
    [ 'false_numne',      's', 'my $V = (1 != 1) . ":" . length(1 != 1);' ],
    [ 'false_numlt',      's', 'my $V = (2 < 1) . ":" . length(2 < 1);' ],
    [ 'false_numgt',      's', 'my $V = (1 > 2) . ":" . length(1 > 2);' ],
    [ 'false_numle',      's', 'my $V = (2 <= 1) . ":" . length(2 <= 1);' ],
    [ 'false_numge',      's', 'my $V = (1 >= 2) . ":" . length(1 >= 2);' ],
    [ 'false_streq',      's', 'my $V = ("a" eq "b") . ":" . length("a" eq "b");' ],
    [ 'false_strne',      's', 'my $V = ("a" ne "a") . ":" . length("a" ne "a");' ],
    [ 'false_strlt',      's', 'my $V = ("b" lt "a") . ":" . length("b" lt "a");' ],
    [ 'false_strgt',      's', 'my $V = ("a" gt "b") . ":" . length("a" gt "b");' ],
    [ 'false_strle',      's', 'my $V = ("b" le "a") . ":" . length("b" le "a");' ],
    [ 'false_strge',      's', 'my $V = ("a" ge "b") . ":" . length("a" ge "b");' ],
    [ 'false_lognot',     's', 'my $V = (!1) . ":" . length(!1);' ],
    [ 'false_defined',    's', 'my $V = defined(undef) . ":" . length(defined undef);' ],
    [ 'false_exists',     's', 'my $V = do { my %h = (a => 1); exists($h{b}) . ":" . length(exists $h{b}) };' ],
    [ 'false_match',      's', 'my $V = do { my $m = ("abc" =~ /zzz/); $m . ":" . length($m) };' ],
    [ 'false_notmatch',   's', 'my $V = do { my $m = ("abc" !~ /a/); $m . ":" . length($m) };' ],
    [ 'false_isa',        's', 'my $V = do { my $o = bless {}, "P::C"; $o->isa("Nope") . ":" . length($o->isa("Nope")) };' ],
    # A failed match in LIST context is the empty list, not a one-element list
    # holding the false value. Rendering `scalar(@V)` is the only way to see
    # the difference: both forms are false in boolean context, but a
    # one-element list makes `my @m = /…/` guards run their true branch and
    # passes an argument to a call where perl passes none. `/g` and non-`/g`
    # are separate probes because they took separate code paths and only the
    # `/g` one was wrong.
    [ 'false_match_list', 'a', 'my @V = ("abc" =~ /zzz/);' ],
    [ 'false_match_glist','a', 'my @V = ("abc" =~ /zzz/g);' ],
    [ 'false_match_gcap', 'a', 'my @V = ("abc" =~ /(z)(z)/g);' ],
    [ 'false_match_gscal','s', 'my $V = do { my $m = ("abc" =~ /zzz/g); $m . ":" . length($m) };' ],
    [ 'true_is_one',      's', 'my $V = (1 == 1) . ":" . length(1 == 1);' ],
    [ 'false_numeric_use','s', 'my $V = (1 == 2) + 5;' ],
    [ 'false_sum_preds',  's', 'my $V = (1 == 2) + (2 == 2) + (3 == 3);' ],
    [ 'false_join',       's', 'my $V = join(",", (1 == 2), (2 == 2), (3 == 4));' ],
    # `<=>` and `cmp` are not booleans — their 0 means "equal" and must stay 0.
    [ 'eq_spaceship_zero','s', 'my $V = (3 <=> 3) . ":" . length(3 <=> 3);' ],
    [ 'eq_strcmp_zero',   's', 'my $V = ("a" cmp "a") . ":" . length("a" cmp "a");' ],
    [ 'ternary',          's', 'my $V = (3 > 2 ? "yes" : "no");' ],
    [ 'defined_or',       's', 'my $V = (undef // "fallback");' ],
    [ 'defined_or_zero',  's', 'my $V = (0 // "fallback");' ],
    [ 'logical_or_zero',  's', 'my $V = (0 || "fallback");' ],
    [ 'logical_and',      's', 'my $V = (1 && "kept");' ],
    [ 'not_low_prec',     's', 'my $V = (not 0);' ],
    [ 'string_repeat',    's', 'my $V = "ab" x 3;' ],
    [ 'string_increment', 's', 'my $V = "Az"; $V++;' ],
    [ 'string_incr_zz',   's', 'my $V = "zz"; $V++;' ],
    [ 'string_incr_a9',   's', 'my $V = "a9"; $V++;' ],
    [ 'numeric_string',   's', 'my $V = "10" + "5";' ],
    [ 'num_str_concat',   's', 'my $V = 10 . 5;' ],

    # ── strings ──────────────────────────────────────────────────────────────
    [ 'str_length',       's', 'my $V = length("hello world");' ],
    [ 'str_uc_lc',        's', 'my $V = uc(lc("aBcDeF"));' ],
    [ 'str_ucfirst',      's', 'my $V = ucfirst("hello");' ],
    [ 'str_lcfirst',      's', 'my $V = lcfirst("HELLO");' ],
    [ 'str_reverse_sc',   's', 'my $V = scalar reverse "stryke";' ],
    [ 'str_quotemeta',    's', 'my $V = quotemeta(".*+?[]");' ],
    [ 'str_index',        's', 'my $V = index("alphabet", "ph");' ],
    [ 'str_index_miss',   's', 'my $V = index("alphabet", "zz");' ],
    [ 'str_index_pos',    's', 'my $V = index("abcabc", "b", 2);' ],
    [ 'str_rindex',       's', 'my $V = rindex("banana", "na");' ],
    [ 'str_rindex_pos',   's', 'my $V = rindex("banana", "na", 2);' ],
    [ 'str_substr',       's', 'my $V = substr("testing", 1, 3);' ],
    [ 'str_substr_neg',   's', 'my $V = substr("testing", -3);' ],
    [ 'str_substr_negln', 's', 'my $V = substr("testing", 1, -2);' ],
    [ 'str_substr_4arg',  's', 'my $V = "testing"; substr($V, 0, 4) = "TEST";' ],
    [ 'str_substr_lv',    's', 'my $V = "abcdef"; substr($V, 1, 2, "XY");' ],
    [ 'str_ord_chr',      's', 'my $V = chr(ord("A") + 2);' ],
    [ 'str_sprintf_d',    's', 'my $V = sprintf("%05d", 42);' ],
    [ 'str_sprintf_f',    's', 'my $V = sprintf("%.3f", 3.14159);' ],
    [ 'str_sprintf_e',    's', 'my $V = sprintf("%e", 31415.9);' ],
    [ 'str_sprintf_g',    's', 'my $V = sprintf("%g", 0.0000314159);' ],
    [ 'str_sprintf_x',    's', 'my $V = sprintf("%#x|%#o|%#b", 255, 255, 255);' ],
    [ 'str_sprintf_star', 's', 'my $V = sprintf("%*d", 6, 42);' ],
    [ 'str_sprintf_starp','s', 'my $V = sprintf("%.*f", 2, 3.14159);' ],
    [ 'str_sprintf_pos',  's', 'my $V = sprintf(q{%2$s-%1$s}, "a", "b");' ],
    [ 'str_sprintf_plus', 's', 'my $V = sprintf("%+d|% d", 42, 42);' ],
    [ 'str_sprintf_left', 's', 'my $V = sprintf("[%-6s]", "ab");' ],
    [ 'str_sprintf_vec',  's', 'my $V = sprintf("%vd", "1.22.333");' ],
    [ 'str_sprintf_pct',  's', 'my $V = sprintf("100%%");' ],
    [ 'str_sprintf_s_und','s', 'my $V = sprintf("%s", "");' ],
    [ 'str_chop',         's', 'my $t = "abc"; my $V = chop($t); $V .= "|$t";' ],
    [ 'str_chomp',        's', 'my $t = "abc\n"; my $V = chomp($t); $V .= "|$t";' ],
    [ 'str_lc_uc_esc',    's', 'my $V = "\Uab\E-\LCD\E-\ufoo-\lBAR";' ],
    [ 'str_join',         's', 'my $V = join("-", "a", "b", "c");' ],
    [ 'str_x_list',       'a', 'my @V = (1, 2) x 3;' ],
    [ 'str_crypt_free',   's', 'my $V = lc(sprintf("%s", "MiXeD"));' ],
    [ 'str_tr_count',     's', 'my $t = "hello world"; my $V = ($t =~ tr/o/o/);' ],
    [ 'str_tr_mutate',    's', 'my $t = "hello"; $t =~ tr/a-y/b-z/; my $V = $t;' ],
    [ 'str_tr_delete',    's', 'my $t = "hello"; $t =~ tr/l//d; my $V = $t;' ],
    [ 'str_tr_squeeze',   's', 'my $t = "aabbcc"; $t =~ tr/a-z//s; my $V = $t;' ],
    [ 'str_tr_return_r',  's', 'my $V = ("hello" =~ tr/a-y/b-z/r);' ],

    # ── regex ────────────────────────────────────────────────────────────────
    [ 're_match_bool',    's', 'my $V = ("hello" =~ /ell/) ? 1 : 0;' ],
    [ 're_capture',       's', 'my $V = ("2026-08-09" =~ /^(\d+)-(\d+)/) ? "$1/$2" : "no";' ],
    [ 're_named',         's', 'my $V = ("abc" =~ /(?<mid>b)/) ? $+{mid} : "no";' ],
    [ 're_global_list',   'a', 'my @V = ("a1b2c3" =~ /(\d)/g);' ],
    [ 're_subst',         's', 'my $t = "aaa"; $t =~ s/a/b/; my $V = $t;' ],
    [ 're_subst_g',       's', 'my $t = "aaa"; $t =~ s/a/b/g; my $V = $t;' ],
    [ 're_subst_count',   's', 'my $t = "aaa"; my $V = ($t =~ s/a/b/g);' ],
    [ 're_subst_e',       's', 'my $t = "1 2 3"; $t =~ s/(\d)/$1*2/ge; my $V = $t;' ],
    [ 're_subst_ee',      's', 'my $t = "x"; $t =~ s/x/"1+1"/ee; my $V = $t;' ],
    [ 're_subst_r',       's', 'my $V = ("aaa" =~ s/a/b/gr);' ],
    [ 're_case_insens',   's', 'my $V = ("HeLLo" =~ /hello/i) ? 1 : 0;' ],
    [ 're_multiline',     's', 'my @m = ("a\nb" =~ /^(\w)$/mg); my $V = join(",", @m);' ],
    [ 're_extended',      's', 'my $V = ("abc" =~ / a b c /x) ? 1 : 0;' ],
    [ 're_dotall',        's', 'my $V = ("a\nb" =~ /a.b/s) ? 1 : 0;' ],
    [ 're_nongreedy',     's', 'my $V = ("<<a>>" =~ /<(.+?)>/) ? $1 : "no";' ],
    [ 're_lookahead',     's', 'my $V = ("foobar" =~ /foo(?=bar)/) ? 1 : 0;' ],
    [ 're_lookbehind',    's', 'my $V = ("foobar" =~ /(?<=foo)bar/) ? 1 : 0;' ],
    [ 're_alternation',   's', 'my $V = ("cat" =~ /^(dog|cat|cow)$/) ? $1 : "no";' ],
    [ 're_anchors',       's', 'my $V = ("abc" =~ /\Aabc\z/) ? 1 : 0;' ],
    [ 're_backref',       's', 'my $V = ("abab" =~ /(ab)\1/) ? 1 : 0;' ],
    [ 're_prematch',      's', 'my $V = ("hello" =~ /ll/) ? "$`|$&|$\'" : "no";' ],
    [ 're_at_minus_plus', 's', 'my $V = ("hello" =~ /l(l)/) ? "$-[0],$+[0],$-[1],$+[1]" : "no";' ],
    [ 're_pos',           's', 'my $t = "aaa"; $t =~ /a/g; my $V = pos($t);' ],
    [ 're_split_limit',   'a', 'my @V = split(/,/, "a,b,c,d", 2);' ],
    [ 're_split_neg',     'a', 'my @V = split(/,/, "a,b,,", -1);' ],
    [ 're_split_trail',   'a', 'my @V = split(/,/, "a,b,,");' ],
    [ 're_split_capture', 'a', 'my @V = split(/(\d)/, "a1b2c");' ],
    [ 're_split_empty',   'a', 'my @V = split(//, "abc");' ],
    [ 're_split_space',   'a', 'my @V = split(" ", "  a  b  c  ");' ],
    [ 're_split_ws_re',   'a', 'my @V = split(/\s+/, "  a  b  c  ");' ],
    [ 're_qr_interp',     's', 'my $re = qr/b+/; my $V = ("abbbc" =~ $re) ? $& : "no";' ],
    [ 're_qr_stringify',  's', 'my $V = "" . qr/ab/i;' ],

    # ── arrays / lists ───────────────────────────────────────────────────────
    [ 'arr_literal',      'a', 'my @V = (10, 20, 30);' ],
    [ 'arr_range',        'a', 'my @V = (3 .. 9);' ],
    [ 'arr_range_str',    'a', 'my @V = ("aa" .. "ad");' ],
    [ 'arr_scalar_ctx',   's', 'my @a = (1, 2, 3); my $V = @a;' ],
    [ 'arr_last_index',   's', 'my @a = (1, 2, 3); my $V = $#a;' ],
    [ 'arr_neg_index',    's', 'my @a = (1, 2, 3); my $V = $a[-1];' ],
    [ 'arr_oob_index',    's', 'my @a = (1, 2, 3); my $V = $a[99];' ],
    [ 'arr_slice',        'a', 'my @a = (1 .. 9); my @V = @a[2, 4, 6];' ],
    [ 'arr_slice_neg',    'a', 'my @a = (1 .. 9); my @V = @a[-2, -1];' ],
    [ 'arr_slice_range',  'a', 'my @a = (1 .. 9); my @V = @a[1 .. 3];' ],
    [ 'arr_push_ret',     's', 'my @a = (1); my $V = push(@a, 2, 3);' ],
    [ 'arr_pop_ret',      's', 'my @a = (1, 2, 3); my $V = pop(@a);' ],
    [ 'arr_shift_ret',    's', 'my @a = (1, 2, 3); my $V = shift(@a);' ],
    [ 'arr_unshift_ret',  's', 'my @a = (3); my $V = unshift(@a, 1, 2);' ],
    [ 'arr_pop_empty',    's', 'my @a = (); my $V = pop(@a);' ],
    [ 'arr_splice_mid',   'a', 'my @V = (1 .. 6); splice(@V, 1, 2);' ],
    [ 'arr_splice_ins',   'a', 'my @V = (1, 4); splice(@V, 1, 0, 2, 3);' ],
    [ 'arr_splice_ret',   'a', 'my @a = (1 .. 6); my @V = splice(@a, 1, 3);' ],
    [ 'arr_splice_neg',   'a', 'my @V = (1 .. 6); splice(@V, -2);' ],
    [ 'arr_reverse',      'a', 'my @V = reverse(1 .. 5);' ],
    [ 'arr_sort_num',     'a', 'my @V = sort { $a <=> $b } (10, 9, 100, 1);' ],
    [ 'arr_sort_str',     'a', 'my @V = sort (10, 9, 100, 1);' ],
    [ 'arr_sort_desc',    'a', 'my @V = sort { $b <=> $a } (3, 1, 2);' ],
    [ 'arr_sort_stable',  'a', 'my @V = map { $_->[1] } sort { $a->[0] <=> $b->[0] } ([1,"a"],[1,"b"],[0,"c"]);' ],
    [ 'arr_sort_named',   'a', 'sub bynum { $a <=> $b } my @V = sort bynum (3, 1, 2);' ],
    [ 'arr_map',          'a', 'my @V = map { $_ * 2 } (1, 2, 3);' ],
    [ 'arr_map_multi',    'a', 'my @V = map { ($_, $_) } (1, 2);' ],
    [ 'arr_map_empty',    'a', 'my @V = map { $_ > 1 ? $_ : () } (1, 2, 3);' ],
    [ 'arr_grep',         'a', 'my @V = grep { $_ % 2 } (1 .. 6);' ],
    [ 'arr_grep_scalar',  's', 'my $V = grep { $_ % 2 } (1 .. 6);' ],
    [ 'arr_grep_expr',    'a', 'my @V = grep /a/, ("ab", "bc", "ca");' ],
    [ 'arr_wantarray_l',  's', 'sub w { return wantarray ? "list" : "scalar" } my @x = w(); my $V = $x[0];' ],
    [ 'arr_wantarray_s',  's', 'sub w { return wantarray ? "list" : "scalar" } my $V = w();' ],
    [ 'arr_flatten',      'a', 'my @inner = (2, 3); my @V = (1, @inner, 4);' ],
    [ 'arr_list_assign',  's', 'my ($x, $y, @rest) = (1, 2, 3, 4); my $V = "$x|$y|@rest";' ],
    [ 'arr_swap',         's', 'my ($x, $y) = (1, 2); ($x, $y) = ($y, $x); my $V = "$x$y";' ],
    [ 'arr_count_assign', 's', 'my $V = () = (1, 2, 3);' ],
    [ 'arr_join_nested',  's', 'my @a = (1, [2, 3]); my $V = ref($a[1]) . scalar(@a);' ],
    [ 'arr_exists_del',   's', 'my @a = (1, 2, 3); my $V = (exists $a[1] ? "y" : "n") . (exists $a[9] ? "y" : "n");' ],
    [ 'arr_local_sep',    's', 'my @a = (1, 2, 3); local $" = ":"; my $V = "@a";' ],
    [ 'arr_interp',       's', 'my @a = (1, 2, 3); my $V = "@a";' ],
    [ 'arr_interp_slice', 's', 'my @a = (1 .. 5); my $V = "@a[1,2]";' ],
    [ 'arr_lc_in_interp', 's', 'my @a = (1, 2); my $V = "pre-@{[ scalar(@a) * 10 ]}-post";' ],

    # ── hashes ───────────────────────────────────────────────────────────────
    [ 'hash_lookup',      's', 'my %h = (a => 1, b => 2); my $V = $h{b};' ],
    [ 'hash_count',       's', 'my %h = (a => 1, b => 2, c => 3); my $V = scalar keys %h;' ],
    [ 'hash_sorted_keys', 'a', 'my %h = (b => 2, a => 1, c => 3); my @V = sort keys %h;' ],
    [ 'hash_sorted_vals', 'a', 'my %h = (b => 2, a => 1, c => 3); my @V = map { $h{$_} } sort keys %h;' ],
    [ 'hash_exists',      's', 'my %h = (a => undef); my $V = (exists $h{a} ? "y" : "n") . (defined $h{a} ? "y" : "n");' ],
    [ 'hash_delete_ret',  's', 'my %h = (a => 1, b => 2); my $V = delete $h{a};' ],
    [ 'hash_delete_miss', 's', 'my %h = (a => 1); my $V = delete $h{zz};' ],
    [ 'hash_slice',       'a', 'my %h = (a => 1, b => 2, c => 3); my @V = @h{qw(a c)};' ],
    [ 'hash_kv_slice',    'a', 'my %h = (a => 1, b => 2); my @V = map { "$_=$h{$_}" } sort keys %h;' ],
    [ 'hash_scalar_ctx',  's', 'my %h = (a => 1, b => 2); my $V = (scalar(%h) ? "true" : "false");' ],
    [ 'hash_each_sorted', 's', 'my %h = (a => 1, b => 2); my @p; while (my ($k, $v) = each %h) { push @p, "$k$v" } my $V = join(",", sort @p);' ],
    [ 'hash_from_list',   's', 'my @l = (a => 1, b => 2); my %h = @l; my $V = join(",", map { "$_$h{$_}" } sort keys %h);' ],
    [ 'hash_autoviv',     's', 'my %h; $h{a}{b} = 1; my $V = (exists $h{a} ? "y" : "n") . ref($h{a});' ],
    [ 'hash_nested',      's', 'my %h = (x => { y => [1, 2] }); my $V = $h{x}{y}[1];' ],
    [ 'hash_interp',      's', 'my %h = (k => "v"); my $V = "val=$h{k}";' ],

    # ── references / OO ──────────────────────────────────────────────────────
    [ 'ref_array',        's', 'my $V = ref([1, 2]);' ],
    [ 'ref_hash',         's', 'my $V = ref({ a => 1 });' ],
    [ 'ref_code',         's', 'my $V = ref(sub { 1 });' ],
    [ 'ref_scalar',       's', 'my $x = 1; my $V = ref(\$x);' ],
    [ 'ref_ref',          's', 'my $x = 1; my $r = \$x; my $V = ref(\$r);' ],
    [ 'ref_regex',        's', 'my $V = ref(qr/x/);' ],
    [ 'ref_deref_arr',    's', 'my $r = [1, 2, 3]; my $V = "@{$r}[0] $$r[1] $r->[2]";' ],
    [ 'ref_deref_hash',   's', 'my $r = { a => 1 }; my $V = "${$r}{a} $$r{a} $r->{a}";' ],
    [ 'ref_postfix',      's', 'my $r = [1, 2, 3]; my $V = scalar @{$r};' ],
    [ 'ref_code_call',    's', 'my $c = sub { return $_[0] * 3 }; my $V = $c->(7);' ],
    [ 'ref_code_amp',     's', 'my $c = sub { return 5 }; my $V = &$c();' ],
    [ 'ref_bless',        's', 'my $o = bless {}, "Foo"; my $V = ref($o);' ],
    [ 'ref_isa',          's', 'my $o = bless {}, "Foo"; my $V = ($o->isa("Foo") ? 1 : 0);' ],
    [ 'ref_can',          's', 'sub Foo::hi { 42 } my $o = bless {}, "Foo"; my $V = ($o->can("hi") ? "y" : "n");' ],
    [ 'ref_method',       's', 'sub Foo::val { return 7 } my $o = bless {}, "Foo"; my $V = $o->val;' ],
    [ 'ref_method_args',  's', 'sub Foo::add { my ($s, $n) = @_; return $n + 1 } my $o = bless {}, "Foo"; my $V = $o->add(41);' ],
    [ 'ref_class_method', 's', 'sub Foo::mk { return bless {}, $_[0] } my $V = ref(Foo->mk);' ],
    [ 'ref_inherit',      's', 'sub Base::who { "base" } @Derived::ISA = ("Base"); my $o = bless {}, "Derived"; my $V = $o->who;' ],
    [ 'ref_super',        's', 'sub Base::who { "base" } { package Derived; our @ISA = ("Base"); sub who { my $s = shift; return "d+" . $s->SUPER::who() } } my $o = bless {}, "Derived"; my $V = $o->who;' ],
    [ 'ref_weak_free',    's', 'my $r = \my @tmp; push @$r, 1, 2; my $V = scalar @$r;' ],
    [ 'ref_circular',     's', 'my $h = {}; $h->{self} = $h; my $V = ref($h->{self}{self});' ],
    [ 'ref_anon_nest',    's', 'my $d = { list => [1, { k => "deep" }] }; my $V = $d->{list}[1]{k};' ],
    [ 'ref_arrow_chain',  's', 'my $d = [[1, 2], [3, 4]]; my $V = $d->[1][0];' ],
    [ 'ref_code_closure',  's', 'my $n = 10; my $c = sub { return $n++ }; $c->(); my $V = $c->() . "|" . $n;' ],
    [ 'ref_string_over',  's', 'my $o = bless {}, "Foo"; my $V = (("$o" =~ /^Foo=HASH\(0x[0-9a-f]+\)$/) ? "ok" : "bad:$o");' ],

    # ── control flow / context ───────────────────────────────────────────────
    [ 'ctl_last',         's', 'my $V = 0; for my $i (1 .. 10) { last if $i > 3; $V = $i }' ],
    [ 'ctl_next',         's', 'my $V = ""; for my $i (1 .. 5) { next if $i % 2; $V .= $i }' ],
    [ 'ctl_redo_free',    's', 'my $V = 0; my $n = 0; while ($n < 3) { $n++; $V += $n }' ],
    [ 'ctl_labeled_last', 's', 'my $V = ""; OUTER: for my $i (1 .. 3) { for my $j (1 .. 3) { next OUTER if $j == 2; $V .= "$i$j" } }' ],
    [ 'ctl_do_while',     's', 'my $V = 0; my $n = 0; do { $n++; $V += $n } while ($n < 3);' ],
    [ 'ctl_until',        's', 'my $V = 0; until ($V >= 5) { $V += 2 }' ],
    [ 'ctl_unless',       's', 'my $V = "no"; unless (0) { $V = "yes" }' ],
    [ 'ctl_postfix_for',  's', 'my $V = ""; $V .= $_ for (1 .. 4);' ],
    [ 'ctl_c_style',      's', 'my $V = 0; for (my $i = 0; $i < 4; $i++) { $V += $i }' ],
    [ 'ctl_foreach_alias','s', 'my @a = (1, 2, 3); $_ *= 2 for @a; my $V = "@a";' ],
    [ 'ctl_nested_sub',   's', 'sub outer { my $x = shift; return inner($x) + 1 } sub inner { return $_[0] * 2 } my $V = outer(5);' ],
    [ 'ctl_recursion',    's', 'sub fact { my $n = shift; return $n <= 1 ? 1 : $n * fact($n - 1) } my $V = fact(10);' ],
    [ 'ctl_mutual_rec',   's', 'sub ev { my $n = shift; return $n == 0 ? 1 : od($n - 1) } sub od { my $n = shift; return $n == 0 ? 0 : ev($n - 1) } my $V = ev(10);' ],
    [ 'ctl_local_dyn',    's', 'our $g = "outer"; sub rd { return $g } sub wr { local $g = "inner"; return rd() } my $V = wr() . "|" . rd();' ],
    [ 'ctl_eval_die',     's', 'my $V = eval { die "boom\n"; 1 } || "caught:$@"; chomp $V;' ],
    [ 'ctl_eval_ok',      's', 'my $V = eval { 42 };' ],
    [ 'ctl_eval_string',  's', 'my $V = eval "2 + 3";' ],
    [ 'ctl_eval_syntaxe', 's', 'my $r = eval "2 +"; my $V = defined($r) ? "def" : "undef:" . (($@ ne "") ? "err" : "noerr");' ],
    [ 'ctl_die_object',   's', 'my $V = eval { die { code => 7 } } || (ref($@) eq "HASH" ? "hash:$@->{code}" : "other");' ],
    [ 'ctl_nested_eval',  's', 'my $V = eval { eval { die "inner\n" }; my $e = $@; chomp $e; die "outer($e)\n" } || $@; chomp $V;' ],
    [ 'ctl_warn_free',    's', 'my $V = "ok";' ],
    [ 'ctl_return_list',  'a', 'sub lst { return (1, 2, 3) } my @V = lst();' ],
    [ 'ctl_return_empty', 's', 'sub emp { return } my @x = emp(); my $V = scalar @x;' ],
    [ 'ctl_return_scalar','s', 'sub lst { return (1, 2, 3) } my $V = lst();' ],
    [ 'ctl_implicit_ret', 's', 'sub imp { my $x = shift; $x * 3 } my $V = imp(4);' ],
    [ 'ctl_implicit_if',  's', 'sub imp { my $x = shift; if ($x) { "yes" } else { "no" } } my $V = imp(1) . imp(0);' ],
    [ 'ctl_goto_free',    's', 'my $V = join(",", map { $_ } 1 .. 3);' ],
    [ 'ctl_args_alias',   's', 'sub bump { $_[0]++ } my $x = 5; bump($x); my $V = $x;' ],
    [ 'ctl_args_count',   's', 'sub cnt { return scalar @_ } my $V = cnt(1, 2, 3);' ],
    [ 'ctl_default_arg',  's', 'sub d { my $x = shift // "def"; return $x } my $V = d() . "|" . d("set");' ],
    [ 'ctl_closure_loop', 's', 'my @c; for my $i (1 .. 3) { push @c, sub { $i } } my $V = join(",", map { $_->() } @c);' ],
    [ 'ctl_closure_share','s', 'my $n = 0; my $inc = sub { $n++ }; my $get = sub { $n }; $inc->(); $inc->(); my $V = $get->();' ],
    [ 'ctl_string_eval_c','s', 'my $c = eval q{ sub { $_[0] + 1 } }; my $V = $c->(41);' ],
    [ 'ctl_sort_in_sub',  's', 'sub srt { return join(",", sort { $a <=> $b } @_) } my $V = srt(3, 1, 2);' ],
    [ 'ctl_map_in_sub',   's', 'sub mp { return join(",", map { $_ + 1 } @_) } my $V = mp(1, 2, 3);' ],
    [ 'ctl_grep_in_sub',  's', 'sub gp { return join(",", grep { $_ > 1 } @_) } my $V = gp(1, 2, 3);' ],
    [ 'ctl_nested_close',  's', 'my $mk = sub { my $b = shift; return sub { return $b + $_[0] } }; my $add5 = $mk->(5); my $V = $add5->(10);' ],

    # ── numeric formatting / stringification ─────────────────────────────────
    [ 'num_str_int',      's', 'my $V = 42 . "";' ],
    [ 'num_str_float',    's', 'my $V = 0.1 + 0.2;' ],
    [ 'num_str_big',      's', 'my $V = 2 ** 53;' ],
    [ 'num_str_bigger',   's', 'my $V = 2 ** 53 + 1;' ],
    [ 'num_str_neg_zero', 's', 'my $V = -0.0;' ],
    [ 'num_str_sci',      's', 'my $V = 1e21;' ],
    [ 'num_str_small',    's', 'my $V = 1e-7;' ],
    [ 'num_str_div',      's', 'my $V = 1 / 3;' ],
    [ 'num_str_intmax',   's', 'my $V = 9007199254740993;' ],
    [ 'num_str_hexlit',   's', 'my $V = 0xFF + 0b101 + 0o17 + 017;' ],
    [ 'num_underscore',   's', 'my $V = 1_000_000;' ],
    [ 'num_str_inc_float','s', 'my $V = 0.1; $V += 0.2; $V .= "";' ],

    # ── pack / unpack ────────────────────────────────────────────────────────
    [ 'pack_C',           's', 'my $V = unpack("H*", pack("C*", 1, 255, 16));' ],
    [ 'pack_n_N',         's', 'my $V = unpack("H*", pack("nN", 258, 66051));' ],
    [ 'pack_v_V',         's', 'my $V = unpack("H*", pack("vV", 258, 66051));' ],
    [ 'pack_A_a',         's', 'my $V = "[" . pack("A5a5", "ab", "cd") . "]";' ],
    [ 'pack_unpack_A',    's', 'my $V = join("|", unpack("A3A3", "ab cd "));' ],
    [ 'pack_Z',           's', 'my $V = unpack("H*", pack("Z4", "ab"));' ],
    [ 'pack_w_free',      's', 'my $V = join(",", unpack("C*", pack("N", 1)));' ],
    [ 'pack_star',        's', 'my $V = join(",", unpack("C*", pack("A*", "hi")));' ],

    # ── file tests ───────────────────────────────────────────────────────────
    #
    # `-t` takes a FILEHANDLE, so it has the two falsehoods the other file
    # tests have, but split on a different axis: a handle operand is always
    # DEFINED (1 for a terminal, "" otherwise) and a path names no handle at
    # all, so it is undef. Testing the operand as a path inverts both halves,
    # which is why the handle and the path forms are probed separately.
    # Definedness and length() are rendered rather than the bare value because
    # undef, "" and "0" all print as nothing or near-nothing on their own.
    #
    # These are terminal-state independent: perl and st are run the same way,
    # so both observe the same stdin/stdout, whatever the harness was given.
    [ 'filetest_t_handle','s', 'my $r = -t STDIN; my $V = defined($r) ? "def:" . length($r) : "undef";' ],
    [ 'filetest_t_stdout','s', 'my $r = -t STDOUT; my $V = defined($r) ? "def:" . length($r) : "undef";' ],
    [ 'filetest_t_fd',    's', 'my $d = "0"; my $r = -t $d; my $V = defined($r) ? "def:" . length($r) : "undef";' ],
    [ 'filetest_t_path',  's', 'my $p = $0; my $r = -t $p; my $V = defined($r) ? "def:" . length($r) : "undef";' ],
    [ 'filetest_t_devfd', 's', 'my $p = "/dev/fd/1"; my $r = -t $p; my $V = defined($r) ? "def:" . length($r) : "undef";' ],
    [ 'filetest_t_nofh',  's', 'my $p = "NOPEFH"; my $r = -t $p; my $V = defined($r) ? "def:" . length($r) : "undef";' ],

    # ── misc builtins ────────────────────────────────────────────────────────
    [ 'misc_lcuc_sort',   'a', 'my @V = sort { lc($a) cmp lc($b) or $a cmp $b } qw(B a A b);' ],
    [ 'misc_sprintf_list','s', 'my @a = (1, 2); my $V = sprintf("%d-%d", @a);' ],
    [ 'misc_printf_ret',  's', 'my $V = sprintf("%s", join("", map { chr(65 + $_) } 0 .. 3));' ],
    [ 'misc_string_multi','s', 'my $V = "a" . "b" x 2 . "c";' ],
    [ 'misc_chained_cmp', 's', 'my $V = ((1 <=> 2) || ("a" cmp "b"));' ],
    [ 'misc_list_in_bool','s', 'my @e = (); my $V = (@e ? "t" : "f");' ],
    [ 'misc_undef_warn',  's', 'my $V = defined(undef) ? "d" : "u";' ],
    [ 'misc_exists_sub',  's', 'sub known {} my $V = (defined(&known) ? "y" : "n") . (defined(&unknown) ? "y" : "n");' ],
    [ 'misc_ref_eq',      's', 'my $r = [1]; my $s = $r; my $V = ($r == $s ? "same" : "diff");' ],
    [ 'misc_num_str_eq',  's', 'my $V = ("1.0" == 1 ? "numeq" : "numne") . ("1.0" eq "1" ? "streq" : "strne");' ],
    [ 'misc_sort_uniq',   'a', 'my %seen; my @V = grep { !$seen{$_}++ } qw(b a b c a);' ],
    [ 'misc_wantarray_u', 's', 'sub ctx { return defined(wantarray) ? (wantarray ? "list" : "scalar") : "void" } my $V = ctx();' ],
    [ 'misc_sprintf_undef','s','my $V = length(sprintf("%s", ""));' ],
    [ 'misc_lc_unicode',  's', 'my $V = lc("ÄÖÜ") . uc("äöü");' ],
    [ 'misc_length_utf8', 's', 'use utf8; my $V = length("héllo");' ],
    [ 'misc_ord_high',    's', 'my $V = ord("\x{263A}");' ],
    [ 'misc_sprintf_c',   's', 'my $V = sprintf("%c%c", 72, 105);' ],
    [ 'misc_reverse_hash','s', 'my %h = (a => 1, b => 2); my %r = reverse %h; my $V = join(",", map { "$_=$r{$_}" } sort keys %r);' ],
    [ 'misc_nested_ternary','s','my $x = 5; my $V = $x < 3 ? "lo" : $x < 7 ? "mid" : "hi";' ],
    [ 'misc_chained_str', 's', 'my $V = "a"; $V .= "b" for 1 .. 3;' ],
    [ 'misc_hash_in_list','s', 'my %h = (a => 1); my @l = (%h); my $V = scalar @l;' ],
    [ 'misc_deep_copy',   's', 'my @o = ([1], [2]); my @c = map { [@$_] } @o; $c[0][0] = 9; my $V = "$o[0][0]$c[0][0]";' ],
);

# ── Context matrix ───────────────────────────────────────────────────────────
#
# Each context wraps the same statement in a different execution environment.
# `%s` is the statement, `%r` the render expression. A construct that is correct
# at top level but wrong inside a closure — or vice versa — is precisely the
# cross-context bug class this matrix exists to surface.

my %RENDER = (
    's' => 'defined($V) ? "S:" . $V : "S:<undef>"',
    'a' => '"A:" . scalar(@V) . ":" . join(",", map { defined($_) ? $_ : "<undef>" } @V)',
);

my %CONTEXT = (
    'top'     => sub { my ($s, $r) = @_; "$s\nprint(($r), \"\\n\");\n" },
    'sub'     => sub { my ($s, $r) = @_; "sub probe_ctx { $s\n    return ($r); }\nprint(probe_ctx(), \"\\n\");\n" },
    'closure' => sub { my ($s, $r) = @_; "my \$probe_ctx = sub { $s\n    return ($r); };\nprint(\$probe_ctx->(), \"\\n\");\n" },
    'loop'    => sub { my ($s, $r) = @_; "for my \$probe_iter (1 .. 2) { $s\n    print(($r), \"\\n\"); }\n" },
    'tail'    => sub { my ($s, $r) = @_; "sub probe_tail { $s\n    ($r); }\nprint(probe_tail(), \"\\n\");\n" },

    # `hot` exists because every context above executes its subroutine a
    # handful of times, and stryke only promotes a sub to its native tiers
    # after ~50 invocations (`STRYKE_JIT_SUB_INVOKES`, default 50). So the
    # whole compiled-tier answer — Cranelift and the fusevm segment bridge —
    # was unreachable from this probe: a construct could be right in the
    # interpreter and wrong once jitted and still score clean everywhere.
    # $HOT_ITERS calls clear the threshold with margin, and only the final
    # value is printed so the output stays one line like every other context.
    'hot'     => sub {
        my ($s, $r) = @_;
        "sub probe_hot { $s\n    return ($r); }\n"
          . "my \$probe_out;\n"
          . "for my \$probe_iter (1 .. $HOT_ITERS) { \$probe_out = probe_hot(); }\n"
          . "print(\$probe_out, \"\\n\");\n";
    },
);

my @ctx_order = grep { exists $CONTEXT{$_} } split(/,/, $CONTEXTS);
die "probe: no valid contexts in '$CONTEXTS'\n" unless @ctx_order;

# ── Runner ───────────────────────────────────────────────────────────────────

# Run a command with a hard timeout. Returns (stdout+stderr, exit_code, timed_out).
sub run_capture {
    my ($timeout, @cmd) = @_;
    my $out_file = "$ENV{PROBE_TMP}/run.out";
    my $pid      = fork();
    die "probe: fork failed: $!\n" unless defined $pid;
    if ($pid == 0) {
        open(STDOUT, '>', $out_file) or exit 127;
        open(STDERR, '>&', \*STDOUT) or exit 127;
        open(STDIN,  '<', '/dev/null');
        exec(@cmd) or exit 127;
    }
    my $timed_out = 0;
    my $status;
    eval {
        local $SIG{ALRM} = sub { die "timeout\n" };
        alarm($timeout);
        waitpid($pid, 0);
        $status = $?;
        alarm(0);
        1;
    } or do {
        alarm(0);
        $timed_out = 1;
        kill('KILL', $pid);
        waitpid($pid, 0);
        $status = -1;
    };
    my $out = '';
    if (open(my $fh, '<', $out_file)) {
        local $/;
        $out = <$fh> // '';
        close($fh);
    }
    unlink($out_file);
    my $code = $timed_out ? -1 : ($status >> 8);
    return ($out, $code, $timed_out);
}

my $tmp = $KEEP_DIR ? $KEEP_DIR : tempdir("stryke-probe.XXXXXX", TMPDIR => 1, CLEANUP => 1);
make_path($tmp) unless -d $tmp;
$ENV{PROBE_TMP} = $tmp;
$ENV{LC_ALL}    = 'C';
$ENV{LANG}      = 'C';

open(my $flog, '>', $FAIL_LOG) or die "probe: cannot write $FAIL_LOG: $!\n";

my %tally = (
    total          => 0,
    passed         => 0,
    div_jit        => 0,
    div_nojit      => 0,
    div_both       => 0,
    div_jit_only   => 0,
    div_nojit_only => 0,
    skip_oracle    => 0,
    skip_nondet    => 0,
    skip_timeout   => 0,
    st_timeout     => 0,
);
my @divergences;
my @skips;

for my $p (@PROBES) {
    my ($id, $kind, $stmt) = @$p;
    my $render = $RENDER{$kind} or die "probe: bad kind '$kind' for $id\n";

    for my $ctx (@ctx_order) {
        my $case = "$id\@$ctx";
        next if $FILTER && $case !~ /$FILTER/;
        $tally{total}++;

        my $body = $CONTEXT{$ctx}->($stmt, $render);
        my $src  = "use strict;\nuse warnings;\n$body";
        my $file = "$tmp/$id.$ctx.pl";
        open(my $fh, '>', $file) or die "probe: cannot write $file: $!\n";
        print $fh $src;
        close($fh);

        # ── oracle gate 1: perl must succeed ─────────────────────────────────
        my ($p1, $pc1, $pt1) = run_capture($TIMEOUT, $PERL, $file);
        if ($pt1) {
            $tally{skip_timeout}++;
            push @skips, [ $case, 'oracle-timeout', '' ];
            next;
        }
        if ($pc1 != 0) {
            $tally{skip_oracle}++;
            push @skips, [ $case, 'oracle-nonzero-exit', $p1 ];
            next;
        }

        # ── oracle gate 2: perl must be deterministic ────────────────────────
        my ($p2, $pc2, $pt2) = run_capture($TIMEOUT, $PERL, $file);
        if ($pt2 || $pc2 != 0 || $p2 ne $p1) {
            $tally{skip_nondet}++;
            push @skips, [ $case, 'oracle-nondeterministic', "run1:$p1\nrun2:$p2" ];
            next;
        }

        # ── both stryke execution paths, scored independently ────────────────
        my ($j_out, $j_code, $j_to) = run_capture($TIMEOUT, $ST, '--compat', $file);
        my ($n_out, $n_code, $n_to) = run_capture($TIMEOUT, $ST, '--compat', '--no-jit', $file);

        $tally{st_timeout}++ if $j_to || $n_to;

        my $j_bad = ($j_to || $j_out ne $p1) ? 1 : 0;
        my $n_bad = ($n_to || $n_out ne $p1) ? 1 : 0;

        if (!$j_bad && !$n_bad) {
            $tally{passed}++;
            next;
        }

        $tally{div_jit}++        if $j_bad;
        $tally{div_nojit}++      if $n_bad;
        $tally{div_both}++       if $j_bad && $n_bad;
        $tally{div_jit_only}++   if $j_bad && !$n_bad;
        $tally{div_nojit_only}++ if $n_bad && !$j_bad;

        my $class = ($j_bad && $n_bad) ? 'both'
                  : $j_bad             ? 'jit-only'
                  :                      'nojit-only';
        push @divergences, [ $case, $class ];
        print STDERR "probe DIVERGE [$class]: $case\n" unless $QUIET;

        print $flog "==== $case  [$class] ====\n";
        print $flog "--- source ---\n$src";
        print $flog "--- perl (exit $pc1) ---\n$p1";
        print $flog "--- st --compat (exit $j_code" . ($j_to ? ", TIMEOUT" : "") . ") ---\n$j_out";
        print $flog "--- st --compat --no-jit (exit $n_code" . ($n_to ? ", TIMEOUT" : "") . ") ---\n$n_out";
        print $flog "\n";
    }
}

close($flog);

my $skipped = $tally{skip_oracle} + $tally{skip_nondet} + $tally{skip_timeout};
my $scored  = $tally{total} - $skipped;
my $diverged = scalar @divergences;

printf("probe: scored %d/%d · passed %d · DIVERGED %d · SKIPPED %d\n",
    $scored, $tally{total}, $tally{passed}, $diverged, $skipped);
printf("probe: divergence split — both-paths %d · jit-only %d · nojit-only %d\n",
    $tally{div_both}, $tally{div_jit_only}, $tally{div_nojit_only});
printf("probe: skip split — oracle-nonzero-exit %d · oracle-nondeterministic %d · oracle-timeout %d · st-timeout %d\n",
    $tally{skip_oracle}, $tally{skip_nondet}, $tally{skip_timeout}, $tally{st_timeout});
print "probe: divergence detail in $FAIL_LOG\n" if $diverged;

if ($JSON_OUT) {
    open(my $jh, '>', $JSON_OUT) or die "probe: cannot write $JSON_OUT: $!\n";
    printf $jh "{\n  \"total\": %d,\n  \"scored\": %d,\n  \"passed\": %d,\n  \"diverged\": %d,\n  \"skipped\": %d,\n  \"div_both\": %d,\n  \"div_jit_only\": %d,\n  \"div_nojit_only\": %d,\n  \"skip_oracle_exit\": %d,\n  \"skip_nondeterministic\": %d,\n  \"skip_oracle_timeout\": %d,\n  \"st_timeout\": %d\n}\n",
        $tally{total}, $scored, $tally{passed}, $diverged, $skipped,
        $tally{div_both}, $tally{div_jit_only}, $tally{div_nojit_only},
        $tally{skip_oracle}, $tally{skip_nondet}, $tally{skip_timeout}, $tally{st_timeout};
    close($jh);
}

exit($diverged ? 1 : 0);
