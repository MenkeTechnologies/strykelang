//! Parser accepts parallel-extension and regex-binding forms (one test per snippet).

fn p(src: &str) {
    perlrs::parse(src).unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"));
}

#[test]
fn accepts_pmap_block() {
    p("my @r = pmap { $_ * 2 } (1, 2, 3);");
}

#[test]
fn accepts_pmap_chunked_block() {
    p("my @r = pmap_chunked 2 { $_ * 2 } (1, 2, 3, 4);");
}

#[test]
fn accepts_pgrep_block() {
    p("my @r = pgrep { $_ > 0 } (1, -1, 2);");
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
fn accepts_fan_block() {
    p("fan 4 { my $i = $_; };");
}

#[test]
fn accepts_fan_zero() {
    p("fan 0 { 1 };");
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
fn accepts_preduce_block() {
    p("my $sum = preduce { $a + $b } (1, 2, 3);");
}

#[test]
fn accepts_preduce_with_array() {
    p("my @nums = (1, 2, 3); preduce { $a + $b } @nums;");
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
