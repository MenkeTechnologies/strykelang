# Audit: _git Completion Function Requirements

This document audits what zsh builtins/functions `_git` calls and maps them to compsys Rust implementations.

## Core Completion Builtins

| zsh Builtin | Count | compsys Rust | Status |
|-------------|-------|--------------|--------|
| `_arguments` | 209 | `arguments_execute()` | ✅ Implemented |
| `compadd` | 55 | `compadd_execute()` | ✅ Implemented |
| `compset` | 49 | `compset_execute()` | ✅ Implemented |
| `zstyle` | 14 | `ZStyleStore`, `lookup_zstyle()` | ✅ Implemented |

## High-Level Completion Functions

| zsh Function | Count | compsys Rust | Status |
|--------------|-------|--------------|--------|
| `_describe` | 45 | `describe_execute()` | ✅ Implemented |
| `_alternative` | 57 | `alternative()` | ✅ Implemented |
| `_tags` | - | `TagManager`, `try_tags()` | ✅ Implemented |
| `_requested` | - | `requested()` | ✅ Implemented |
| `_wanted` | 30 | `wanted()` | ✅ Implemented |
| `_message` | 20 | `message()` | ✅ Implemented |
| `_nothing` | 11 | `nothing()` | ✅ Implemented |
| `_default` | - | `default_complete()` | ✅ Implemented |
| `_normal` | - | `normal()` | ✅ Implemented |

## File/Path Completion

| zsh Function | Count | compsys Rust | Status |
|--------------|-------|--------------|--------|
| `_files` | 108 | `files_execute()` | ✅ Implemented |
| `_directories` | 74 | `directories_execute()` | ✅ Implemented |
| `_path_files` | - | `path_files()` | ✅ Implemented |

## Program Execution

| zsh Function | Count | compsys Rust | Status |
|--------------|-------|--------------|--------|
| `_call_program` | 48 | `call_program()` | ✅ Implemented |

## Git-Specific Helper Functions (Need Shell Interpreter)

These are defined within `_git` itself and need the shell function interpreter:

| Helper Function | Count | Notes |
|-----------------|-------|-------|
| `__git_commits` | 58 | Runs `git rev-parse`, `git log` |
| `__git_guard_number` | 65 | Pattern matching guard |
| `__git_tree_files` | 30 | Lists files in git tree |
| `__git_cached_files` | 29 | `git ls-files --cached` |
| `__git_command_successful` | 27 | Checks git command exit status |
| `__git_revisions` | 22 | Branches, tags, remotes |
| `__git_branch_names` | 22 | `git branch` |
| `__git_config_values` | 31 | `git config --get-all` |
| `__git_remotes` | 19 | `git remote` |
| `__git_references` | 19 | All refs |
| `__git_tree_ishs` | 18 | Tree-ish objects |
| `__git_any_repositories` | 20 | Repository paths |
| `__git_ignore_line*` | 33 | Filters already-used args |
| `__git_changed` | 20 | Changed files |
| `__git_config_sections` | 15 | Config section names |
| `__git_blobs` | 12 | Blob objects |
| `__git_objects` | 11 | Git objects |
| `__git_is_committish` | 11 | Validates commit-ish |
| `__git_files` | 11 | All tracked files |
| `__git_submodules` | 10 | Submodule paths |
| `__git_is_treeish` | 10 | Validates tree-ish |
| `__git_heads` | 10 | Branch heads |
| `__git_tags` | 9 | Tag names |
| `__git_modified_files` | 8 | Modified files |

## Shell Language Features Required

For the shell function interpreter to execute `_git`:

### Variables & Parameters
- [x] Local variables (`local`, `declare`, `typeset`)
- [x] Arrays (`declare -a`, array indexing)
- [x] Associative arrays (`declare -A`, `opt_args`)
- [x] Special parameters (`$words`, `$CURRENT`, `$curcontext`, `$state`, `$line`)
- [x] Parameter expansion (${...})
- [x] Array slicing (`${words[@]}`, `${array[idx]}`)

### Control Flow
- [x] `if`/`then`/`else`/`fi`
- [x] `case`/`esac` with patterns
- [x] `for`/`do`/`done`
- [x] `while` loops
- [x] `(( ... ))` arithmetic
- [x] `[[ ... ]]` conditionals
- [x] `||` and `&&` short-circuit

### Functions
- [x] Function definition `name () { ... }`
- [x] Function calls
- [x] Return values
- [x] `$+functions[name]` to check if function exists

