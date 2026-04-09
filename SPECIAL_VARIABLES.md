# Perl special variables vs perlrs

This document audits **Perl 5’s “special” globals** against **perlrs** as implemented in the tree-walker / VM (`src/interpreter.rs`, `src/lexer.rs`, `src/vm.rs`, `src/scope.rs`). Full stock **perlvar** parity is not claimed; this file tracks what is wired, stubbed, or absent. **Any** scalar whose name starts with `^` (including `${^NAME}` from the lexer) is routed through [`get_special_var`](src/interpreter.rs); unknown names read from `Interpreter.special_caret_scalars` (default `undef`) and can be assigned for compatibility. Documented names are pre-seeded from [`special_vars::PERL5_DOCUMENTED_CARET_NAMES`](src/special_vars.rs).

Legend: **Yes** = behavior matches intent for typical use; **Partial** = exists but semantics differ; **No** = not implemented or wrong tokenization.

---

## Implemented with dedicated handling

| Perl | Role | perlrs |
|------|------|--------|
| `__FILE__` / `__LINE__` | Compile-time literals | `ExprKind::MagicConst` — `__FILE__` → `Interpreter::file` (driver sets `-e` or script path); `__LINE__` → lexer line (1-based). VM bytecode uses `Compiler::source_file` (wired from `Interpreter::file` in `try_vm_execute`). |
| `$_` | Default topic | Ordinary scalar `$_` in scope; set by `map`/`grep`/many iterators, `given`, `readline`, etc. |
| `$.` | Input line number | `get_special_var(".")`: after a `readline`/`<>`, the line count for `last_readline_handle` (`handle_line_numbers`); otherwise `line_number` (e.g. `-n`/`-p` `process_line`). |
| `$/` | Input record separator | `irs` field; get/set via `get_special_var` / `set_special_var` for `"/"`. |
| `$,` | Output field separator | `ofs` field; `","` in special get/set. |
| `$"` | List separator (array in `"..."`) | `Interpreter.list_separator`; used when interpolating `@array` into strings (`src/interpreter.rs`). |
| `$\` | Output record separator | `ors` field; `"\\"` in special get/set. |
| `$~` | Current format name | Ordinary scalar `~` (default `"STDOUT"` in `Interpreter::new`); `write` resolves `package::NAME` in `format_templates` (`src/interpreter.rs`). |
| `$!` | OS error (errno) | **`PerlValue::ErrnoDual`**: numeric `errno_code` + string `errno` (`get_special_var("!")`). Numeric/string contexts use the dualvar; I/O failures call `apply_io_error_to_errno`. Assignment via `set_special_var("!")` takes a numeric value, updates `errno_code`, and sets the message from `std::io::Error::from_raw_os_error` (Perl-like). |
| `$@` | Eval error | Reads use `eval_error` (`get_special_var("@")`). Assignment via `set_special_var("@")` updates `eval_error` (plain string; not a dualvar like `$!`). |
| `$0` | Program name | `program_name`; `"0"` in special get/set. |
| `$$` | Process ID | `get_special_var("$$")` → `std::process::id()`. |
| `$1`…`$n` | Capture groups | After a successful match, `apply_regex_captures` sets `scope` scalars `"1"`…`"n"` (`src/interpreter.rs`). |
| `${^MATCH}` / `${^PREMATCH}` / `${^POSTMATCH}` | Match text / before / after | Same data as `$&`, `` $` ``, `$'` — read via `get_special_var("^MATCH")` etc. on the interpreter after `apply_regex_captures` (`src/interpreter.rs`). |
| `${^LAST_SUBMATCH_RESULT}` | Last bracket `$+` | Same as `$+` / `last_paren_match`; exposed as `get_special_var("^LAST_SUBMATCH_RESULT")`. |
| `@-` / `@+` | Match start/end offsets | After a successful match, `apply_regex_captures` sets arrays `"-` and `"+"` (whole match at index 0, then groups; `-1` for unused groups). |
| `%+` | Named captures | `scope.set_hash("+", …)` from regex named groups. |
| `@{^CAPTURE}` | Subpattern captures from last match | Array `^CAPTURE` in scope: capture groups `1..n-1` after `apply_regex_captures` (Perl excludes the full match). Lexer: `@^NAME` → `ArrayVar("^NAME")`. |
| `@ARGV` | Script arguments | Declared in `Interpreter::new`; populated by `main` driver (`src/main.rs`). |
| `$ARGV` | Current filename for `<>` | `argv_current_file`; set when `<>` opens each `@ARGV` file; empty when reading stdin or before first file. |
| `<>` | Read lines | Iterate `@ARGV` files in order (then undef); if `@ARGV` is empty, stdin. |
| `@INC` | Library path | Array of search dirs; `%INC` used for loaded paths in `require`. |
| `%INC` | Loaded modules | Hash entries set by `require`/`use` (see `require_execute`). |
| `%ENV` | Environment | Hash in scope; filled from `std::env::vars()` on first access (`Interpreter::materialize_env_if_needed`) to reduce cold-start cost. |
| `%SIG` | Signal handlers | Hash in scope. On **Unix**, `SIGINT` / `SIGTERM` / `SIGALRM` / `SIGCHLD` are registered (`signal_hook`); [`perl_signal::poll`](src/perl_signal.rs) runs **between statements** and invokes code refs (`IGNORE` / `DEFAULT` are no-ops). Non-Unix: no OS delivery. |
| `$]` | Numeric language version | `get_special_var("]")` → `perl_bracket_version()` (emulated Perl 5.x.y level; see `perl_bracket_version` in `src/interpreter.rs`). |
| `$;` | Subscript separator | `subscript_sep` field; default `\x1c` (Perl `\034`). |
| `$^I` | In-place edit extension | `inplace_edit` string; lexer reads `$^` + letter as variable name `^I`. The **`pe`/`perlrs` driver** sets this from **`-i`** / **`-i.ext`** (backup suffix) and applies in-place rewrites for **`-n`/`-p`** over **`@ARGV`** files. |
| `$^D` | Debug flags | `debug_flags` (`i64`). |
| `$^P` | Debugger flags | `perl_debug_flags` (`i64`). |
| `$^S` | Exception state (in eval) | `eval_nesting > 0` while `eval` runs (tree-walker and VM `eval` / `evalblock`). |
| `$^W` | Warnings | `warnings` boolean (`true` → `1`). |
| `$^O` | OS name | `perl_osname()` maps `std::env::consts::OS` toward Perl names (`linux`, `darwin`, `MSWin32`, …). |
| `$^T` | Script start time | `Interpreter.script_start_time` (seconds since Unix epoch, set in `Interpreter::new`). |
| `$^V` | Version string | `v{CARGO_PKG_VERSION}` (e.g. `v0.1.12`); not a full Perl `version` object. |
| `$^E` | Extended OS error | `std::io::Error::last_os_error().to_string()` (not Windows `GetLastError` semantics). |
| `$^H` | Compile-time hints | `compile_hints` (`i64`); read/write via `get_special_var` / `set_special_var`. |
| `${^WARNING_BITS}` | Warnings bitmask | `warning_bits` (`i64`); read/write via `get_special_var` / `set_special_var`. |
| `${^GLOBAL_PHASE}` | Interpreter phase | `global_phase` string (default `RUN`); read-only assignment in `set_special_var`. |
| `$+` | Last bracket match | `last_paren_match`; also `scope` `"+"` after regex; `get_special_var("+")`. |
| `$*` | Multiline (deprecated) | `multiline_match`: when true, `compile_regex` prepends `(?s)` so `.` matches newlines (Rust `regex` dotall). |
| `$%` / `$=` / `$-` / `$:` | Format page / lines / remainder / break chars | `format_page_number`, `format_lines_per_page`, `format_lines_left`, `format_line_break_chars`. |
| `$^` | Top-of-form format name | `format_top_name` (scalar name `"^"`). |
| `$^A` | Format accumulator | `accumulator_format`. |
| `$^C` | Pending interrupt | On Unix, `SIGINT` sets a latch before the `%SIG` handler runs; `get_special_var("^C")` returns `1` once then clears (otherwise `0`). |
| `$^F` | Max system FD | `max_system_fd` (default `2`). |
| `$^L` | Form feed | `formfeed_string` (default `\f`). |
| `$^M` | Emergency memory pool | `emergency_memory` string (no native pool in perlrs). |
| `$^N` | Last opened named capture | `last_subpattern_name` after `apply_regex_captures`. |
| `$^X` | Executable path | `executable_path` from `std::env::current_exe()` at interpreter init. |
| `$INC` | `@INC` hook index | `inc_hook_index` (Perl 5.37+ hook traversal; hooks not fully implemented). |
| Other `${^Name}` | perlvar extras | `special_caret_scalars["^Name"]` or `undef` if unset; assign stores in the map unless the name is read-only in `set_special_var`. |
| `$<` / `$>` | Real/effective uid | On Unix `libc::getuid` / `geteuid`; on non-Unix `0`. |
| `$(` / `$)` | Real/effective gid sets | On Unix, space-separated group id list (`getgroups` + primary gid), matching Perl’s string form; on non-Unix empty string. |
| `${^MATCH}` / `${^PREMATCH}` / `${^POSTMATCH}` | Regexp spellings | Same as `$&` / `` $` `` / `$'` data on `Interpreter` (`last_match`, `prematch`, `postmatch`). |
| `__PACKAGE__` | Current package | Scalar in scope; `package` statements update it. |
| `wantarray` | List/scalar/void context | `WantarrayCtx` on interpreter; `ExprKind::Wantarray` / `BuiltinId::Wantarray`. |

