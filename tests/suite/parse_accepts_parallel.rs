//! Parser accepts parallel-extension and regex-binding forms (one test per snippet).

fn p(src: &str) {
    perlrs::parse(src).unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"));
}

#[test]
fn accepts_pmap_block() {
    p("my @r = pmap { $_ * 2 } (1, 2, 3);");
}

#[test]
fn accepts_pmap_empty_list() {
    p("my @r = pmap { $_ } ();");
}

#[test]
fn accepts_pgrep_empty_list() {
    p("my @r = pgrep { 1 } ();");
}

#[test]
fn accepts_pmap_progress_option() {
    p("my @r = pmap { $_ * 2 } (1, 2, 3), progress => 0;");
    p("my @s = pmap { $_ } (1), progress => 1;");
}

#[test]
fn accepts_pmap_chunked_block() {
    p("my @r = pmap_chunked 2 { $_ * 2 } (1, 2, 3, 4);");
}

#[test]
fn accepts_pmap_chunked_progress() {
    p("my @r = pmap_chunked 2 { $_ * 2 } (1, 2, 3, 4), progress => 1;");
}

#[test]
fn accepts_pmap_chunked_empty_list() {
    p("my @r = pmap_chunked 3 { $_ } ();");
}

#[test]
fn accepts_async_await() {
    p("my $t = async { 1 }; my $x = await($t);");
}

#[test]
fn accepts_pgrep_block() {
    p("my @r = pgrep { $_ > 0 } (1, -1, 2);");
}

#[test]
fn accepts_pfor_psort_pcache_par_lines_progress() {
    p("pfor { 1 } (1), progress => 1;");
    p("my @s = psort { $a <=> $b } (3, 1, 2), progress => 1;");
    p("my @t = psort (3, 1, 2), progress => 1;");
    p("my @u = pcache { $_ } (1, 2), progress => 1;");
    p(r#"par_lines "x.txt", sub { 1 }, progress => 1;"#);
}

#[test]
fn accepts_parallel_for_loop() {
    p("parallel_for (1, 2, 3) { my $x = $_; };");
}

#[test]
fn accepts_psort_with_default() {
    p("my @r = psort (3, 1, 2);");
}

#[test]
fn accepts_psort_with_block() {
    p("my @r = psort { $a <=> $b } (2, 1);");
}

#[test]
fn accepts_psort_empty_list() {
    p("my @r = psort ();");
}

#[test]
fn accepts_fan_block() {
    p("fan 4 { my $i = $_; };");
}

#[test]
fn accepts_fan_block_default_count() {
    p("fan { my $i = $_; };");
}

#[test]
fn accepts_fan_progress() {
    p("fan 2 { 1 }, progress => 1;");
    p("fan { 1 }, progress => 0;");
}

#[test]
fn accepts_fan_cap_block() {
    p("fan_cap 4 { my $i = $_; };");
    p("fan_cap { $_ };");
    p("fan_cap 2 { 1 }, progress => 0;");
}

#[test]
fn accepts_glob_par_progress() {
    p(r#"glob_par "src/*.rs", progress => 0;"#);
}

#[test]
fn accepts_fan_zero() {
    p("fan 0 { 1 };");
}

#[test]
fn accepts_fan_bareword_stmt_in_block() {
    p("fan 3 { worker };");
}

#[test]
fn accepts_pfor_bareword_stmt_in_block() {
    p("pfor { process_item } (1, 2);");
}

#[test]
fn accepts_pwatch_glob_and_sub() {
    p(r#"pwatch "/var/log/*.log", sub { say $_ };"#);
}

#[test]
fn accepts_watch_literal_and_block() {
    p(r#"watch "/tmp/x", { say };"#);
}

#[test]
fn accepts_trace_block() {
    p("trace { fan 2 { mysync $c = 0; $c++ } };");
}

#[test]
fn accepts_timer_block() {
    p("my $ms = timer { 1 + 1 };");
}

#[test]
fn accepts_pipeline_chain() {
    p("my @r = pipeline(1, 2)->map(sub { $_ * 2 })->collect();");
}

#[test]
fn accepts_pipeline_qualified_method() {
    p("my @r = pipeline(1)->P::triple->collect();");
}

#[test]
fn accepts_preduce_preduce_init_pmap_reduce_progress() {
    p("my $a = preduce { $a + $b } (1, 2, 3), progress => 1;");
    p("my $b = preduce_init 0, { $a + $b } (1, 2), progress => 0;");
    p("my $c = pmap_reduce { $_ * 2 } { $a + $b } (1, 2, 3), progress => 1;");
}

#[test]
fn accepts_preduce_block() {
    p("my $sum = preduce { $a + $b } (1, 2, 3);");
}

#[test]
fn accepts_preduce_with_array() {
    p("my @nums = (1, 2, 3); preduce { $a + $b } @nums;");
}

#[test]
fn accepts_preduce_init() {
    p("my @words = qw(a b a); my $h = preduce_init {}, { $a->{$b}++; $a } @words;");
}

#[test]
fn accepts_binding_match_scalar() {
    p(r#"my $s = "ab"; $s =~ /a/;"#);
}

#[test]
fn accepts_binding_match_underscore() {
    p(r#"$_ = "xy"; $_ =~ /x/;"#);
}

#[test]
fn accepts_binding_substitute_global() {
    p(r#"my $t = "aaa"; $t =~ s/a/b/g;"#);
}

#[test]
fn accepts_binding_tr_slash() {
    p(r#"my $u = "abc"; $u =~ tr/a-z/A-Z/;"#);
}

#[test]
fn accepts_negated_binding_match() {
    p(r#"my $v = "nope"; $v !~ /yes/;"#);
}

#[test]
fn accepts_list_match_operator() {
    p(r#"my @m = ($str =~ /./g);"#);
}

#[test]
fn accepts_regex_as_expression_statement() {
    p("/pattern/;");
}

#[test]
fn accepts_substitute_expression_statement() {
    p("s/foo/bar/;");
}

#[test]
fn accepts_qr_scalar_value() {
    p(r#"my $r = qr/pat/;"#);
}

#[test]
fn accepts_match_slash_after_scalar_binding_line() {
    p(r#"my $txt = "x"; $txt =~ /pat/;"#);
}

#[test]
fn accepts_split_on_regex_binding() {
    p(r#"my @f = split /\s+/, $line;"#);
}

#[test]
fn accepts_join_after_parallel_map() {
    p(r#"my $j = join ",", pmap { $_ } (1, 2);"#);
}

#[test]
fn accepts_nested_pmap_in_expression() {
    p("scalar pmap { $_ } pmap { $_ * 2 } (1);");
}

#[test]
fn accepts_eof_handle_readline() {
    p("eof FH;");
}

#[test]
fn accepts_tell_on_handle() {
    p("tell STDOUT;");
}

#[test]
fn accepts_seek_triple_form() {
    p("seek FH, 0, 0;");
}

#[test]
fn accepts_binmode_handle() {
    p("binmode STDOUT, ':utf8';");
}

#[test]
fn accepts_select_four_arg() {
    p("select RB, WB, EB, 0.5;");
}
