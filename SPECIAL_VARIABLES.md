# Perl special variables vs perlrs

This document audits **Perl 5’s “special” globals** against **perlrs** as implemented in the tree-walker / VM (`src/interpreter.rs`, `src/lexer.rs`, `src/vm.rs`, `src/scope.rs`). It is **not** an exhaustive perlvar(1) list; it groups the usual categories and states what is wired, partial, or absent.

Legend: **Yes** = behavior matches intent for typical use; **Partial** = exists but semantics differ; **No** = not implemented or wrong tokenization.

---

## Implemented with dedicated handling

| Perl | Role | perlrs |
|------|------|--------|
| `$_` | Default topic | Ordinary scalar `$_` in scope; set by `map`/`grep`/many iterators, `given`, `readline`, etc. |
| `$.` | Input line number | `Interpreter.line_number` via `get_special_var(".")` (`src/interpreter.rs`); incremented on `readline` paths. |
| `$/` | Input record separator | `irs` field; get/set via `get_special_var` / `set_special_var` for `"/"`. |
| `$,` | Output field separator | `ofs` field; `","` in special get/set. |
| `$\` | Output record separator | `ors` field; `"\\"` in special get/set. |
| `$!` | OS error (errno string) | Reads use `Interpreter.errno` (`get_special_var("!")`). Writes go to the scalar stash and **do not** update `errno`, so they are not read back — prefer treating `$!` as read-only. |
| `$@` | Eval error | Reads use `eval_error` (`get_special_var("@")`). Writes store a scalar `"@"` that is **not** read back — same read/write split as `$!`. |
| `$0` | Program name | `program_name`; `"0"` in special get/set. |
| `$$` | Process ID | `get_special_var("$$")` → `std::process::id()`. |
| `$1`…`$n` | Capture groups | After a successful match, `apply_regex_captures` sets `scope` scalars `"1"`…`"n"` (`src/interpreter.rs`). |
| `@-` / `@+` | Match start/end offsets | After a successful match, `apply_regex_captures` sets arrays `"-` and `"+"` (whole match at index 0, then groups; `-1` for unused groups). |
| `%+` | Named captures | `scope.set_hash("+", …)` from regex named groups. |
| `@ARGV` | Script arguments | Declared in `Interpreter::new`; populated by `main` driver (`src/main.rs`). |
| `$ARGV` | Current filename for `<>` | `argv_current_file`; set when `<>` opens each `@ARGV` file; empty when reading stdin or before first file. |
| `<>` | Read lines | Iterate `@ARGV` files in order (then undef); if `@ARGV` is empty, stdin. |
| `@INC` | Library path | Array of search dirs; `%INC` used for loaded paths in `require`. |
| `%INC` | Loaded modules | Hash entries set by `require`/`use` (see `require_execute`). |
| `%ENV` | Environment | Hash in scope; filled from `std::env::vars()` on first access (`Interpreter::materialize_env_if_needed`) to reduce cold-start cost. |
| `%SIG` | Signal handlers | Hash exists in scope; **OS signal delivery** is not wired to these entries. |
| `$]` | Numeric language version | `get_special_var("]")` → `perl_bracket_version()` (emulated Perl 5.x.y level; see `perl_bracket_version` in `src/interpreter.rs`). |
| `$;` | Subscript separator | `subscript_sep` field; default `\x1c` (Perl `\034`). |
| `$^I` | In-place edit extension | `inplace_edit` string; lexer reads `$^` + letter as variable name `^I`. |
| `$^D` | Debug flags | `debug_flags` (`i64`). |
| `$^P` | Debugger flags | `perl_debug_flags` (`i64`). |
| `$^S` | Exception state (in eval) | `eval_nesting > 0` while `eval` runs (tree-walker and VM `eval` / `evalblock`). |
| `$^W` | Warnings | `warnings` boolean (`true` → `1`). |
| `__PACKAGE__` | Current package | Scalar in scope; `package` statements update it. |
| `wantarray` | List/scalar/void context | `WantarrayCtx` on interpreter; `ExprKind::Wantarray` / `BuiltinId::Wantarray`. |

---

## Partially implemented or different from Perl 5

