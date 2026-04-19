# Perl special variables vs stryke

This document audits **Perl 5’s “special” globals** against **stryke** as implemented in the tree-walker / VM (`src/interpreter.rs`, `src/lexer.rs`, `src/vm.rs`, `src/scope.rs`). Full stock **perlvar** parity is not claimed; this file tracks what is wired, stubbed, or absent. **Any** scalar whose name starts with `^` (including `${^NAME}` from the lexer) is routed through [`get_special_var`](src/interpreter.rs); unknown names read from `Interpreter.special_caret_scalars` (default `undef`) and can be assigned for compatibility. Documented `${^NAME}` scalars from Perl 5 are pre-seeded (see [`special_vars::PERL5_DOCUMENTED_CARET_NAMES`](src/special_vars.rs)) so `defined ${^NAME}` works without a prior assignment; a few have dedicated semantics (e.g. `${^UNICODE}`, `${^TAINT}`). **`${^OPEN}`** is **`1`** after **`use open`** with `:utf8` / `:std` / `:encoding(UTF-8)` (stryke enables UTF-8 lossy readline decoding); **`0`** otherwise — not Perl’s full I/O layer bitmask.

Legend: **Yes** = behavior matches intent for typical use; **Partial** = exists but semantics differ; **No** = not implemented or wrong tokenization.

