//! Parse-only smoke tests (batch 3). Conservative snippets that must parse.

use crate::parse;

macro_rules! parse_batch3 {
    ($($name:ident => $src:expr;)*) => {
        $(#[test] fn $name() { parse($src).expect("parse"); })*
    };
}

parse_batch3! {
    parse_b3_001 => "my $v = sin(0);";
    parse_b3_002 => "my $v = cos(0);";
    parse_b3_003 => "my $v = atan2(1, 1);";
    parse_b3_004 => "my $v = exp(0);";
    parse_b3_005 => "my $v = log(2.718281828);";
    parse_b3_006 => "my $r = rand();";
    parse_b3_007 => "srand(12345);";
    parse_b3_008 => "my $x = bless {}, 'C';";
    parse_b3_009 => "my $c = caller();";
    parse_b3_010 => "pack('C*', 65, 66);";
    parse_b3_011 => "unpack('C*', 'AB');";
    parse_b3_012 => "quotemeta('a.b');";
    parse_b3_013 => "alarm 1;";
    parse_b3_014 => "sleep 0;";
    parse_b3_015 => "umask 022;";
    parse_b3_016 => "times();";
    parse_b3_017 => "time();";
    parse_b3_018 => "localtime();";
    parse_b3_019 => "gmtime();";
    parse_b3_020 => "getlogin();";
    parse_b3_021 => "getpwuid(0);";
    parse_b3_022 => "getpwnam('root');";
    parse_b3_023 => "getgrnam('wheel');";
    parse_b3_024 => "gethostbyname('localhost');";
    parse_b3_025 => "getprotobyname('tcp');";
    parse_b3_026 => "getservbyname('http', 'tcp');";
    parse_b3_027 => "socketpair S1, S2, 1, 1, 0;";
    parse_b3_028 => "shutdown S, 2;";
    parse_b3_029 => "listen S, 5;";
    parse_b3_030 => "accept NS, S;";
    parse_b3_031 => "connect S, '127.0.0.1';";
    parse_b3_032 => "bind S, '0.0.0.0';";
    parse_b3_033 => "setsockopt S, 1, 1, 1;";
    parse_b3_034 => "getsockopt S, 1, 1;";
    parse_b3_035 => "getpeername S;";
    parse_b3_036 => "getsockname S;";
    parse_b3_037 => "send S, 'x', 0;";
    parse_b3_038 => "recv S, $buf, 100, 0;";
    parse_b3_039 => "fork();";
    parse_b3_040 => "wait();";
    parse_b3_041 => "waitpid -1, 0;";
    parse_b3_042 => "pipe R, W;";
    parse_b3_043 => "open2 R, W, 'true';";
    parse_b3_044 => "qx(true);";
    parse_b3_045 => "`true`;";
    parse_b3_046 => "readline F;";
    parse_b3_047 => "getc F;";
    parse_b3_048 => "read F, $buf, 10;";
    parse_b3_049 => "sysread F, $buf, 10, 0;";
    parse_b3_050 => "my @pair = each %ENV;";
    parse_b3_051 => "no strict 'refs';";
    parse_b3_052 => "p 1;";
    parse_b3_053 => "dbmopen(%H, 'f', 0644);";
    parse_b3_054 => "dbmclose %H;";
    parse_b3_055 => "syscall(1, 1, 'x', 1);";
    parse_b3_056 => "my $z = 'a' cmp 'b';";
    parse_b3_057 => "my $z = 'a' ~~ 'b';";
    parse_b3_058 => "state $st = 1;";
    parse_b3_059 => "continue { 1; }";
    parse_b3_060 => "sub foo { 1; }";
}