---

## Partially implemented or different from Perl 5

| Perl | Issue |
|------|-------|
| `$@` | Eval error is **string** only (not dualvar). |
| `$.` | Simplified per-handle tracking (`last_readline_handle` + `handle_line_numbers`) vs Perl’s tied IO layer. |
| `$1`…`$n`, `%+`, `@-`, `@+` | Driven by the **Rust `regex` crate**; Perl’s regexp engine differs (lookbehind, backtracking, etc.). |
| `@_` | Works as the **subroutine argument array** in user subs; not fully identical to Perl’s XS calling conventions. |
| `pos $_` | Supported with `regex_pos` map; edge cases may differ from Perl. |
| `%SIG` | On Unix, delivery is **between statements** only (not mid-op); handlers see `$^C==1` on the first read after `SIGINT` if the latch was set; see [`perl_signal`](src/perl_signal.rs). |
| `$^I` | The **`pe`/`perlrs` driver** applies **`-i`** / **`-i.bak`** for **`-n`/`-p`** over **`@ARGV`**; value is stored for compatibility with other code paths. |
| `$^V` | String form only (`v…` from crate version); not a Perl `version` object. |
| `$^E` | Uses `std::io::Error::last_os_error()`, not Perl’s per-platform extended error. |
| `${^GLOBAL_PHASE}` | Single string field; not full Perl phase transitions (`BEGIN`/`CHECK`/…). |