### Expansions
- [x] Command substitution `$(...)` and `` `...` ``
- [x] Arithmetic expansion `$(( ... ))`
- [x] Brace expansion `{a,b,c}`
- [x] Glob patterns
- [x] Parameter flags `${(f)var}`, `${(M)...}`, etc.

### Zsh-Specific
- [x] `emulate -L zsh` / `emulate ksh`
- [x] `setopt` / `unsetopt`
- [x] Array subscript flags `[(I)]`, `[(b:n:I)]`

## Implementation Status Summary

### Fully Ready in Native Rust (compsys/)
- ✅ `_arguments` - Full option/argument parsing
- ✅ `_describe` - Description formatting
- ✅ `_alternative` - Multiple completion sources
- ✅ `_tags` / `_requested` / `_wanted` - Tag management
- ✅ `compadd` - Adding completions
- ✅ `compset` - Prefix/suffix manipulation
- ✅ `zstyle` - Style configuration
- ✅ `_files` / `_directories` - File completion
- ✅ `_call_program` - External command execution
- ✅ Menu completion UI

### Requires Shell Interpreter (src/shell_*)
- ⚠️ `__git_*` helper functions - Need interpreter to run shell code
- ⚠️ Pattern matching in `case` statements
- ⚠️ Complex parameter expansions with flags
- ⚠️ `emulate` mode switching

## Execution Flow for `git add <TAB>`

1. User presses TAB → ZLE widget `expand-or-complete`
2. ZLE signals `WidgetResult::TriggerCompletion`
3. Shell calls `compsys::main_complete()`
4. `main_complete` looks up completion for "git" → finds `_git`
5. **Shell interpreter** executes `_git` function:
   - Sets up `$words`, `$CURRENT`, etc.
   - Calls `_arguments` (→ Rust `arguments_execute`)
   - Reaches state `->file`
   - Calls `_alternative` (→ Rust `alternative`)
   - Calls `__git_modified_files` (→ Shell interpreter runs git)
   - `compadd` called (→ Rust `compadd_execute`)
6. Completions returned to menu system
7. Menu rendered by `compsys::menu::MenuState`

## Shell Runner Bridge

The `compsys/shell_runner.rs` module provides the bridge between the shell interpreter and the native Rust completion primitives:

```rust
// Shell interpreter calls this when executing completion functions
pub trait CompletionRunner {
    fn run_completion_function(
        &mut self,
        func_name: &str,
        context: &ShellCompletionContext,
        zstyle: &ZStyleStore,
    ) -> CompletionResult;
    
    fn has_completion_function(&self, name: &str) -> bool;
    fn get_completer(&self, command: &str) -> Option<String>;
}

// Dispatches builtin calls to native Rust
pub struct BuiltinDispatcher {
    pub fn compadd(&mut self, ...) -> i32;
    pub fn compset(&mut self, ...) -> i32;
    pub fn arguments(&mut self, ...) -> i32;
    pub fn describe(&mut self, ...) -> i32;
    pub fn files(&mut self, ...) -> i32;
    pub fn directories(&mut self, ...) -> i32;
    pub fn message(&mut self, ...);
    pub fn zstyle_lookup(&self, ...) -> Option<String>;
    pub fn begin_group(&mut self, ...);
    pub fn end_group(&mut self);
    pub fn add_completion(&mut self, ...);
}
```

## Conclusion

The core completion infrastructure is implemented in Rust:

| Component | Status |
|-----------|--------|
| `_arguments` parser | ✅ Native Rust |
| `_describe` formatter | ✅ Native Rust |
| `compadd` | ✅ Native Rust |
| `compset` | ✅ Native Rust |
| `zstyle` | ✅ Native Rust |
| `_files`/`_directories` | ✅ Native Rust |
| Menu completion UI | ✅ Native Rust |
| ZLE widget integration | ✅ Native Rust |
| Shell runner bridge | ✅ Native Rust |

What's missing is the **shell function interpreter** in `src/shell_exec.rs` that needs to:
1. Parse and execute zsh function bodies
2. Handle zsh-specific parameter expansions (`${(f)var}`, `${(M)...}`)
3. Execute the `__git_*` helper functions that call external `git` commands
4. Call the `BuiltinDispatcher` methods when completion builtins are invoked

The completion functions themselves (`_git`, `_ls`, etc.) are **shell code** and should remain interpreted, not ported to Rust. The Rust code provides the fast, native implementation of the core primitives they call.
