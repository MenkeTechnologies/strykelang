//! Grammar-aware differential test against stock `perl(1)`.
//!
//! A seeded generator emits scalar expressions over a small Perl grammar
//! (integer and float arithmetic, string builtins, `sprintf`, list builtins,
//! comparisons, ternaries). Each expression is evaluated under `eval { }` and
//! printed on its own line, the program runs through `perl` and
//! `stryke --compat`, and the two stdout streams must match line for line. A
//! mismatch names the generated expression, so the failure is directly
//! reproducible as a one-liner.
//!
//! The grammar keeps integer magnitudes small enough that no result leaves the
//! signed 64-bit range: unsigned (UV) scalars are a separate, documented gap
//! and are pinned by `parity/cases/` instead.
//!
//! If `perl` is not on `$PATH` the tests no-op, like the other parity suites.

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

const STRINGS: &[&str] = &[
    "\"\"",
    "\"a\"",
    "\"abc\"",
    "\"Hello\"",
    "\"x y\"",
    "\"12\"",
    "\"3.50\"",
    "\"-4\"",
    "\"0\"",
    "\"007\"",
    "\"a,b,c\"",
];

const FORMATS: &[&str] = &[
    "%d", "%s", "%5d", "%-5d|", "%05d", "%x", "%o", "%e", "%.2f", "%5.1f", "%g", "%+d", "% d",
    "%3s", "%-3s|", "%.1s", "%c", "%%", "%b",
];

/// A numeric-valued expression. `depth` bounds nesting.
fn num(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 {
        return match rng.below(4) {
            0 => format!("({})", rng.below(2001) as i64 - 1000),
            1 => format!("{}", rng.below(100)),
            2 => format!("({}.{})", rng.below(50), rng.below(100)),
            _ => format!("(-{}.{})", rng.below(20), rng.below(10)),
        };
    }
    let d = depth - 1;
    match rng.below(14) {
        0 => format!("({} + {})", num(rng, d), num(rng, d)),
        1 => format!("({} - {})", num(rng, d), num(rng, d)),
        2 => format!("({} * {})", num(rng, d), num(rng, d)),
        3 => format!("({} / {})", num(rng, d), num(rng, d)),
        4 => format!("({} % {})", num(rng, d), num(rng, d)),
        5 => format!("({} ** {})", num(rng, 0), rng.below(4)),
        6 => format!("int({})", num(rng, d)),
        7 => format!("abs({})", num(rng, d)),
        8 => format!("(-{})", num(rng, d)),
        9 => format!("length({})", string(rng, d)),
        10 => format!("index({}, {})", string(rng, d), string(rng, 0)),
        11 => format!("ord({})", string(rng, d)),
        12 => format!(
            "({} {} {} ? {} : {})",
            num(rng, d),
            rng.pick(&["<", ">", "<=", ">=", "==", "!="]),
            num(rng, d),
            num(rng, d),
            num(rng, d)
        ),
        _ => format!("({} <=> {})", num(rng, d), num(rng, d)),
    }
}

/// A string-valued expression.
fn string(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 {
        return rng.pick(STRINGS).to_string();
    }
    let d = depth - 1;
    match rng.below(14) {
        0 => format!("({} . {})", string(rng, d), string(rng, d)),
        1 => format!("({} x {})", string(rng, d), rng.below(4)),
        2 => format!("uc({})", string(rng, d)),
        3 => format!("lc({})", string(rng, d)),
        4 => format!("ucfirst({})", string(rng, d)),
        5 => format!("lcfirst({})", string(rng, d)),
        6 => format!("scalar reverse({})", string(rng, d)),
        7 => format!(
            "substr({}, {}, {})",
            string(rng, d),
            rng.below(4),
            rng.below(5)
        ),
        8 => format!("sprintf(\"{}\", {})", rng.pick(FORMATS), num(rng, d)),
        9 => format!("sprintf(\"{}\", {})", rng.pick(FORMATS), string(rng, d)),
        10 => format!("join(\",\", {})", list(rng, d)),
        11 => format!("({} cmp {})", string(rng, d), string(rng, d)),
        12 => format!("({} eq {} ? \"y\" : \"n\")", string(rng, d), string(rng, d)),
        _ => format!("({} . {})", num(rng, d), string(rng, d)),
    }
}

/// A list-valued expression (always parenthesised).
fn list(rng: &mut Rng, depth: u32) -> String {
    let d = depth.saturating_sub(1);
    let items = |rng: &mut Rng| -> String {
        (0..1 + rng.below(4))
            .map(|_| num(rng, d.min(1)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match rng.below(5) {
        0 => format!("({})", items(rng)),
        1 => format!("(sort {{ $a <=> $b }} ({}))", items(rng)),
        2 => format!("(reverse ({}))", items(rng)),
        3 => format!("(map {{ $_ * 2 }} ({}))", items(rng)),
        _ => format!("(grep {{ $_ > 0 }} ({}))", items(rng)),
    }
}

fn generate(seed: u64, count: usize) -> Vec<String> {
    let mut rng = Rng::new(seed);
    (0..count)
        .map(|_| {
            let kind = rng.below(2);
            let depth = 1 + rng.below(3) as u32;
            if kind == 0 {
                num(&mut rng, depth)
            } else {
                string(&mut rng, depth)
            }
        })
        .collect()
}

fn run_program(interpreter: &[&str], script: &std::path::Path) -> Option<String> {
    let (bin, flags) = interpreter.split_first()?;
    let out = Command::new(bin)
        .args(flags)
        .arg(script)
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn check_seed(seed: u64) {
    if Command::new("perl").arg("-e").arg("1").output().is_err() {
        eprintln!("skip: perl(1) not available");
        return;
    }
    let exprs = generate(seed, 120);
    let mut program = String::new();
    for e in &exprs {
        program.push_str(&format!(
            "{{ my $r = eval {{ {e} }}; print defined($r) ? $r : \"undef\", \"|\", ($@ ? \"ERR\" : \"ok\"), \"\\n\"; }}\n"
        ));
    }
    let dir = std::env::temp_dir();
    let script = dir.join(format!("stryke_diff_gen_{}_{seed}.pl", std::process::id()));
    fs::write(&script, &program).expect("write generated program");

    let perl_out = run_program(&["perl"], &script).expect("run perl");
    let st = env!("CARGO_BIN_EXE_st");
    let st_out = run_program(&[st, "--compat"], &script).expect("run st");
    let _ = fs::remove_file(&script);

    let perl_lines: Vec<&str> = perl_out.lines().collect();
    let st_lines: Vec<&str> = st_out.lines().collect();
    assert_eq!(
        perl_lines.len(),
        exprs.len(),
        "perl produced {} lines for {} expressions (seed {seed})",
        perl_lines.len(),
        exprs.len()
    );
    let mut diffs = Vec::new();
    for (i, expr) in exprs.iter().enumerate() {
        let got = st_lines.get(i).copied().unwrap_or("<missing>");
        if perl_lines[i] != got {
            diffs.push(format!(
                "  {expr}\n    perl: {}\n    st:   {got}",
                perl_lines[i]
            ));
        }
    }
    assert!(
        diffs.is_empty(),
        "seed {seed}: {} of {} generated expressions diverge from perl:\n{}",
        diffs.len(),
        exprs.len(),
        diffs.join("\n")
    );
}

#[test]
fn generated_expressions_match_perl_seed_1() {
    check_seed(1);
}

#[test]
fn generated_expressions_match_perl_seed_2() {
    check_seed(2);
}

#[test]
fn generated_expressions_match_perl_seed_3() {
    check_seed(3);
}
