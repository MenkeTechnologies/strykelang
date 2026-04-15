//! Deconvert perlrs .pr syntax back to standard Perl .pl syntax.
//!
//! This is the inverse of [`crate::convert`]: takes a parsed perlrs program
//! and emits valid Perl 5 source code that can run under stock `perl`.
//!
//! Transformations applied:
//! - `fn` → `sub`
//! - `p` (say alias) → `say`
//! - Pipe chains and thread macros → nested function calls (preserved from parse)
//! - Adds trailing semicolons
//! - `#!/usr/bin/env perl` shebang prepended

use crate::ast::*;
use crate::deparse;

/// Options for the deconvert module.
#[derive(Debug, Clone, Default)]
pub struct DeconvertOptions {
    /// Custom delimiter for s///, tr///, m// patterns (e.g., '|', '#', '!').
    pub output_delim: Option<char>,
}

/// Convert a parsed perlrs program to standard Perl syntax.
pub fn deconvert_program(p: &Program) -> String {
    deconvert_program_with_options(p, &DeconvertOptions::default())
}

/// Convert a parsed perlrs program to standard Perl syntax with custom options.
pub fn deconvert_program_with_options(p: &Program, opts: &DeconvertOptions) -> String {
    let body = if let Some(delim) = opts.output_delim {
        deparse::deparse_block_with_delim(&p.statements, delim)
    } else {
        deparse::deparse_block(&p.statements)
    };
    format!(
        "#!/usr/bin/env perl\nuse v5.10;\nuse strict;\nuse warnings;\n\n{}\n",
        body
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn deconvert(code: &str) -> String {
        let p = parse(code).expect("parse failed");
        deconvert_program(&p)
    }

    #[test]
    fn deconvert_simple() {
        let out = deconvert("my $x = 1;");
        assert!(out.contains("#!/usr/bin/env perl"));
        assert!(out.contains("my $x = 1;"));
    }

    #[test]
    fn deconvert_fn_to_sub() {
        let out = deconvert("sub foo { 42 }");
        assert!(out.contains("sub foo"));
        assert!(out.contains("42"));
    }

    #[test]
    fn deconvert_say() {
        let out = deconvert("say 'hello';");
        assert!(out.contains("say"));
        assert!(out.contains("hello"));
    }

    #[test]
    fn deconvert_has_strict_warnings() {
        let out = deconvert("1;");
        assert!(out.contains("use strict;"));
        assert!(out.contains("use warnings;"));
    }

    #[test]
    fn deconvert_pipe_to_nested() {
        let out = deconvert("@a |> map { $_ * 2 } |> join \",\";");
        assert!(out.contains("join"));
        assert!(out.contains("map"));
        assert!(out.contains("@a"));
        assert!(!out.contains("|>"));
    }

    #[test]
    fn deconvert_thread_macro() {
        let out = deconvert("t $x uc lc;");
        assert!(out.contains("lc"));
        assert!(out.contains("uc"));
        assert!(out.contains("$x"));
        assert!(!out.contains(" t "));
    }
}