**Double-quoted / `qq` interpolation** — Implemented in [`parse_interpolated_string`](src/parser.rs) (not the lexer): Perl allows whitespace between `$` and the variable name; `$$` is the PID only when the two `$` are adjacent (otherwise `$` + `$name` / `$` + `$n`); `$^FOO` uses a `^`-prefixed name; one-character punctuation specials follow [`Interpreter::is_special_scalar_name_for_get`](src/interpreter.rs) (plus `` $` `` / `$'` for match text); `"@+"` / `"@-"` after a match interpolate those arrays; a `$` with only whitespace before the closing quote is a parse error (Perl’s “Final $ should be…”).

---

## Implemented with dedicated handling

| Perl | Role | stryke |
|------|------|--------|
| `__FILE__` / `__LINE__` | Compile-time literals | `ExprKind::MagicConst` — `__FILE__` → `Interpreter::file` (driver sets `-e` or script path); `__LINE__` → lexer line (1-based). VM bytecode uses `Compiler::source_file` (wired from `Interpreter::file` in `try_vm_execute`). |
| `$_` | Default topic | Ordinary scalar `$_` in scope; set by `map`/`grep`/many iterators, `given`, `readline`, etc. |
| `$.` | Input line number | `get_special_var(".")`: **undef** until the first `readline`/`<>` line (and `line_number` is still 0), matching Perl; after a read, the line count for `last_readline_handle` (`handle_line_numbers`); when no handle-specific read yet but `-n`/`-p` ran, `line_number` from `process_line`. **`set_special_var(".")`** (assignment to `$.`) sets the per-handle count when `last_readline_handle` is set, else `line_number` (Perl `perlvar`). Parity: [`parity/cases/012_dollar_dot_assign.pl`](parity/cases/012_dollar_dot_assign.pl). |
| `$/` | Input record separator | `irs` field; get/set via `get_special_var` / `set_special_var` for `"/"`. |
| `$,` | Output field separator | `ofs` field; `","` in special get/set. |
| `$"` | List separator (array in `"..."`) | `Interpreter.list_separator`; used when interpolating `@array` into strings (`src/interpreter.rs`). |
| `$\` | Output record separator | `ors` field; `"\\"` in special get/set. |
| `$~` | Current format name | Ordinary scalar `~` (default `"STDOUT"` in `Interpreter::new`); `write` resolves `package::NAME` in `format_templates` (`src/interpreter.rs`). |
| `$!` | OS error (errno) | **`PerlValue::errno_dual`**: numeric `errno_code` + string `errno` (`get_special_var("!")`). I/O failures call `apply_io_error_to_errno`. Assignment via `set_special_var("!")` takes a numeric value, updates `errno_code`, and sets the message from `std::io::Error::from_raw_os_error` (Perl-like). |
| `$@` | Eval error | Dualvar like **`$!`**: `get_special_var("@")` → `PerlValue::errno_dual(eval_error_code, eval_error)` (`eval_error` / `eval_error_code` on [`Interpreter`](src/interpreter.rs)). **`die`** (and **`warn`**) append **` at FILE line N.`** when the message does not end with newline, matching Perl 5’s **`$@`** text. Typical failures set `eval_error_code` to **`1`** with the message string; `set_special_var("@")` accepts a dualvar or derives code from `to_int` / `1` when the message is non-empty. |
| `$0` | Program name | `program_name`; `"0"` in special get/set. |
| `$$` | Process ID | `get_special_var("$$")` → `std::process::id()`. Double-quoted interpolation is handled in [`parse_interpolated_string`](src/parser.rs) (Perl rules: `$$` is PID when the second `$` is not followed by a digit or word character; otherwise it is `$` + `$name` / `$` + `$n`). |
| `$1`…`$n` | Capture groups | After a successful match, `apply_regex_captures` sets `scope` scalars `"1"`…`"n"` (`src/interpreter.rs`). |
| `${^MATCH}` / `${^PREMATCH}` / `${^POSTMATCH}` | Match text / before / after | Same data as `$&`, `` $` ``, `$'` — read via `get_special_var("^MATCH")` etc. on the interpreter after `apply_regex_captures` (`src/interpreter.rs`). |
| `${^LAST_SUBMATCH_RESULT}` | Last bracket `$+` | Same as `$+` / `last_paren_match`; exposed as `get_special_var("^LAST_SUBMATCH_RESULT")`. |
| `@-` / `@+` | Match start/end offsets | After a successful match, `apply_regex_captures` sets arrays `"-` and `"+"` (whole match at index 0, then groups; `-1` for unused groups). |
| `%+` | Named captures | `scope.set_hash("+", …)` from regex named groups. |
| `@{^CAPTURE}` / `@{^CAPTURE_ALL}` | Subpattern captures from last match | Arrays `^CAPTURE` and `^CAPTURE_ALL` in scope (same data after `apply_regex_captures`; Perl’s `CAPTURE_ALL` can differ for `/g` — not fully modeled). Lexer: `@^NAME` → `ArrayVar("^NAME")`. |
| `@ARGV` | Script arguments | Declared in `Interpreter::new`; populated by `main` driver (`src/main.rs`). |
| `$ARGV` | Current filename for `<>` | `argv_current_file`; set when `<>` opens each `@ARGV` file; empty when reading stdin or before first file. |
| `<>` | Read lines | Iterate `@ARGV` files in order (then undef); if `@ARGV` is empty, stdin. |
| `@INC` | Library path | Array of search dirs; `%INC` used for loaded paths in `require`. |
| `%INC` | Loaded modules | Hash entries set by `require`/`use` (see `require_execute`). |
| `%{^HOOK}` | `require` hooks (Perl 5.37+) | Hash `^HOOK` pre-declared (empty). Coderefs `require__before` / `require__after` are invoked from [`Interpreter::require_execute`](src/interpreter.rs) (`invoke_require_hook`) around each successful file load. Lexer: `%^NAME` → `HashVar("^NAME")`. |
| `%ENV` | Environment | Hash in scope; filled from `std::env::vars()` on first access (`Interpreter::materialize_env_if_needed`) to reduce cold-start cost. |
| `%SIG` | Signal handlers | Hash in scope. On **Unix**, `SIGINT` / `SIGTERM` / `SIGALRM` / `SIGCHLD` are registered (`signal_hook`); [`perl_signal::poll`](src/perl_signal.rs) runs **before each tree-walker statement** (`exec_statement_inner`) and **before each VM opcode** (and once before the linear JIT fast path). Invokes code refs (`IGNORE` / `DEFAULT` are no-ops). Non-Unix: no OS delivery. |
| `$]` | Numeric language version | `get_special_var("]")` → `perl_bracket_version()` (emulated Perl 5.x.y level; see `perl_bracket_version` in `src/interpreter.rs`). |
| `$;` | Subscript separator | `subscript_sep` field; default `\x1c` (Perl `\034`). |
| `$^I` | In-place edit extension | `inplace_edit` string; lexer reads `$^` + letter as variable name `^I`. The **`fo`/`stryke` driver** sets this from **`-i`** / **`-i.ext`** (backup suffix) and applies in-place rewrites for **`-n`/`-p`** over **`@ARGV`** files. |
| `$^D` | Debug flags | `debug_flags` (`i64`). |
| `$^P` | Debugger flags | `perl_debug_flags` (`i64`). |
| `$^S` | Exception state (in eval) | `eval_nesting > 0` while `eval` runs (tree-walker and VM `eval` / `evalblock`). |
| `$^W` | Warnings | `warnings` boolean (`true` → `1`). |
| `$^O` | OS name | `perl_osname()` maps `std::env::consts::OS` toward Perl names (`linux`, `darwin`, `MSWin32`, …). |
| `$^T` | Script start time | `Interpreter.script_start_time` (seconds since Unix epoch, set in `Interpreter::new`). |
| `$^V` | Version string | `v{CARGO_PKG_VERSION}` (from `Cargo.toml` at build time); not a full Perl `version` object. |
| `$^E` | Extended OS error | `std::io::Error::last_os_error().to_string()` (not Windows `GetLastError` semantics). |
| `$^H` | Compile-time hints | `compile_hints` (`i64`); read/write via `get_special_var` / `set_special_var`. |
| `${^WARNING_BITS}` | Warnings bitmask | `warning_bits` (`i64`); read/write via `get_special_var` / `set_special_var`. |
| `${^GLOBAL_PHASE}` | Interpreter phase | `global_phase` string: tree-walker [`execute_tree`](src/interpreter.rs) and bytecode VM ([`Op::SetGlobalPhase`](src/bytecode.rs) from [`compile_program`](src/compiler.rs)) set **`START`** during **`BEGIN`** and (like Perl 5) still **`START`** during **`UNITCHECK`** blocks, then **`CHECK`** / **`INIT`** while those blocks run, **`RUN`** during the main program, **`END`** during **`END`**. Read-only in `set_special_var`. No **`DESTRUCT`** yet. |
| `$+` | Last bracket match | `last_paren_match`; also `scope` `"+"` after regex; `get_special_var("+")`. |
| `$*` | Multiline (deprecated) | `multiline_match`: when true, `compile_regex` prepends `(?s)` so `.` matches newlines (Rust `regex` dotall). |
| `$%` / `$=` / `$-` / `$:` | Format page / lines / remainder / break chars | `format_page_number`, `format_lines_per_page`, `format_lines_left`, `format_line_break_chars`. |
| `$^` | Top-of-form format name | `format_top_name` (scalar name `"^"`). |
| `$^A` | Format accumulator | `accumulator_format`. |
| `$^C` | Pending interrupt | On Unix, `SIGINT` sets a latch before the `%SIG` handler runs; `get_special_var("^C")` returns `1` once then clears (otherwise `0`). |
| `$^F` | Max system FD | `max_system_fd` (default `2`). |
| `$^L` | Form feed | `formfeed_string` (default `\f`). |
| `$^M` | Emergency memory pool | `emergency_memory` string (no native pool in stryke). |
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
| `$@` | Same heap dualvar as **`$!`** (`ErrnoDual`); `ref`/`type_name` report **`Errno`** for both. |
| `$.` | Simplified per-handle tracking (`last_readline_handle` + `handle_line_numbers`) vs Perl’s tied IO layer. |
| `$1`…`$n`, `%+`, `@-`, `@+` | Driven by the **Rust `regex` crate**; Perl’s regexp engine differs (lookbehind, backtracking, etc.). |
| `@_` | Works as the **subroutine argument array** in user subs; not fully identical to Perl’s XS calling conventions. |
| `pos $_` | Supported with `regex_pos` map; edge cases may differ from Perl. |
| `%SIG` / `$^C` | Tree-walker: **between statements**. VM: **between opcodes** (not inside a single native/Rust op). `$^C` reads `1` once after `SIGINT` if the latch was set; see [`perl_signal`](src/perl_signal.rs). |
| `$^I` | The **`fo`/`stryke` driver** applies **`-i`** / **`-i.bak`** for **`-n`/`-p`** over **`@ARGV`**; value is stored for compatibility with other code paths. |
| `$^V` | String form only (`v…` from crate version); not a Perl `version` object. |
| `$^E` | Uses `std::io::Error::last_os_error()`, not Perl’s per-platform extended error. |
| `${^GLOBAL_PHASE}` | **`DESTRUCT`** is set during [`Interpreter::run_global_teardown`](src/interpreter.rs) after a top-level program (post-`END`) so `DESTROY` drains match Perl’s global-destruction phase name; ordering vs every Perl 5 edge case is not guaranteed. Otherwise **`START`** … **`END`** track the tree-walker and VM the same way. |

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
| **Match / regexp** | Stash-backed `$&` / `$1` / `` $` `` / `$'` / `$+` and Rust `regex` still differ from Perl’s regexp engine; `${^…}` beyond dedicated fields are stubs in `special_caret_scalars` (except documented rows above). **Windows-only** `${^…}` names (e.g. Win32-specific perlvars) are not modeled; use `special_caret_scalars` / `undef`. |
| **Process / status** | `$PROCESS_ID` aliases. (`$?` is set after `system`, `capture`, and `close` on pipe children; POSIX-style packed status.) |
| **Perlio / globs** | Many handle-related specials beyond what IO builtins use. |
| **English.pm** | No `English` module tying long names to these variables. |

---

## Short list (what’s still missing)

**Still commonly missing vs stock Perl 5:** **full `English`** (only a subset of long names via [`english::scalar_alias`](src/english.rs)); **full `perlform` state machine** (formats/`write` are implemented but not full Perl parity); full **`$^V`** as a version object; Windows-only **`${^…}`** internals. **`exists $href->{key}`** / **`delete $href->{key}`** (hash references and blessed hash-like objects) are implemented; other exotic **`exists`/`delete`** targets may still differ from Perl 5.

| Area | Perl | Notes |
|------|------|--------|
| Dualvar | `$!`, `$@` | Both use **`errno_dual`** / **`ErrnoDual`**; numeric and string contexts differ per field. |
| Signals | `%SIG` | Unix: **`INT`**/**`TERM`**/**`ALRM`**/**`CHLD`** into subs between statements; non-Unix: stubs. |
| Aliases | `English` | Subset of long names when `use English` is active (see [`english`](src/english.rs)); not full core `English.pm`. |

---

## Maintenance

When adding I/O, regex, or `eval` behavior, update this file if new globals become meaningful or if `get_special_var` / `set_special_var` change. When adding documented `${^NAME}` entries from perl 5, consider extending [`special_vars::PERL5_DOCUMENTED_CARET_NAMES`](src/special_vars.rs).
