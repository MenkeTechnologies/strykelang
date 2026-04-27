# Stryke Function Namespacing — MANDATORY

## Rule: NEVER shadow stdlib. ALWAYS namespace your functions.

When writing functions in Stryke, you MUST namespace them to avoid shadowing builtins.

## Syntax

```perl
fn Namespace::funcname($args) { body }
```

## Examples

```perl
# WRONG — shadows stdlib sum()
fn sum($x, $y) { $x + $y }

# CORRECT — namespaced, no collision
fn Math::sum($x, $y) { $x + $y }
fn MyProject::Utils::sum($x, $y) { $x + $y }
```

## Calling namespaced functions

```perl
fn Stats::mean(@vals) { sum(@vals) / scalar(@vals) }
fn Stats::median(@vals) {
    my @s = sort { $a <=> $b } @vals
    $s[@s / 2]
}

p Stats::mean(1, 2, 3, 4, 5)    # 3
p Stats::median(1, 2, 3, 4, 5)  # 3
```

## Multi-level namespaces

```perl
fn My::Deep::Utils::add($a, $b) { $a + $b }
fn My::Deep::Utils::mul($a, $b) { $a * $b }

p My::Deep::Utils::add(10, 20)  # 30
```

## Why this matters

Stryke has 3200+ builtins. Common names are taken:

- `sum`, `min`, `max`, `avg`, `mean`
- `map`, `grep`, `filter`, `reduce`
- `sort`, `reverse`, `shuffle`
- `read`, `write`, `open`, `close`
- `split`, `join`, `trim`, `chomp`
- `push`, `pop`, `shift`, `unshift`
- `keys`, `values`, `exists`, `delete`

If you define `fn sum(...)`, you shadow the builtin and break existing code.

## Checklist before defining a function

1. Is this name already a Stryke builtin? → Namespace it
2. Is this name a common word? → Namespace it
3. Could someone else define this name? → Namespace it
4. When in doubt → Namespace it

## Convention

Use `ProjectName::Module::function` pattern:

```perl
fn MyApp::Config::load($path) { ... }
fn MyApp::Config::save($path, $data) { ... }
fn MyApp::DB::connect($dsn) { ... }
fn MyApp::DB::query($sql) { ... }
```

## DO NOT

```perl
# DO NOT define top-level functions with common names
fn add($x, $y) { ... }      # BAD
fn process($data) { ... }   # BAD
fn handle($event) { ... }   # BAD
fn get($key) { ... }        # BAD
fn set($key, $val) { ... }  # BAD

# DO namespace everything
fn Utils::add($x, $y) { ... }        # GOOD
fn Handler::process($data) { ... }   # GOOD
fn Events::handle($event) { ... }    # GOOD
fn Cache::get($key) { ... }          # GOOD
fn Cache::set($key, $val) { ... }    # GOOD
```

## Reserved Words

These names cannot be used as function names at all (even namespaced) — they are lexer-level operators or language keywords:

```
y tr s m q qq qw qx qr
if unless while until for foreach given when else elsif
do eval return last next redo goto
my our local state sub fn class struct enum trait
use no require package BEGIN END CHECK INIT UNITCHECK
and or not x eq ne lt gt le ge cmp
```

Attempting `fn y { }` or `fn Foo::goto { }` produces:
```
`y` is a reserved word and cannot be used as a function name
```

## Summary

**ALWAYS use `fn Namespace::name(...)` syntax. NEVER define bare top-level functions.**
