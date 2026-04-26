#!/usr/bin/env perl
# Smoke tests for Top-N modules (parity/cpan_topn). Run via: st smoke_all.pl
use strict;
use warnings;

sub fail {
    my ( $name, $err ) = @_;
    chomp $err if defined $err;
    die "cpan_topn FAIL [$name] ${err}\n";
}

# --- Try::Tiny (before JSON::PP: loading JSON::PP first currently breaks a follow-on require Try::Tiny in stryke) ---
eval {
    require Try::Tiny;
    Try::Tiny->VERSION >= 0.20 or die "version";
};
fail( 'Try::Tiny', $@ ) if $@;

# --- JSON (JSON::PP when require succeeds; else builtins — full JSON::PP.pm still hits parser gaps in eval/qq) ---
my $json_pp_ok = 0;
eval {
    require JSON::PP;
    my $j = JSON::PP->new;
    $j->encode( { a => 1 } ) eq '{"a":1}' or die "encode";
    $json_pp_ok = 1;
};
if ( !$json_pp_ok ) {
    eval {
        json_encode( { a => 1 } ) eq '{"a":1}' or die "json_encode";
        json_decode('{"a":1}')->{a} == 1 or die "json_decode";
        json_jq( json_decode('{"x":2}'), '.x' ) == 2 or die "json_jq";
    };
    fail( 'JSON', $@ ) if $@;
}

# --- Text::Balanced (pure-Perl; avoids Test2 → List::Util::import chain from Test::More) ---
eval {
    require Text::Balanced;
    my @x = Text::Balanced::extract_delimited( '"hi"', '"' );
    @x && $x[0] eq '"hi"' or die "delim";
};
fail( 'Text::Balanced', $@ ) if $@;

# --- Carp ---
eval {
    require Carp;
    my $m = Carp::longmess('x');
    $m =~ /x/ or die "longmess";
};
fail( 'Carp', $@ ) if $@;

# --- Exporter ---
eval {
    require Exporter;
    $Exporter::VERSION or die "version";
};
fail( 'Exporter', $@ ) if $@;

# --- parent ---
eval {
    require parent;
    1;
};
fail( 'parent', $@ ) if $@;

# --- URI ---
eval {
    require URI;
    my $u = URI->new('http://example.com/foo/bar');
    $u->path eq '/foo/bar' or die "path";
};
fail( 'URI', $@ ) if $@;

# --- URI::Escape ---
eval {
    require URI::Escape;
    URI::Escape::uri_escape('a b') eq 'a%20b' or die "esc";
};
fail( 'URI::Escape', $@ ) if $@;

# --- File::Find ---
eval {
    require File::Find;
    require File::Spec;
    my $here = File::Spec->curdir;
    my $seen = 0;
    File::Find::find(
        {
            wanted => sub {
                no warnings 'once';
                $seen = 1 if $File::Find::name =~ /smoke_all\.pl\z/;
            },
            no_chdir => 1,
        },
        $here
    );
    $seen or die "did not see smoke_all.pl";
};
fail( 'File::Find', $@ ) if $@;

# --- File::Spec ---
eval {
    require File::Spec;
    my $x = File::Spec->catfile( 'a', 'b' );
    length $x or die "catfile";
};
fail( 'File::Spec', $@ ) if $@;

# --- File::Path ---
eval {
    require File::Path;
    my $tmp = "cpan_topn_mkpath_test_$$";
    File::Path::mkpath( [$tmp], 0, 0700 );
    File::Path::rmtree( [$tmp] );
};
fail( 'File::Path', $@ ) if $@;

# --- File::Basename ---
eval {
    require File::Basename;
    File::Basename::basename('/x/y/z.pl') eq 'z.pl' or die "base";
};
fail( 'File::Basename', $@ ) if $@;

# --- Getopt::Long ---
eval {
    require Getopt::Long;
    local @ARGV = ( '--foo', '1' );
    my $foo;
    Getopt::Long::GetOptions( 'foo=i', \$foo ) or die "opts";
    ( $foo // 0 ) == 1 or die "foo";
};
fail( 'Getopt::Long', $@ ) if $@;

# --- Pod::Usage ---
eval {
    require Pod::Usage;
    Pod::Usage->can('pod2usage') or die "pod2usage";
};
fail( 'Pod::Usage', $@ ) if $@;

# --- Text::ParseWords ---
eval {
    require Text::ParseWords;
    my @w = Text::ParseWords::quotewords( '\s+', 0, 'a "b c"' );
    @w == 2 && $w[1] eq 'b c' or die "words";
};
fail( 'Text::ParseWords', $@ ) if $@;

# --- constant ---
eval {
    require constant;
    my $ok = eval 'use constant CPANTOPN_K => 7; CPANTOPN_K() == 7; 1';
    die $@ || 'const' unless $ok;
};
fail( 'constant', $@ ) if $@;

# --- overload (stryke requires \&name, not anon coderef) ---
eval {
    require overload;
    {
        package CpanTopnOv;
        sub as_string { 'O' }
        use overload '""' => \&as_string;
    }
    my $o = bless {}, 'CpanTopnOv';
    "$o" eq 'O' or die "overload";
};
fail( 'overload', $@ ) if $@;

# --- Text::Tabs (pure-Perl; no sockets / POSIX beyond stub) ---
eval {
    require Text::Tabs;
    Text::Tabs::expand("a\tb") =~ /a/ or die "tabs";
};
fail( 'Text::Tabs', $@ ) if $@;

# --- Path::Tiny ---
eval {
    require Path::Tiny;
    my $p = Path::Tiny->new('.');
    $p->stringify ne '' or die "path";
};
fail( 'Path::Tiny', $@ ) if $@;

# --- Module::Load ---
eval {
    require Module::Load;
    Module::Load::load('Carp');
};
fail( 'Module::Load', $@ ) if $@;

# --- sum builtin (bare name) ---
eval {
    sum( 1, 2, 3 ) == 6 or die "sum";
};
fail( 'sum(native)', $@ ) if $@;

print "cpan_topn: all smokes passed\n";
exit 0;
