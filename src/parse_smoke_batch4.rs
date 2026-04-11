//! Parse-only smoke tests (batch 4). Snippets that must parse.

use crate::parse;

macro_rules! parse_batch4 {
    ($($name:ident => $src:expr;)*) => {
        $(#[test] fn $name() { parse($src).expect("parse"); })*
    };
}

parse_batch4! {
    parse_b4_001 => "my $v = vec($x, 0, 8);";
    parse_b4_002 => "my $x = prototype &foo;";
    parse_b4_003 => "formline '@', @a;";
    parse_b4_004 => "my $r = readpipe 'true';";
    parse_b4_005 => "pipe R, W;";
    parse_b4_006 => "my $p = getppid();";
    parse_b4_007 => "my $g = getpgrp();";
    parse_b4_008 => "setpgrp;";
    parse_b4_009 => "my $e = getpriority 0, 0;";
    parse_b4_010 => "setpriority 0, 0, 0;";
    parse_b4_011 => "my $u = getrlimit 1;";
    parse_b4_012 => "ioctl F, 1, $buf;";
    parse_b4_013 => "fcntl F, 1, 0;";
    parse_b4_014 => "flock F, 2;";
    parse_b4_015 => "truncate 'f', 0;";
    parse_b4_016 => "utime 1, 1, 'f';";
    parse_b4_017 => "my @s = stat 'Cargo.toml';";
    parse_b4_018 => "my @s = lstat 'Cargo.toml';";
    parse_b4_019 => "chown 0, 0, 'f';";
    parse_b4_020 => "chmod 0644, 'f';";
    parse_b4_021 => "chroot '/';";
    parse_b4_022 => "rename 'a', 'b';";
    parse_b4_023 => "link 'a', 'b';";
    parse_b4_024 => "symlink 'a', 'b';";
    parse_b4_025 => "my $t = readlink 'f';";
    parse_b4_026 => "mkdir 'd', 0755;";
    parse_b4_027 => "rmdir 'd';";
    parse_b4_028 => "opendir D, '.';";
    parse_b4_029 => "readdir $d;";
    parse_b4_030 => "closedir $d;";
    parse_b4_031 => "rewinddir $d;";
    parse_b4_032 => "telldir $d;";
    parse_b4_033 => "seekdir $d, 0;";
    parse_b4_034 => "my $c = crypt 'pw', 'sa';";
    parse_b4_035 => "study $s;";
    parse_b4_036 => "my $p = pos $s;";
    parse_b4_037 => "pos $s = 0;";
    parse_b4_038 => "fc $s;";
    parse_b4_039 => "my $z = getc;";
    parse_b4_040 => "eof F;";
    parse_b4_041 => "binmode F;";
    parse_b4_042 => "select F;";
    parse_b4_043 => "select;";
    parse_b4_044 => "fileno F;";
    parse_b4_045 => "tell F;";
    parse_b4_046 => "seek F, 0, 0;";
    parse_b4_047 => "truncate F, 0;";
    parse_b4_048 => "syswrite F, 'x', 1;";
    parse_b4_049 => "print F 'x';";
    parse_b4_050 => "printf F '%d', 1;";
    parse_b4_051 => "my $n = syswrite F, $buf, 10;";
    parse_b4_052 => "my $fh; open $fh, '<', 'Cargo.toml';";
    parse_b4_053 => "open F, 'Cargo.toml';";
    parse_b4_054 => "open(HANDLE, '>', 'out.txt');";
    parse_b4_055 => "close F;";
    parse_b4_056 => "my $pid = fork;";
    parse_b4_057 => "wait;";
    parse_b4_058 => "waitpid -1, 0;";
    parse_b4_059 => "pipe R, W;";
    parse_b4_060 => "socket S, 1, 1, 0;";
    parse_b4_061 => "connect S, '127.0.0.1';";
    parse_b4_062 => "bind S, '0.0.0.0';";
    parse_b4_063 => "listen S, 5;";
    parse_b4_064 => "accept NS, S;";
    parse_b4_065 => "shutdown S, 2;";
    parse_b4_066 => "my $m = map { $_ * 2 } @a;";
    parse_b4_067 => "my @g = grep { $_ > 0 } @a;";
    parse_b4_068 => "sort { $a cmp $b } @a;";
    parse_b4_069 => "(0, @a) |> reduce { $a + $b };";
    parse_b4_070 => "my $t = threads->create(sub { 1 });";
}