---

## Lexer vs semantics

Single-character names after `$` are accepted (`src/lexer.rs` `read_variable_name`), including `&` `` ` `` `'` `+` `*` `?` `|` etc. Scalars handled in `get_special_var` / `set_special_var` (including any **`^`…** name) use interpreter fields or `special_caret_scalars`; other names resolve as **ordinary scalars** in scope. Match-related scalars (`$&`, `` $` ``, `$'`, `$+`, …) are updated by `apply_regex_captures` into both dedicated fields and the scope where applicable. **`$?`** (child wait status) and **`$|`** (stdout autoflush after `print` / `printf`) use `get_special_var` / `set_special_var` and `Interpreter::record_child_exit_status`.

**`$^` + letter:** The lexer reads **`^` plus one alphabetic character** as names like `^I`, `^O`, `^M` (see `read_variable_name`).

**`${^NAME}` (brace form):** `{` … `}` after `$` is read as the inner name (e.g. **`^MATCH`**, **`^UNICODE`**). Any name starting with `^` is treated as special; unknown long names use `special_caret_scalars` / `undef`.

**`@^NAME`:** After `@`, a **`^`** plus identifier yields `ArrayVar("^NAME")` (e.g. **`@{^CAPTURE}`**).

---

## Not implemented (common Perl specials)

| Category | Examples |
|----------|----------|
| **Match / regexp** | Stash-backed `$&` / `$1` / `` $` `` / `$'` / `$+` and Rust `regex` still differ from Perl’s regexp engine; `${^…}` beyond dedicated fields are stubs in `special_caret_scalars`. |
| **Process / status** | `$PROCESS_ID` aliases. (`$?` is set after `system`, `capture`, and `close` on pipe children; POSIX-style packed status.) |
| **Perlio / globs** | Many handle-related specials beyond what IO builtins use. |
| **English.pm** | No `English` module tying long names to these variables. |

---

## Short list (what’s still missing)

**Still commonly missing vs stock Perl 5:** full **`$@`** dualvar (if Perl exposes one for your platform); **`English`**; full **`$^V`** as a version object; **`${^GLOBAL_PHASE}`** lifecycle matching Perl. **`exists $href->{key}`** / **`delete $href->{key}`** (hash references and blessed hash-like objects) are implemented; other exotic **`exists`/`delete`** targets may still differ from Perl 5.

| Area | Perl | Notes |
|------|------|--------|
| Dualvar | `$!`, `$@` | **`$!`** is errno dualvar; **`$@`** is string-only in perlrs. |
| Signals | `%SIG` | Unix: **`INT`**/**`TERM`**/**`ALRM`**/**`CHLD`** into subs between statements; non-Unix: stubs. |
| Aliases | `English` | No long-name aliases module. |

---

## Maintenance

When adding I/O, regex, or `eval` behavior, update this file if new globals become meaningful or if `get_special_var` / `set_special_var` change. When adding documented `${^NAME}` entries from perl 5, consider extending [`special_vars::PERL5_DOCUMENTED_CARET_NAMES`](src/special_vars.rs).