| Perl | Issue |
|------|-------|
| `$!` / `$@` | **String** errno / eval error only; not dual-var. Assignments do not feed back into reads (see table above). |
| `$.` | Updated on **readline-style** I/O; not a full per-handle line counter as in Perl. |
| `$1`…`$n`, `%+`, `@-`, `@+` | Driven by the **Rust `regex` crate**; Perl’s regexp engine differs (lookbehind, backtracking, etc.). |
| `@_` | Works as the **subroutine argument array** in user subs; not fully identical to Perl’s XS calling conventions. |
| `pos $_` | Supported with `regex_pos` map; edge cases may differ from Perl. |
| `%SIG` | Storage only; **no** Unix signal delivery into subs. |
| `$^I` | In-place editing is **not** implemented; the value is stored for compatibility. |

---

## Lexer may tokenize but no Perl semantics

Single-character names after `$` are accepted (`src/lexer.rs` `read_variable_name`), including `&` `` ` `` `'` `+` `*` `?` `|` etc. **Only** the subset handled in `get_special_var` / `set_special_var` and regex capture logic has meaning. The rest resolve as **ordinary scalars** in scope (usually undef), **not** Perl’s `$&`, `` $` ``, `$'`, `$+`, etc. **`$?`** (child wait status) and **`$|`** (stdout autoflush after `print` / `printf` in the VM and tree interpreter) **are** implemented — see `get_special_var` / `set_special_var` and `Interpreter::record_child_exit_status`.

**`$^X` (caret + letter):** The lexer reads **`^` plus one alphabetic character** as names like `^I`, `^O`, `^W` (see `read_variable_name`).

---

## Not implemented (common Perl specials)

| Category | Examples |
|----------|----------|
| **Match / regexp** | `${^MATCH}` / `${^PREMATCH}` / `${^POSTMATCH}` — not implemented; `$&` / `` $` `` / `$'` / `$+` (last bracket) are set on the scalar stash from `apply_regex_captures` (not via `get_special_var`). |
| **Process / status** | `$^E` extended OS error, `$PROCESS_ID` aliases. (`$?` is set after `system`, `capture`, and `close` on pipe children; POSIX-style packed status.) |
| **Ids / groups** | `$<` `$>` `$(` `$)` real/effective uid/gid. |
| **Perlio / globs** | Many handle-related specials beyond what IO builtins use. |
| **Compiler / phase** | `$^H`, `${^WARNING_BITS}`, `${^GLOBAL_PHASE}`, etc. |
| **Time** | `$^T` base time, `$^V` version object. |
| **English.pm** | No `English` module tying long names to these variables. |

---

## Short list (what’s still missing)

If you only care about **common Perl specials** not yet covered (see **Partially implemented** for things that exist but differ):

| Area | Perl | Notes |
|------|------|--------|
| OS / identity | `$^O` | Not in `get_special_var` (lexer allows `$^O`). |
| Time / version | `$^T`, `$^V` | `$^T`: start time exists internally but is **not** exposed as `$^T`. `$^V` (version object) **not** in `get_special_var`. |
| OS error | `$^E` | Extended OS error (Perl uses it heavily on Windows). |
| Compiler / phase | `$^H`, `${^WARNING_BITS}`, `${^GLOBAL_PHASE}`, … | Not wired. |
| Process ids | `$<`, `$>`, `$(`, `$)` | Real/effective uid/gid. |
| Match spellings | `${^MATCH}`, `${^PREMATCH}`, `${^POSTMATCH}` | Not via `get_special_var`. After a match, `$&`, `` $` ``, `$'`, `$+` are set on the **stash** as `"&"`, `` ` ``, `"'"`, `"+"` in `apply_regex_captures` — not the `${^…}` variable names. |
| Dualvar | `$!`, `$@` | String reads only; not full dualvar; writes don’t round-trip (see partial table). |
| Signals | `%SIG` | Hash exists; **no** delivery of OS signals into Perl subs. |
| Aliases | `English` | No long-name aliases module. |

---

## Maintenance

When adding I/O, regex, or `eval` behavior, update this file if new globals become meaningful or if `get_special_var` / `set_special_var` change.
