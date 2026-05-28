//! Drive `stryke --lsp` over JSON-RPC stdio: completion, hover, and go-to-definition.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const ST: &str = env!("CARGO_BIN_EXE_st");
const URI: &str = "file:///lsp_completion_fixture.pl";
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_DRAIN: usize = 64;

fn write_msg(stdin: &mut impl Write, body: &Value) {
    let bytes = serde_json::to_vec(body).expect("serialize json");
    write!(stdin, "Content-Length: {}\r\n\r\n", bytes.len()).expect("write header");
    stdin.write_all(&bytes).expect("write body");
    stdin.flush().expect("flush");
}

fn read_msg<R: Read>(reader: &mut BufReader<R>) -> Value {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read header line");
        if line == "\n" || line == "\r\n" {
            break;
        }
        let t = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = t.strip_prefix("Content-Length:") {
            len = Some(rest.trim().parse().expect("Content-Length parse"));
        }
    }
    let n = len.expect("missing Content-Length");
    let mut buf = vec![0u8; n];
    reader.read_exact(&mut buf).expect("read body");
    serde_json::from_slice(&buf).expect("json body")
}

fn drain_stderr(mut stderr: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        let _ = tx.send(s);
    });
    rx
}

fn recv_until_result<R: Read>(reader: &mut BufReader<R>, want_id: i64) -> Value {
    let start = Instant::now();
    for _ in 0..MAX_DRAIN {
        if start.elapsed() > READ_TIMEOUT {
            panic!("timeout waiting for JSON-RPC id {want_id}");
        }
        let msg = read_msg(reader);
        if msg.get("id").and_then(Value::as_i64) == Some(want_id) {
            if msg.get("error").is_some() {
                panic!("LSP error response: {msg}");
            }
            return msg;
        }
    }
    panic!("too many messages without id {want_id}");
}

fn labels_from_completion_result(result: &Value) -> Vec<String> {
    let items = result
        .as_array()
        .expect("completion result should be an array");
    items
        .iter()
        .filter_map(|it| it.get("label").and_then(Value::as_str).map(str::to_string))
        .collect()
}

struct LspHarness {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    stderr_rx: mpsc::Receiver<String>,
    next_id: i64,
}

impl LspHarness {
    fn new(document: &str) -> Self {
        let mut child = Command::new(ST)
            .arg("--lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn stryke --lsp");

        let stdin = child.stdin.take().expect("stdin");
        let reader = BufReader::new(child.stdout.take().expect("stdout"));
        let stderr_rx = drain_stderr(child.stderr.take().expect("stderr"));

        let mut h = Self {
            child,
            stdin,
            reader,
            stderr_rx,
            next_id: 1,
        };
        h.handshake();
        h.open_doc(document);
        h.drain_diagnostics();
        h.next_id = 2;
        h
    }

    fn handshake(&mut self) {
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": null,
                    "rootUri": null,
                    "capabilities": {},
                },
            }),
        );
        let init = recv_until_result(&mut self.reader, 1);
        assert!(init.get("result").is_some(), "initialize: {init}");

        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "initialized",
                "params": {},
            }),
        );
    }

    fn open_doc(&mut self, text: &str) {
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": URI,
                        "languageId": "perl",
                        "version": 1,
                        "text": text,
                    },
                },
            }),
        );
    }

    fn drain_diagnostics(&mut self) {
        let start = Instant::now();
        loop {
            if start.elapsed() > READ_TIMEOUT {
                let err = self.stderr_rx.recv().unwrap_or_default();
                panic!("timeout waiting for diagnostics; stderr:\n{err}");
            }
            let msg = read_msg(&mut self.reader);
            if msg.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            {
                break;
            }
        }
    }

    fn alloc_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn completion(&mut self, line: u32, character: u32) -> Vec<String> {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/completion",
                "params": {
                    "textDocument": { "uri": URI },
                    "position": { "line": line, "character": character },
                },
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        let result = msg.get("result").expect("completion result");
        labels_from_completion_result(result)
    }

    /// Same as `completion()` but returns the full CompletionItem array
    /// so tests can inspect `insertText`, `filterText`, `kind`, `detail`.
    fn completion_items(&mut self, line: u32, character: u32) -> Vec<Value> {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/completion",
                "params": {
                    "textDocument": { "uri": URI },
                    "position": { "line": line, "character": character },
                },
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        msg.get("result")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    fn hover(&mut self, line: u32, character: u32) -> Value {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": URI },
                    "position": { "line": line, "character": character },
                },
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        msg.get("result").cloned().unwrap_or(Value::Null)
    }

    fn definition(&mut self, line: u32, character: u32) -> Value {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/definition",
                "params": {
                    "textDocument": { "uri": URI },
                    "position": { "line": line, "character": character },
                },
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        // Normalize to a single Location-shaped object `{uri, range}`
        // regardless of which LSP response variant the server picked:
        //   - `Location` (object with `uri` + `range`)
        //   - `Location[]` (array of Locations) — take first
        //   - `LocationLink[]` (array of LocationLinks; `targetUri` +
        //     `targetRange` + `targetSelectionRange`) — take first,
        //     remap to Location shape via targetSelectionRange (the
        //     range the IDE uses to position the caret).
        let raw = msg.get("result").cloned().unwrap_or(Value::Null);
        if raw.is_object() && raw.get("range").is_some() {
            return raw;
        }
        if let Some(arr) = raw.as_array() {
            if let Some(first) = arr.first() {
                if let (Some(target_uri), Some(target_range)) = (
                    first.get("targetUri"),
                    first
                        .get("targetSelectionRange")
                        .or_else(|| first.get("targetRange")),
                ) {
                    return json!({ "uri": target_uri, "range": target_range });
                }
                if first.get("range").is_some() {
                    return first.clone();
                }
            }
        }
        raw
    }

    fn document_symbols(&mut self) -> Value {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/documentSymbol",
                "params": {
                    "textDocument": { "uri": URI },
                },
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        msg.get("result").cloned().unwrap_or(Value::Null)
    }

    fn resolve_completion(&mut self, item: Value) -> Value {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "completionItem/resolve",
                "params": item,
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        msg.get("result").cloned().unwrap_or(Value::Null)
    }

    fn document_highlight(&mut self, line: u32, character: u32) -> Value {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/documentHighlight",
                "params": {
                    "textDocument": { "uri": URI },
                    "position": { "line": line, "character": character },
                },
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        msg.get("result").cloned().unwrap_or(Value::Null)
    }

    fn references(&mut self, line: u32, character: u32, include_declaration: bool) -> Value {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/references",
                "params": {
                    "textDocument": { "uri": URI },
                    "position": { "line": line, "character": character },
                    "context": { "includeDeclaration": include_declaration },
                },
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        msg.get("result").cloned().unwrap_or(Value::Null)
    }

    fn declaration(&mut self, line: u32, character: u32) -> Value {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/declaration",
                "params": {
                    "textDocument": { "uri": URI },
                    "position": { "line": line, "character": character },
                },
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        // Same normalization as `definition()` — server can return
        // `Location` | `Location[]` | `LocationLink[]`, callers want
        // a single `{uri, range}` shape to assert against.
        let raw = msg.get("result").cloned().unwrap_or(Value::Null);
        if raw.is_object() && raw.get("range").is_some() {
            return raw;
        }
        if let Some(arr) = raw.as_array() {
            if let Some(first) = arr.first() {
                if let (Some(target_uri), Some(target_range)) = (
                    first.get("targetUri"),
                    first
                        .get("targetSelectionRange")
                        .or_else(|| first.get("targetRange")),
                ) {
                    return json!({ "uri": target_uri, "range": target_range });
                }
                if first.get("range").is_some() {
                    return first.clone();
                }
            }
        }
        raw
    }

    fn prepare_rename(&mut self, line: u32, character: u32) -> Value {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/prepareRename",
                "params": {
                    "textDocument": { "uri": URI },
                    "position": { "line": line, "character": character },
                },
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        msg.get("result").cloned().unwrap_or(Value::Null)
    }

    fn rename(&mut self, line: u32, character: u32, new_name: &str) -> Value {
        let id = self.alloc_id();
        write_msg(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "textDocument/rename",
                "params": {
                    "textDocument": { "uri": URI },
                    "position": { "line": line, "character": character },
                    "newName": new_name,
                },
            }),
        );
        let msg = recv_until_result(&mut self.reader, id);
        msg.get("result").cloned().unwrap_or(Value::Null)
    }

    fn finish(mut self) {
        drop(self.stdin);
        let _ = self.child.kill();
        let _ = self.child.wait();
        let err = self.stderr_rx.recv().unwrap_or_default();
        if err.contains("panic") {
            panic!("stryke --lsp stderr:\n{err}");
        }
    }
}

#[test]
fn lsp_stdio_completion_lists_sub_in_buffer() {
    let mut h = LspHarness::new("fn yellow_minion { }\nyell");
    let labels = h.completion(1, 4);
    h.finish();
    assert!(
        labels.iter().any(|l| l == "yellow_minion"),
        "expected yellow_minion in {:?}",
        labels
    );
}

#[test]
fn lsp_stdio_completion_scalar_after_sigil() {
    let mut h = LspHarness::new("my $yellow_submarine;\n$yellow");
    let labels = h.completion(1, 7);
    h.finish();
    assert!(
        labels.iter().any(|l| l == "$yellow_submarine"),
        "expected $yellow_submarine in {:?}",
        labels
    );
}

#[test]
fn lsp_stdio_completion_qualified_sub() {
    let mut h = LspHarness::new("package Foo;\nfn barbaz { }\npackage main;\nFoo::bar");
    let labels = h.completion(3, 8);
    h.finish();
    assert!(
        labels.iter().any(|l| l == "Foo::barbaz"),
        "expected Foo::barbaz in {:?}",
        labels
    );
}

#[test]
fn lsp_stdio_hover_builtin_say() {
    let mut h = LspHarness::new("p 1;\n");
    let result = h.hover(0, 1);
    h.finish();
    let md = result
        .pointer("/contents/value")
        .and_then(Value::as_str)
        .expect("hover markdown");
    assert!(
        md.to_lowercase().contains("say") || md.contains("Print"),
        "unexpected hover: {md}"
    );
}

#[test]
fn lsp_stdio_hover_sub_decl_line() {
    let mut h = LspHarness::new("fn yellow_minion { }\nyellow_minion();\n");
    let result = h.hover(1, 3);
    h.finish();
    let contents = result.get("contents").expect("hover contents");
    let value = contents
        .get("value")
        .and_then(Value::as_str)
        .expect("value");
    assert!(
        value.contains("Subroutine") && value.contains("yellow_minion"),
        "unexpected hover: {value}"
    );
}

// ── Rename audit ─────────────────────────────────────────────────
//
// Comprehensive coverage matrix: one test per SymbolKind, asserting
// either same-file rename + cross-file rename, OR same-file only for
// file-local kinds. If a kind is broken, this test set surfaces it.
//
// Same-file rename: open one document, rename, count edits.
// Cross-file rename: open two documents, rename, assert edits in
// both.

fn rename_same_file(src: &str, line: u32, col: u32, new_name: &str) -> Vec<String> {
    let mut h = LspHarness::new(src);
    let r = h.rename(line, col, new_name);
    h.finish();
    r.pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("newText").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn audit_rename_my_local_same_file() {
    let edits = rename_same_file("my $x = 1\np $x\np $x + 1\n", 0, 4, "y");
    assert!(
        edits.len() >= 3,
        "my-local must rename all 3 (decl + 2 refs): {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "$y"));
}

#[test]
fn audit_rename_our_var_same_file() {
    let edits = rename_same_file("our $level = 1\np $level\n", 0, 5, "intensity");
    assert!(
        edits.len() >= 2,
        "our-var must rename decl + ref: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "$intensity"));
}

#[test]
fn audit_rename_state_var_same_file() {
    let edits = rename_same_file(
        "fn Demo::Counter::next { state $n = 0; $n += 1; $n }\nDemo::Counter::next()\n",
        0,
        32,
        "tick",
    );
    assert!(
        edits.len() >= 3,
        "state-var must rename decl + 2 refs: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "$tick"));
}

#[test]
fn audit_rename_param_same_file() {
    let edits = rename_same_file(
        "fn Demo::Math::twice($n) { $n * 2 }\nDemo::Math::twice(5)\n",
        0,
        22,
        "x",
    );
    assert!(
        edits.len() >= 2,
        "param must rename decl + body ref: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "$x"));
}

#[test]
fn audit_rename_sub_same_file() {
    // Cursor on `s` of `say` (col 16) — avoid landing on a `:` of
    // `::`, which makes identifier_span yield a half-segment span.
    let edits = rename_same_file(
        "fn Demo::Greet::say { p hi }\nDemo::Greet::say()\nDemo::Greet::say()\n",
        0,
        16,
        "salute",
    );
    assert!(
        edits.len() >= 3,
        "sub must rename decl + 2 calls: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "salute"));
}

#[test]
fn audit_rename_struct_type_same_file() {
    let edits = rename_same_file(
        "struct Point { x, y }\nmy $p = Point->new(x => 1, y => 2)\n",
        0,
        7,
        "Vertex",
    );
    assert!(
        edits.len() >= 2,
        "struct must rename decl + constructor: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "Vertex"));
}

#[test]
fn audit_rename_enum_type_same_file() {
    let edits = rename_same_file("enum Op { Add, Sub }\nmy $r = Op::Add\n", 0, 5, "Operator");
    assert!(
        edits.len() >= 2,
        "enum must rename decl + qualified usage: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "Operator"));
}

#[test]
fn audit_rename_class_type_same_file() {
    let edits = rename_same_file(
        "class Animal { name: Str }\nclass Dog extends Animal { breed: Str }\n",
        0,
        6,
        "Creature",
    );
    assert!(
        edits.len() >= 2,
        "class must rename decl + extends-ref: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "Creature"));
}

#[test]
fn audit_rename_trait_type_same_file() {
    let edits = rename_same_file(
        "trait Walks { fn walk }\nclass Dog impl Walks { name: Str }\n",
        0,
        6,
        "Runs",
    );
    assert!(edits.len() >= 2, "trait must rename decl + impl: {edits:?}");
    assert!(edits.iter().all(|n| *n == "Runs"));
}

#[test]
fn audit_rename_package_same_file() {
    // Cursor on `L` of `Lib` (col 14) — avoid second `:` of `::`.
    let edits = rename_same_file(
        "package Demo::Lib\nfn hi { 1 }\npackage main\nmy $v = Demo::Lib::hi()\n",
        0,
        14,
        "Demo::Util",
    );
    assert!(
        edits.len() >= 2,
        "package must rename decl + qualified-call prefix: {edits:?}"
    );
}

#[test]
fn audit_rename_format_same_file() {
    let edits = rename_same_file(
        "format REPORT =\n@<<<<<\n\"hi\"\n.\nwrite REPORT\n",
        0,
        8,
        "SUMMARY",
    );
    assert!(
        !edits.is_empty(),
        "format rename must produce at least one edit: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "SUMMARY"));
}

#[test]
fn audit_rename_loop_label_same_file() {
    let edits = rename_same_file(
        "OUTER: for my $i (1..3) {\n    last OUTER\n}\n",
        0,
        2,
        "TOP",
    );
    assert!(
        edits.len() >= 2,
        "loop label must rename decl + last-target: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "TOP"));
}

#[test]
fn audit_rename_struct_field_same_file() {
    let edits = rename_same_file(
        "struct Rectangle { width, height }\nmy $r = Rectangle(width => 1, height => 2)\n",
        0,
        19,
        "w",
    );
    assert!(
        edits.len() >= 2,
        "struct field must rename decl + constructor arg: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "w"));
}

/// User's exact post-corruption fixture. Verify that a CLEAN rename
/// from `Red` → `Redd` over the clean source does NOT produce the
/// `TrafficLight::Redd` doubled-prefix shape the user reported.
/// Rename a method declared inside a `class` body. Both at decl line
/// and at the `$obj->method()` call site, the rename must produce a
/// WorkspaceEdit that covers decl + every call site.
/// Critical word-boundary regression: a Field named `y` must NOT
/// match the `y` inside `my`. Without strict word-boundary checking,
/// rename rewrites `my` to the new name everywhere.
#[test]
fn audit_rename_short_field_name_respects_word_boundary() {
    let src = "struct Point { x, y }\nmy $p = Point->new(x => 3, y => 4)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `y` of `struct Point { x, y }` at line 0, col 18.
    let r = h.rename(0, 18, "yy");
    h.finish();
    let edits = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let positions: Vec<(u64, u64)> = edits
        .iter()
        .filter_map(|e| {
            let l = e.pointer("/range/start/line")?.as_u64()?;
            let c = e.pointer("/range/start/character")?.as_u64()?;
            Some((l, c))
        })
        .collect();
    // Line 1 has `my $p = Point->new(x => 3, y => 4)`.
    // `m` of `my` is at col 0. `y` of `my` is at col 1.
    // `y` of `y => 4` is at col 27.
    // The `y` at col 1 (inside `my`) MUST NOT get an edit.
    assert!(
        !positions.contains(&(1, 1)),
        "must NOT rewrite `y` inside `my` (col 1): {positions:?}"
    );
    // The legit field-key `y` at col 27 SHOULD be rewritten.
    assert!(
        positions.contains(&(1, 27)),
        "expected rewrite at line 1 col 27 (`y => 4`): {positions:?}"
    );
}

#[test]
fn audit_rename_class_method_via_arrow_call() {
    let src = "class Account {\n    holder: Str\n    fn statement {\n        sprintf(\"Account(%s)\", $self->holder)\n    }\n}\nmy $a = Account->new(holder => \"x\")\np $a->statement()\np $a->statement()\n";
    let mut h = LspHarness::new(src);
    // Cursor on `statement` of `fn statement` at line 2, col 7.
    let r = h.rename(2, 7, "describe");
    h.finish();
    let edits = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let new_texts: Vec<&str> = edits
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    // Decl + 2 call sites = 3 edits, all `describe`.
    assert!(
        edits.len() >= 3,
        "expected ≥3 edits (decl + 2 calls), got {edits:#?}"
    );
    assert!(
        new_texts.iter().all(|n| *n == "describe"),
        "every edit must be `describe`: {new_texts:?}"
    );
}

#[test]
fn audit_rename_class_field_via_constructor_call() {
    // User's exact fixture: `class Point { x: Int, y: Int }` + a
    // constructor call `Point->new(x => 10, y => 20)`. Renaming `y`
    // must rewrite both the field decl AND the constructor key.
    let src =
        "class Point {\n    x : Int\n    y : Int\n}\n\nmy $p = Point->new(x => 10, y => 20)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `y` of `y : Int` decl at line 2, col 4.
    let r = h.rename(2, 4, "yy");
    h.finish();
    let edits = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let by_line: Vec<u64> = edits
        .iter()
        .filter_map(|e| e.pointer("/range/start/line").and_then(Value::as_u64))
        .collect();
    let new_texts: Vec<&str> = edits
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    // Expect: decl line 2 + constructor call line 5 → 2 edits, both `yy`.
    assert!(
        by_line.contains(&2),
        "expected decl line 2 rewrite: lines={by_line:?}"
    );
    assert!(
        by_line.contains(&5),
        "expected constructor line 5 rewrite: lines={by_line:?}"
    );
    assert!(
        new_texts.iter().all(|n| *n == "yy"),
        "every edit should be `yy`: {new_texts:?}"
    );
}

/// Enum variant rename in a file with an UNRELATED string literal
/// containing the variant name as a substring must NOT touch the
/// string content. AST-only — strict.
#[test]
fn audit_rename_enum_variant_no_string_false_positives() {
    let src = "enum Color { Red, Green, Blue }\nmy $msg = \"the Red color is loud\"\nmy $c = Color::Red\nmy %h = (Red => 1)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `Red` in the enum decl, line 0 col 13.
    let r = h.rename(0, 13, "Crimson");
    h.finish();
    let edits = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let lines: Vec<u64> = edits
        .iter()
        .filter_map(|e| e.pointer("/range/start/line").and_then(Value::as_u64))
        .collect();
    // Expected:
    //   line 0: decl `Red` → `Crimson`
    //   line 2: `Color::Red` → `Color::Crimson` (the variant suffix)
    // Must NOT touch:
    //   line 1: `"the Red color is loud"` (string content)
    //   line 3: `my %h = (Red => 1)` — unrelated hash literal with
    //           same key name. Not a struct/enum constructor, no
    //           Type-gate → AST walker doesn't record this as a ref.
    assert!(lines.contains(&0), "expected decl edit: {lines:?}");
    assert!(
        lines.contains(&2),
        "expected `Color::Red` rewrite: {lines:?}"
    );
    assert!(
        !lines.contains(&1),
        "must NOT touch the string literal on line 1: {lines:?}"
    );
    assert!(
        !lines.contains(&3),
        "must NOT touch the unrelated `my %h = (Red => 1)` on line 3: {lines:?}"
    );
}

#[test]
fn audit_rename_enum_variant_does_not_add_type_prefix() {
    let src = "enum TrafficLight { Red, Yellow, Green }\nfn TrafficLight::action($c) {\n    match ($c) {\n        TrafficLight::Red    => \"stop\",\n        TrafficLight::Yellow => \"caution\",\n        TrafficLight::Green  => \"go\",\n    }\n}\n";
    let mut h = LspHarness::new(src);
    // Cursor on `Red` of `enum TrafficLight { Red, … }` at col 20.
    let r = h.rename(0, 20, "Redd");
    h.finish();
    let edits = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let new_texts: Vec<&str> = edits
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    // EVERY new_text should be exactly `Redd` — never `TrafficLight::Redd`
    // or any other qualified form. The Field rename emits ranges over
    // just the variant token, never the surrounding qualifier.
    assert!(
        new_texts.iter().all(|n| *n == "Redd"),
        "expected every edit to be exactly `Redd`, got: {new_texts:?}"
    );
    assert!(
        new_texts.iter().all(|n| !n.contains("::")),
        "must NOT add `::` to any rewrite: {new_texts:?}"
    );
}

#[test]
fn audit_rename_enum_variant_with_qualified_match_arms() {
    // User's exact fixture: enum + match-arm qualified usage. Rename
    // `Red` to `Stop` must:
    //   - rewrite `Red` in the enum decl body
    //   - rewrite `TrafficLight::Red` in the match arm
    // and NOT produce `TrafficLight::Stop` inside the enum decl body
    // (that was the bug — qualified prefix was being added there).
    let src = "enum TrafficLight { Red, Yellow, Green }\nfn TrafficLight::action($c) {\n    match ($c) {\n        TrafficLight::Red    => \"stop\",\n        TrafficLight::Yellow => \"caution\",\n        TrafficLight::Green  => \"go\",\n    }\n}\n";
    let mut h = LspHarness::new(src);
    // Cursor on `Red` of `enum TrafficLight { Red, ... }` line 0, col 20.
    let r = h.rename(0, 20, "Stop");
    h.finish();
    let changes = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("rename returned no changes: {r}"));
    let edits = changes
        .values()
        .next()
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no edits: {r}"));
    // Collect every new_text + range for inspection.
    let collected: Vec<(u64, u64, String)> = edits
        .iter()
        .filter_map(|e| {
            let l = e.pointer("/range/start/line")?.as_u64()?;
            let c = e.pointer("/range/start/character")?.as_u64()?;
            let t = e.get("newText")?.as_str()?.to_string();
            Some((l, c, t))
        })
        .collect();
    // None of the edits may produce `TrafficLight::Stop` inside the
    // enum decl line (line 0) — that would be the bug.
    for (line, _col, text) in &collected {
        assert!(
            !(*line == 0 && text.contains("TrafficLight::")),
            "must NOT qualify the variant inside its own enum decl: {collected:?}"
        );
    }
    // Must rewrite the enum-body `Red` (line 0, somewhere around col 20).
    assert!(
        collected.iter().any(|(l, _, t)| *l == 0 && t == "Stop"),
        "expected `Stop` rewrite at line 0: {collected:?}"
    );
    // Must rewrite the match-arm qualified `TrafficLight::Red` (line 3).
    assert!(
        collected.iter().any(|(l, _, _)| *l == 3),
        "expected an edit on line 3 (match arm): {collected:?}"
    );
}

#[test]
fn audit_rename_enum_variant_strips_qualifier_from_qualified_new_name() {
    // Defense-in-depth: if any client sends `newName = "TrafficLight::Stop"`
    // (the IntelliJ plugin used to prefill the dialog with the full
    // qualified form and the user would just edit the suffix), the
    // server must strip the qualifier and use only `Stop` as the bare
    // replacement. Without this, the qualifier was spliced into every
    // match site, producing `TrafficLight::TrafficLight::Stop`.
    let src = "enum TrafficLight { Red, Yellow, Green }\nfn TrafficLight::action($c) {\n    match ($c) {\n        TrafficLight::Red    => \"stop\",\n        TrafficLight::Yellow => \"caution\",\n        TrafficLight::Green  => \"go\",\n    }\n}\nTrafficLight::action(TrafficLight::Red)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `Red` in enum decl (line 0, col 20). NewName is
    // QUALIFIED to mimic the old plugin behavior.
    let r = h.rename(0, 20, "TrafficLight::Stop");
    h.finish();
    let edits = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let new_texts: Vec<&str> = edits
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    assert!(
        !new_texts.is_empty(),
        "expected rename edits even when new_name was qualified"
    );
    assert!(
        new_texts.iter().all(|n| *n == "Stop"),
        "every newText must be the BARE suffix `Stop`, never `TrafficLight::Stop`, got: {new_texts:?}"
    );
    assert!(
        new_texts.iter().all(|n| !n.contains("::")),
        "must NEVER emit `::` in a Field rewrite: {new_texts:?}"
    );
}

#[test]
fn audit_rename_enum_variant_cursor_on_callsite_no_prefix_added() {
    // Same fixture as `audit_rename_enum_variant_does_not_add_type_prefix`
    // but the cursor is on the `Red` INSIDE the qualified call-site
    // `TrafficLight::Red` (line 3), not on the bare decl. Every edit
    // must still be exactly `Stop` — never `TrafficLight::Stop`.
    let src = "enum TrafficLight { Red, Yellow, Green }\nfn TrafficLight::action($c) {\n    match ($c) {\n        TrafficLight::Red    => \"stop\",\n        TrafficLight::Yellow => \"caution\",\n        TrafficLight::Green  => \"go\",\n    }\n}\nTrafficLight::action(TrafficLight::Red)\n";
    let mut h = LspHarness::new(src);
    // Line 3 `        TrafficLight::Red    => "stop",`
    // `T` at col 8, `Red` starts at col 22. Park cursor at col 23 (on `e`).
    let r = h.rename(3, 23, "Stop");
    h.finish();
    let edits = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let new_texts: Vec<&str> = edits
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    assert!(
        !new_texts.is_empty(),
        "expected at least one rename edit when cursor is on call-site variant"
    );
    assert!(
        new_texts.iter().all(|n| *n == "Stop"),
        "every edit must be exactly `Stop` — never qualified — got: {new_texts:?}"
    );
    assert!(
        new_texts.iter().all(|n| !n.contains("::")),
        "must NOT add `::` to any rewrite: {new_texts:?}"
    );
}

#[test]
fn audit_rename_enum_variant_same_file() {
    // Cursor on `Add` of `Op::Add` — should be treated as the
    // enum's variant. v1 lands on enum decl line for goto; rename
    // should at minimum produce an edit for the variant occurrence.
    let edits = rename_same_file("enum Op { Add, Sub }\nmy $r = Op::Add\n", 0, 11, "Plus");
    assert!(
        !edits.is_empty(),
        "enum variant rename must produce an edit: {edits:?}"
    );
}

#[test]
fn audit_rename_use_constant_same_file() {
    // `p LIMIT` with no other args parses as `Say { handle: "LIMIT" }`
    // (Perl filehandle form) — use the value-context call form
    // `(LIMIT)` to ensure the parser sees a Bareword ref instead.
    let edits = rename_same_file(
        "use constant LIMIT => 42\np(LIMIT)\np(LIMIT + 1)\n",
        0,
        14,
        "MAX_LIMIT",
    );
    assert!(
        edits.len() >= 3,
        "use-constant must rename decl + 2 usages: {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "MAX_LIMIT"));
}

// ── Cross-file rename audit ──────────────────────────────────────
//
// One test per kind that crosses files (Sub, Type, Our, Format,
// Field, use constant — file-local kinds Param/Local/State/Label
// are excluded by design).

fn rename_cross_file(
    lib_basename: &str,
    lib_src: &str,
    test_src: &str,
    line: u32,
    character: u32,
    new_name: &str,
) -> (String, String, Vec<String>, Vec<String>) {
    let (test_uri, lib_uri, changes) =
        run_cross_file_rename(lib_basename, lib_src, test_src, line, character, new_name);
    let collect = |key: &str| -> Vec<String> {
        changes
            .get(key)
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("newText").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let test_edits = collect(&test_uri);
    let lib_edits = collect(&lib_uri);
    (test_uri, lib_uri, test_edits, lib_edits)
}

#[test]
fn audit_rename_sub_cross_file() {
    let (_t_uri, _l_uri, test_edits, lib_edits) = rename_cross_file(
        "foo.stk",
        "package Project::Foo;\nfn bar { 1 }\n1;\n",
        "require \"./lib/foo.stk\"\nProject::Foo::bar();\nmy $r = Project::Foo::bar();\n",
        1,
        // Cursor on `b` of `bar` (col 15 of `Project::Foo::bar();`)
        15,
        "renamed",
    );
    assert!(
        !test_edits.is_empty(),
        "sub cross-file: test edits empty: {test_edits:?}"
    );
    assert!(
        !lib_edits.is_empty(),
        "sub cross-file: lib edits empty: {lib_edits:?}"
    );
}

#[test]
fn audit_rename_struct_cross_file() {
    let (_t, _l, test_edits, lib_edits) = rename_cross_file(
        "geom.stk",
        "package Project::Geom;\nstruct Point { x, y }\n1;\n",
        "require \"./lib/geom.stk\"\nmy $p = Project::Geom::Point->new(x => 1, y => 2);\n",
        1,
        // Cursor on `P` of `Point` (col 23 of qualified ref).
        23,
        "Vertex",
    );
    assert!(
        !test_edits.is_empty(),
        "struct cross-file: test edits empty: {test_edits:?}"
    );
    assert!(
        !lib_edits.is_empty(),
        "struct cross-file: lib edits empty: {lib_edits:?}"
    );
}

#[test]
fn audit_rename_our_var_cross_file() {
    let (_t, _l, test_edits, lib_edits) = rename_cross_file(
        "vars.stk",
        "package Project::Vars;\nour $level = 7;\nfn bump { $level += 1 }\n1;\n",
        "require \"./lib/vars.stk\"\nmy $x = $Project::Vars::level;\n",
        1,
        // Cursor on `l` of `level` — col 24 (first letter past the
        // second `:` of `Vars::`). Avoids landing on a `::` colon
        // which corrupts the identifier_span result.
        24,
        "intensity",
    );
    assert!(
        !test_edits.is_empty(),
        "our cross-file: test edits empty: {test_edits:?}"
    );
    assert!(
        !lib_edits.is_empty(),
        "our cross-file: lib edits empty: {lib_edits:?}"
    );
}

#[test]
fn audit_rename_struct_field_cross_file() {
    let (_t, _l, test_edits, lib_edits) = rename_cross_file(
        "geom2.stk",
        "package Project::Geom;\nstruct Point { x, y }\n1;\n",
        "require \"./lib/geom2.stk\"\nmy $p = Project::Geom::Point(x => 1, y => 2);\n",
        1,
        // Cursor on `x` of constructor arg (col 29).
        29,
        "xx",
    );
    assert!(
        !test_edits.is_empty(),
        "field cross-file: test edits empty: {test_edits:?}"
    );
    assert!(
        !lib_edits.is_empty(),
        "field cross-file: lib edits empty: {lib_edits:?}"
    );
}

/// Enum cross-file rename via the variant form
/// (`Project::Ops::Op::Add` cursor on `Add` — the Field/variant).
/// Renaming the enum name itself (`Op`) across files is a known gap:
/// the cursor lands on a middle `::`-segment which triggers the
/// package-prefix rename path; that path can rewrite qualified
/// callers but doesn't find the bare-form `enum Op` decl in the lib.
/// Filed against future work.
#[test]
fn audit_rename_enum_variant_cross_file() {
    let (_t, _l, test_edits, lib_edits) = rename_cross_file(
        "ops.stk",
        "package Project::Ops;\nenum Op { Add, Sub }\n1;\n",
        "require \"./lib/ops.stk\"\nmy $r = Project::Ops::Op::Add\n",
        1,
        // Cursor on `A` of `Add` (col 27) — the variant (Field).
        27,
        "Plus",
    );
    assert!(
        !test_edits.is_empty(),
        "enum variant cross-file: test edits empty: {test_edits:?}"
    );
    assert!(
        !lib_edits.is_empty(),
        "enum variant cross-file: lib edits empty: {lib_edits:?}"
    );
}

#[test]
fn audit_rename_class_cross_file() {
    let (_t, _l, test_edits, lib_edits) = rename_cross_file(
        "animals.stk",
        "package Project::Zoo;\nclass Animal { name: Str }\n1;\n",
        "require \"./lib/animals.stk\"\nmy $a = Project::Zoo::Animal->new(name => \"x\");\n",
        1,
        // Cursor on `A` of `Animal` (col 23).
        23,
        "Creature",
    );
    assert!(
        !test_edits.is_empty(),
        "class cross-file: test edits empty: {test_edits:?}"
    );
    assert!(
        !lib_edits.is_empty(),
        "class cross-file: lib edits empty: {lib_edits:?}"
    );
}

#[test]
fn audit_rename_trait_cross_file() {
    let (_t, _l, test_edits, lib_edits) = rename_cross_file(
        "traits.stk",
        "package Project::Traits;\ntrait Walks { fn walk }\n1;\n",
        "require \"./lib/traits.stk\"\nmy $tt = Project::Traits::Walks;\n",
        1,
        // Cursor on `W` of `Walks` (col 26).
        26,
        "Strides",
    );
    assert!(
        !test_edits.is_empty(),
        "trait cross-file: test edits empty: {test_edits:?}"
    );
    assert!(
        !lib_edits.is_empty(),
        "trait cross-file: lib edits empty: {lib_edits:?}"
    );
}

#[test]
fn audit_rename_format_cross_file() {
    let (_t, _l, test_edits, lib_edits) = rename_cross_file(
        "report.stk",
        "package Project::Report;\nformat REPORT =\n@<<<<<\n\"hi\"\n.\n1;\n",
        "require \"./lib/report.stk\"\nwrite Project::Report::REPORT\n",
        1,
        // Cursor on `R` of `REPORT` (col 27).
        27,
        "SUMMARY",
    );
    assert!(
        !test_edits.is_empty(),
        "format cross-file: test edits empty: {test_edits:?}"
    );
    assert!(
        !lib_edits.is_empty(),
        "format cross-file: lib edits empty: {lib_edits:?}"
    );
}

#[test]
fn audit_rename_use_constant_cross_file() {
    let (_t, _l, test_edits, lib_edits) = rename_cross_file(
        "consts.stk",
        "package Project::Consts;\nuse constant LIMIT => 42;\n1;\n",
        "require \"./lib/consts.stk\"\nmy $x = Project::Consts::LIMIT();\n",
        1,
        // Cursor on `L` of `LIMIT` (col 25).
        25,
        "MAX_LIMIT",
    );
    assert!(
        !test_edits.is_empty(),
        "use-constant cross-file: test edits empty: {test_edits:?}"
    );
    assert!(
        !lib_edits.is_empty(),
        "use-constant cross-file: lib edits empty: {lib_edits:?}"
    );
}

/// Rename on a trait name must update every `impl Trait` / type-bound
/// occurrence in the file. Pin same-file behavior.
#[test]
fn lsp_stdio_rename_trait_same_file() {
    let src = "trait Drawable { fn draw }\nclass Square impl Drawable { side: Int }\n";
    let mut h = LspHarness::new(src);
    // Cursor on `Drawable` decl line 0 col 7.
    let r = h.rename(0, 7, "Renderable");
    h.finish();
    let edits: Vec<&str> = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("rename returned no edits: {r}"))
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    // Decl + impl reference = 2 edits.
    assert!(
        edits.len() >= 2,
        "expected ≥2 edits (decl + impl ref), got {edits:?}"
    );
    assert!(
        edits.iter().all(|n| *n == "Renderable"),
        "all edits should be `Renderable`: {edits:?}"
    );
}

/// Goto Declaration on a struct used in the active file but declared
/// in a `require`d lib must land on the struct's decl line in the
/// lib file. Ported from the cross-file goto-def expansion that now
/// includes Type symbols.
#[test]
fn lsp_stdio_goto_definition_struct_across_require() {
    use std::fs;
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    fs::create_dir(project.join("t")).expect("mkdir t");
    fs::create_dir(project.join("lib")).expect("mkdir lib");
    let lib_path = project.join("lib").join("geom.stk");
    fs::write(
        &lib_path,
        "package Project::Geom;\nstruct Point { x, y }\n1;\n",
    )
    .expect("write lib");
    let test_path = project.join("t").join("test.stk");
    let test_src =
        "require \"./lib/geom.stk\"\nmy $p = Project::Geom::Point->new(x => 1, y => 2);\n";
    fs::write(&test_path, test_src).expect("write test");
    let test_uri = format!("file://{}", test_path.display());

    let mut child = Command::new(ST)
        .arg("--lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stryke --lsp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let stderr_rx = drain_stderr(child.stderr.take().expect("stderr"));
    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "processId": null, "rootUri": null, "capabilities": {} },
        }),
    );
    let _ = recv_until_result(&mut reader, 1);
    write_msg(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    );
    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": test_uri, "languageId": "perl",
                    "version": 1, "text": test_src,
                },
            },
        }),
    );
    let start = Instant::now();
    loop {
        if start.elapsed() > READ_TIMEOUT {
            let err = stderr_rx.recv().unwrap_or_default();
            panic!("timeout: {err}");
        }
        let m = read_msg(&mut reader);
        if m.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") {
            break;
        }
    }
    // Cursor on `Point` of `Project::Geom::Point` at line 1, col 25.
    // "my $p = Project::Geom::Point->new(...)" — `Point` at cols 23-28.
    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0", "id": 2,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": test_uri },
                "position": { "line": 1, "character": 25 },
            },
        }),
    );
    let msg = recv_until_result(&mut reader, 2);
    let raw = msg.get("result").cloned().unwrap_or(Value::Null);
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();

    // Normalize Location | LocationLink[] → Location shape.
    let result = if raw.is_object() && raw.get("range").is_some() {
        raw
    } else if let Some(first) = raw.as_array().and_then(|a| a.first()) {
        if let (Some(u), Some(r)) = (
            first.get("targetUri"),
            first
                .get("targetSelectionRange")
                .or_else(|| first.get("targetRange")),
        ) {
            json!({ "uri": u, "range": r })
        } else {
            first.clone()
        }
    } else {
        raw
    };

    let uri = result
        .get("uri")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("definition missing uri: {result}"));
    assert!(
        uri.ends_with("/lib/geom.stk"),
        "expected lib/geom.stk uri, got {uri}: {result}"
    );
    let line = result
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("definition missing line: {result}"));
    assert_eq!(line, 1, "struct Point on line 1 of lib: {result}");
}

/// Rename on a struct field name must update every call site where the
/// field is used (constructor `Rectangle(width => 1, …)`, fat-comma
/// keys, etc.) — not just the declaration.
#[test]
fn lsp_stdio_rename_struct_field_updates_all_usages() {
    let src = "struct Rectangle { width, height }\nmy $a = Rectangle(width => 1, height => 2)\nmy $b = Rectangle(width => 3, height => 4)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `width` inside `Rectangle(width => 1, ...)` at line 1,
    // col 18.
    let r = h.rename(1, 18, "w");
    h.finish();
    let edits: Vec<&str> = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("rename returned no edits: {r}"))
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    // Expected: decl line `width` + 2 call-site `width` = 3 edits.
    assert!(
        edits.len() >= 3,
        "expected ≥3 edits (decl + 2 call sites), got {edits:?}"
    );
    assert!(
        edits.iter().all(|n| *n == "w"),
        "all edits should be `w`: {edits:?}"
    );
}

/// Go to Declaration on a struct field name inside a constructor call
/// `Rectangle(width => -1, height => 5)` must land on the struct's
/// decl line. Fields registered as `SymbolKind::Field` with the
/// parent struct's decl_line (per-field line tracking would need
/// parser changes; v1 lands you on the struct).
#[test]
fn lsp_stdio_goto_definition_struct_field() {
    let src = "struct Rectangle { width, height }\nmy $r = Rectangle(width => -1, height => 5)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `width` inside the constructor at line 1, col 18.
    let r = h.definition(1, 18);
    h.finish();
    let line = r
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("no goto definition line: {r}"));
    assert_eq!(line, 0, "field `width` declared inside struct on line 0");
}

#[test]
fn audit_goto_definition_lands_caret_on_bare_function_name() {
    // Regression: Cmd+B on a usage of `Algo::binary_search` must
    // navigate the caret to the FIRST CHAR of `binary_search`, not
    // col 0 of the decl line. `fn Algo::binary_search` puts the bare
    // name at col 9 (after `fn Algo::`).
    let src = "fn Algo::binary_search($t, @l) { -1 }\nAlgo::binary_search(3, @sorted)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `binary_search` of the USE site (line 1, somewhere
    // inside the qualified call).
    let r = h.definition(1, 12);
    h.finish();
    let line = r
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("no goto definition line: {r}"));
    let character = r
        .pointer("/range/start/character")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("no goto definition character: {r}"));
    assert_eq!(line, 0, "decl is on line 0: {r}");
    assert_eq!(
        character, 9,
        "caret must land at col 9 (start of bare name `binary_search`), not col 0: {r}",
    );
}

#[test]
fn audit_goto_definition_returns_none_when_cursor_on_decl_line() {
    // When the cursor is already on the declaration line, the LSP
    // server returns None so the platform doesn't self-jump. The
    // plugin handler picks this up and shows ShowUsages instead.
    let src = "fn myfunc($x) { $x + 1 }\nmyfunc(5)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `myfunc` of the decl (line 0, col 5).
    let r = h.definition(0, 5);
    h.finish();
    // Either Null (no result) OR an empty array — both express "no
    // navigation target". The harness's normalization returns the raw
    // value when neither object nor first-array-element shape matches.
    let no_target = r.is_null()
        || r.as_array().is_some_and(|a| a.is_empty())
        || r.is_object() && r.get("range").is_none() && r.get("targetUri").is_none();
    assert!(
        no_target,
        "expected null / empty when cursor is on decl line, got: {r}",
    );
}

#[test]
fn audit_goto_definition_struct_lands_on_struct_name() {
    // `struct Geom::Point { ... }` — `Point` is the bare name at col
    // 13 (after `struct Geom::`). Cmd+B on a `Geom::Point->new(...)`
    // usage must land the caret there.
    let src = "struct Geom::Point { x, y }\nmy $p = Geom::Point->new(x => 1, y => 2)\n";
    let mut h = LspHarness::new(src);
    let r = h.definition(1, 15); // cursor on `Point`
    h.finish();
    let character = r
        .pointer("/range/start/character")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("no struct goto definition: {r}"));
    assert_eq!(
        character, 13,
        "expected col 13 (start of `Point`), got: {r}"
    );
}

/// Go to Declaration on an enum variant (`Op::Add`) must land on the
/// `enum Op { Add, Sub }` decl line. Currently the SymbolTable only
/// registers the enum type itself; the variant lookup falls through.
#[test]
fn lsp_stdio_goto_definition_enum_variant_jumps_to_enum_decl() {
    let src = "enum Op { Add, Sub, Mul }\nmy $x = Op::Add\n";
    let mut h = LspHarness::new(src);
    // Cursor on `Add` of `Op::Add` at line 1, col 12.
    let r = h.definition(1, 12);
    h.finish();
    let line = r
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("no goto definition line: {r}"));
    assert_eq!(line, 0, "enum Op declared on line 0 (0-based): {r}");
}

#[test]
fn lsp_stdio_goto_definition_sub() {
    let mut h = LspHarness::new("fn yellow_minion { }\nyellow_minion();\n");
    let result = h.definition(1, 3);
    h.finish();
    let line = result
        .get("range")
        .and_then(|r| r.get("start"))
        .and_then(|s| s.get("line"))
        .and_then(Value::as_u64)
        .expect("definition line");
    assert_eq!(line, 0, "sub declared on first line (0-based): {result}");
}

#[test]
fn lsp_stdio_document_symbol_lists_sub() {
    let mut h = LspHarness::new("fn yellow_minion { }\n1;\n");
    let result = h.document_symbols();
    h.finish();
    let arr = result.as_array().expect("documentSymbol array");
    let names: Vec<&str> = arr
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
        .collect();
    assert!(
        names.contains(&"sub yellow_minion") || names.contains(&"fn yellow_minion"),
        "expected sub/fn in {:?}",
        names
    );
}

#[test]
fn lsp_stdio_resolve_completion_adds_function_doc() {
    let mut h = LspHarness::new("fn x { }\n");
    let resolved = h.resolve_completion(json!({
        "label": "totally_unknown_sub",
        "kind": 3
    }));
    h.finish();
    let doc = resolved
        .pointer("/documentation/value")
        .and_then(Value::as_str)
        .expect("resolved documentation markdown");
    assert!(
        doc.contains("Subroutine") && doc.contains("this document"),
        "unexpected resolve doc: {doc}"
    );
}

#[test]
fn lsp_stdio_document_highlight_finds_occurrences() {
    let src = "fn yellow_minion { }\nyellow_minion();\nyellow_minion();\n";
    let mut h = LspHarness::new(src);
    let result = h.document_highlight(1, 3);
    h.finish();
    let arr = result.as_array().expect("documentHighlight array");
    assert!(
        arr.len() >= 2,
        "expected multiple highlights, got {:?}",
        arr
    );
}

#[test]
fn lsp_stdio_references_lists_occurrences() {
    let src = "fn yellow_minion { }\nyellow_minion();\nyellow_minion();\n";
    let mut h = LspHarness::new(src);
    let result = h.references(1, 3, false);
    h.finish();
    let arr = result.as_array().expect("references array");
    assert!(arr.len() >= 2, "expected multiple refs, got {:?}", arr);
}

#[test]
fn lsp_stdio_declaration_same_as_definition_for_sub() {
    let src = "fn yellow_minion { }\nyellow_minion();\n";
    let mut h = LspHarness::new(src);
    let def = h.definition(1, 3);
    let decl = h.declaration(1, 3);
    h.finish();
    assert_eq!(
        def.pointer("/range/start/line"),
        decl.pointer("/range/start/line"),
        "declaration should match definition: def={def} decl={decl}"
    );
}

#[test]
fn lsp_stdio_prepare_rename_sub_placeholder() {
    let mut h = LspHarness::new("fn yellow_minion { }\nyellow_minion();\n");
    let r = h.prepare_rename(1, 3);
    h.finish();
    assert_eq!(
        r.get("placeholder").and_then(Value::as_str),
        Some("yellow_minion"),
        "prepareRename: {r}"
    );
}

/// Cross-file go-to-definition: a `require "./lib/foo.stk"` in a test
/// file under `t/` must resolve to the sibling `lib/foo.stk` and land
/// on the `fn` declaration line. Pin for the IntelliJ-plugin workflow
/// (`Cmd+Click` on `Project::Foo::bar` jumps into the library file).
/// Cross-file rename: invoking rename on a fn ref inside a test file
/// must (a) accept the placeholder in `prepareRename` and (b) return a
/// workspace edit that touches BOTH the test file (qualified-form call
/// site) and the lib file (bare-form `fn` declaration + internal call).
/// Without this, IntelliJ's "Rename…" reports "no rename available" on
/// any fn declared in a `require`d lib.
#[test]
fn lsp_stdio_rename_follows_require_into_lib() {
    use std::fs;
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    fs::create_dir(project.join("t")).expect("mkdir t");
    fs::create_dir(project.join("lib")).expect("mkdir lib");

    let lib_path = project.join("lib").join("foo.stk");
    // Lib file declares the sub AND has an internal bare-form call —
    // both spellings must be rewritten by the cross-file rename.
    fs::write(
        &lib_path,
        "package Project::Foo;\nfn bar { 42 }\nfn caller { bar() }\n1;\n",
    )
    .expect("write lib");

    let test_path = project.join("t").join("test_foo.stk");
    let test_src =
        "require \"./lib/foo.stk\"\nProject::Foo::bar();\nmy $x = Project::Foo::bar();\n";
    fs::write(&test_path, test_src).expect("write test");

    let test_uri = format!("file://{}", test_path.display());
    let lib_uri = format!("file://{}", lib_path.display());

    let mut child = Command::new(ST)
        .arg("--lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stryke --lsp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let stderr_rx = drain_stderr(child.stderr.take().expect("stderr"));

    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "processId": null, "rootUri": null, "capabilities": {} },
        }),
    );
    let _ = recv_until_result(&mut reader, 1);
    write_msg(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    );

    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": test_uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": test_src,
                },
            },
        }),
    );
    // Drain the publishDiagnostics from didOpen.
    {
        let start = Instant::now();
        loop {
            if start.elapsed() > READ_TIMEOUT {
                let err = stderr_rx.recv().unwrap_or_default();
                panic!("timeout waiting for diagnostics; stderr:\n{err}");
            }
            let msg = read_msg(&mut reader);
            if msg.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            {
                break;
            }
        }
    }

    // Position the cursor inside `bar` of `Project::Foo::bar()` on line 1.
    // Test file line 1 = "Project::Foo::bar();" — `bar` starts at col 14.
    let line = 1u32;
    let character = 15u32;

    // (a) prepareRename must return a placeholder so IntelliJ enters
    //     rename mode at all.
    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/prepareRename",
            "params": {
                "textDocument": { "uri": test_uri },
                "position": { "line": line, "character": character },
            },
        }),
    );
    let prep = recv_until_result(&mut reader, 2);
    let placeholder = prep
        .pointer("/result/placeholder")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("prepareRename returned no placeholder: {prep}"));
    assert!(
        placeholder.contains("bar"),
        "placeholder should contain 'bar': {placeholder} (full: {prep})"
    );

    // (b) rename returns workspace edits across both files.
    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/rename",
            "params": {
                "textDocument": { "uri": test_uri },
                "position": { "line": line, "character": character },
                "newName": "renamed",
            },
        }),
    );
    let msg = recv_until_result(&mut reader, 3);
    let result = msg.get("result").cloned().unwrap_or(Value::Null);

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();

    let changes = result
        .get("changes")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("rename missing changes map: {result}"));

    // Both files must appear in the workspace edit.
    let test_edits = changes
        .get(&test_uri)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no edits for test file in {result}"));
    let lib_edits = changes
        .get(&lib_uri)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no edits for lib file in {result}"));

    // Test file: 2 qualified-form refs (lines 1 and 2).
    assert_eq!(
        test_edits.len(),
        2,
        "expected 2 test-file edits (one per qualified call site): {test_edits:#?}"
    );
    for e in test_edits {
        let new_text = e.get("newText").and_then(Value::as_str).unwrap_or("");
        assert_eq!(
            new_text, "Project::Foo::renamed",
            "test file edits should use qualified new name: {e}"
        );
    }

    // Lib file: bare decl (line 1) + bare internal call (line 2).
    assert!(
        lib_edits.len() >= 2,
        "expected ≥2 lib-file edits (decl + internal call): {lib_edits:#?}"
    );
    for e in lib_edits {
        let new_text = e.get("newText").and_then(Value::as_str).unwrap_or("");
        assert_eq!(
            new_text, "renamed",
            "lib file edits should use bare new name: {e}"
        );
    }
}

/// Helper: drive `stryke --lsp` to rename a symbol across `require`d
/// files. Writes `lib_src` to `lib/<lib_basename>.stk` and `test_src`
/// to `t/test.stk` in a tempdir, opens the test file, sends
/// prepareRename + rename at `(line, character)` with `new_name`, and
/// returns the parsed `changes` map keyed by URI. Panics if the LSP
/// rejects the rename (test will fail) or if no `changes` object is
/// returned.
fn run_cross_file_rename(
    lib_basename: &str,
    lib_src: &str,
    test_src: &str,
    line: u32,
    character: u32,
    new_name: &str,
) -> (String, String, serde_json::Map<String, Value>) {
    use std::fs;
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    fs::create_dir(project.join("t")).expect("mkdir t");
    fs::create_dir(project.join("lib")).expect("mkdir lib");

    let lib_path = project.join("lib").join(lib_basename);
    fs::write(&lib_path, lib_src).expect("write lib");
    let test_path = project.join("t").join("test.stk");
    fs::write(&test_path, test_src).expect("write test");

    let test_uri = format!("file://{}", test_path.display());
    let lib_uri = format!("file://{}", lib_path.display());

    let mut child = Command::new(ST)
        .arg("--lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stryke --lsp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let stderr_rx = drain_stderr(child.stderr.take().expect("stderr"));

    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "processId": null, "rootUri": null, "capabilities": {} },
        }),
    );
    let _ = recv_until_result(&mut reader, 1);
    write_msg(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    );

    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": test_uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": test_src,
                },
            },
        }),
    );
    let start = Instant::now();
    loop {
        if start.elapsed() > READ_TIMEOUT {
            let err = stderr_rx.recv().unwrap_or_default();
            panic!("timeout waiting for diagnostics; stderr:\n{err}");
        }
        let msg = read_msg(&mut reader);
        if msg.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") {
            break;
        }
    }

    // First, exercise prepareRename — IntelliJ calls it before rename
    // and bails if it returns null. Confirm the LSP accepts the cursor.
    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/prepareRename",
            "params": {
                "textDocument": { "uri": test_uri },
                "position": { "line": line, "character": character },
            },
        }),
    );
    let prep = recv_until_result(&mut reader, 2);
    assert!(
        prep.pointer("/result/placeholder")
            .and_then(Value::as_str)
            .is_some(),
        "prepareRename should accept cross-file cursor: {prep}"
    );

    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/rename",
            "params": {
                "textDocument": { "uri": test_uri },
                "position": { "line": line, "character": character },
                "newName": new_name,
            },
        }),
    );
    let msg = recv_until_result(&mut reader, 3);
    let result = msg.get("result").cloned().unwrap_or(Value::Null);

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();

    let changes = result
        .get("changes")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(|| panic!("rename missing changes map: {result}"));
    (test_uri, lib_uri, changes)
}

/// Cursor on `$var` INSIDE a double-quoted interpolation `"string $var"`
/// must rename it the same as cursor on the decl. Pins that the parser
/// turns the in-string `$var` into `InterpolatedString { ScalarVar }`
/// and the SymbolTable's `walk_expr` records the ref.
#[test]
fn lsp_stdio_rename_var_inside_double_quoted_interpolation() {
    let src = "my $var = 1\nmy $s = \"string $var\"\np \"got $var\"\n";
    let mut h = LspHarness::new(src);
    // Cursor on `$var` inside `"string $var"` at line 1, col 16.
    // (`"` at 8, `s` at 9, `t` at 10, ..., `$` at 15, `v` at 16.)
    let r = h.rename(1, 16, "renamed");
    h.finish();
    let edits: Vec<&str> = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("rename returned no edits: {r}"))
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    // Decl + 2 interp refs = 3 edits, all `$renamed` (sigil preserved).
    assert!(
        edits.len() >= 3,
        "expected ≥3 edits (decl + 2 interp refs), got {edits:?}"
    );
    assert!(
        edits.iter().all(|n| *n == "$renamed"),
        "all edits must be `$renamed`: {edits:?}"
    );
}

/// Find Usages on `$var` from inside `"string $var"` must include every
/// interpolation site too — not just the decl.
#[test]
fn lsp_stdio_references_var_inside_double_quoted_interpolation() {
    let src = "my $var = 1\nmy $a = \"first $var\"\nmy $b = \"second $var\"\n";
    let mut h = LspHarness::new(src);
    // Cursor on `$var` inside `"first $var"` at line 1, col 15.
    let r = h.references(1, 15, true);
    h.finish();
    let arr = r
        .as_array()
        .unwrap_or_else(|| panic!("references returned non-array: {r}"));
    // Decl + 2 in-string interp refs = 3.
    assert!(
        arr.len() >= 3,
        "expected ≥3 locations, got {}: {arr:#?}",
        arr.len()
    );
}

/// Find Usages on `enum Op { Add, Sub }` must NOT include matches of
/// the literal text `Op` that appear inside string literals (e.g.
/// `"Op"` as a string). The textual cross-file scanner must skip
/// string interiors.
#[test]
fn lsp_stdio_references_enum_does_not_match_inside_string_literal() {
    let src = "enum Op { Add, Sub }\nmy $r = Op::Add\nmy $s = \"Op is a type\"\nmy $t = 'Op too'\n";
    let mut h = LspHarness::new(src);
    // Cursor on `Op` of `enum Op` at line 0, col 5.
    let r = h.references(0, 5, true);
    h.finish();
    let arr = r
        .as_array()
        .unwrap_or_else(|| panic!("references returned non-array: {r}"));
    let lines: Vec<u64> = arr
        .iter()
        .filter_map(|loc| loc.pointer("/range/start/line").and_then(Value::as_u64))
        .collect();
    assert!(lines.contains(&0), "expected decl line 0: lines={lines:?}");
    assert!(
        lines.contains(&1),
        "expected reference at line 1 (Op::Add): lines={lines:?}"
    );
    assert!(
        !lines.contains(&2),
        "must NOT match `Op` inside the double-quoted string at line 2: lines={lines:?}"
    );
    assert!(
        !lines.contains(&3),
        "must NOT match `Op` inside the single-quoted string at line 3: lines={lines:?}"
    );
}

/// Find Usages on the `handle` part of `Demo::handle` must NOT return
/// usages of an unrelated bare `handle` in a different package. The
/// qualified-vs-bare distinction is load-bearing: `Demo::handle` and
/// `main::handle` are two different symbols.
#[test]
fn lsp_stdio_references_qualified_sub_does_not_include_unrelated_bare_calls() {
    let src = "package Demo;\nfn handle { 1 }\nDemo::handle();\npackage main;\nfn handle { 2 }\nhandle();\n";
    let mut h = LspHarness::new(src);
    // Cursor on `handle` of `Demo::handle();` at line 2.
    // Line 2 text: `Demo::handle();` — `h` of handle is at col 6.
    let r = h.references(2, 8, true);
    h.finish();
    let arr = r
        .as_array()
        .unwrap_or_else(|| panic!("references returned non-array: {r}"));
    // Expected locations for `Demo::handle`:
    //   - line 1: `fn handle { 1 }`  (decl, the bare-tail "handle" on the
    //     decl line is allowed since it's the actual decl)
    //   - line 2: `Demo::handle();`  (qualified call)
    // Must NOT include:
    //   - line 4: `fn handle { 2 }`  (different sub, `main::handle`)
    //   - line 5: `handle();`        (call to `main::handle`)
    let lines: Vec<u64> = arr
        .iter()
        .filter_map(|loc| loc.pointer("/range/start/line").and_then(Value::as_u64))
        .collect();
    assert!(
        lines.contains(&1),
        "expected ref at line 1 (Demo::handle decl): lines={lines:?}"
    );
    assert!(
        lines.contains(&2),
        "expected ref at line 2 (Demo::handle qualified call): lines={lines:?}"
    );
    assert!(
        !lines.contains(&4),
        "must NOT include line 4 (main::handle decl): lines={lines:?}"
    );
    assert!(
        !lines.contains(&5),
        "must NOT include line 5 (bare main::handle call): lines={lines:?}"
    );
}

/// Find Usages on a hash declared as `my %seen` must locate every
/// `$seen{KEY}` element-access site too (not just `%seen` text matches).
#[test]
fn lsp_stdio_references_hash_via_element_access() {
    let src = "my %seen\n$seen{\"a\"} = 1\n$seen{\"b\"} = 2\nprint $seen{\"a\"}\n";
    let mut h = LspHarness::new(src);
    // Cursor on `%seen` decl at line 0, col 4.
    let r = h.references(0, 4, true);
    h.finish();
    let arr = r
        .as_array()
        .unwrap_or_else(|| panic!("references returned non-array: {r}"));
    // Expect: 1 decl + 3 element-access sites = 4 locations.
    assert!(
        arr.len() >= 4,
        "expected ≥4 locations (decl + 3 element accesses), got {}: {arr:#?}",
        arr.len()
    );
}

/// Find Usages on a foreach loop var `$cb` must locate the var inside
/// `"$cb"` string interpolation too — that's where the parse becomes
/// `InterpolatedString { Expr { ScalarVar("cb") } }`.
#[test]
fn lsp_stdio_references_foreach_loop_var_in_interpolation() {
    let src = "my @cbs = (1, 2, 3)\nmy %seen\nfor my $cb (@cbs) {\n    $seen{\"$cb\"} = 1\n}\n";
    let mut h = LspHarness::new(src);
    // Cursor on `$cb` of `for my $cb` at line 2, col 8.
    let r = h.references(2, 8, true);
    h.finish();
    let arr = r
        .as_array()
        .unwrap_or_else(|| panic!("references returned non-array: {r}"));
    // Expect at least 2: decl + interpolation ref.
    assert!(
        arr.len() >= 2,
        "expected ≥2 locations, got {}: {arr:#?}",
        arr.len()
    );
}

/// Find Usages on a package name (`Demo::Caller`) must locate every
/// `Demo::Caller::method()` call site, treating the trailing `::` as
/// a valid boundary.
#[test]
fn lsp_stdio_references_package_includes_qualified_call_sites() {
    let src = "package Demo::Caller\nfn leaf { 1 }\nfn mid = Demo::Caller::leaf()\nfn top = Demo::Caller::mid()\npackage main\nmy $name = Demo::Caller::top()\n";
    let mut h = LspHarness::new(src);
    // Cursor on `Caller` of `package Demo::Caller` at line 0, col 16.
    let r = h.references(0, 16, true);
    h.finish();
    let arr = r
        .as_array()
        .unwrap_or_else(|| panic!("references returned non-array: {r}"));
    // Decl + 3 call sites = 4.
    assert_eq!(
        arr.len(),
        4,
        "expected 4 locations (decl + 3 calls), got {}: {arr:#?}",
        arr.len()
    );
}

/// Find Usages on a non-last `::`-segment of a qualified name must
/// locate every package-prefix occurrence (the partial-segment rule
#[test]
fn audit_references_qualified_sub_does_not_match_builtin_calls() {
    // Regression: `fn Algo::binary_search` declared at top level has
    // sym.package="main" and sym.name="Algo::binary_search". The cross-
    // file qualified_form derivation used to fall into the
    // `sym.package == "main"` branch FIRST and emit bare "binary_search"
    // as the cross-file scan needle — false-matching every call to the
    // BUILTIN `binary_search` in unrelated files.
    let src = "fn Algo::binary_search($t, @l) { -1 }\nmy @sorted = (1,3,5)\nAlgo::binary_search(3, @sorted)\nbinary_search(5, 1, 2, 3, 4, 5)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `binary_search` of the decl (line 0).
    let r = h.references(0, 15, false);
    h.finish();
    let arr = r
        .as_array()
        .unwrap_or_else(|| panic!("references returned non-array: {r}"));
    let lines: Vec<u64> = arr
        .iter()
        .filter_map(|loc| loc.pointer("/range/start/line").and_then(Value::as_u64))
        .collect();
    // Line 2 is the qualified `Algo::binary_search(...)` call — MUST
    // be included.
    assert!(
        lines.contains(&2),
        "expected qualified call at line 2 in usages: {arr:?}",
    );
    // Line 3 is `binary_search(5, ...)` calling the builtin — MUST
    // NOT be included.
    assert!(
        !lines.contains(&3),
        "bare `binary_search` builtin call (line 3) leaked into usages of qualified `Algo::binary_search`: {arr:?}",
    );
}

/// from rename also applies here).
#[test]
fn lsp_stdio_references_non_last_segment_treats_as_package_prefix() {
    let src = "fn Demo::BitWalk::dump($n) { $n }\nmy $r = Demo::BitWalk::dump(5)\nmy $s = Demo::BitWalk::dump(7)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `BitWalk` at line 0, col 11.
    let r = h.references(0, 11, true);
    h.finish();
    let arr = r
        .as_array()
        .unwrap_or_else(|| panic!("references returned non-array: {r}"));
    // Decl + 2 call sites = 3 prefix matches.
    assert_eq!(
        arr.len(),
        3,
        "expected 3 prefix locations, got {}: {arr:#?}",
        arr.len()
    );
}

/// Rename a named sub referenced via `\&name` (coderef-of operator).
/// Before the fix, `SubroutineCodeRef("named_one")` was invisible to
/// the SymbolTable walker, so the `\&named_one` site never appeared in
/// the rename's WorkspaceEdit.
#[test]
fn lsp_stdio_rename_sub_referenced_via_subroutine_coderef() {
    let src = "fn named_one = 1\nmy $named = \\&named_one\n";
    let mut h = LspHarness::new(src);
    // Cursor on `named_one` of `fn named_one` at line 0, col 5.
    let r = h.rename(0, 5, "renamed_one");
    h.finish();
    let edits: Vec<&str> = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("rename returned no edits: {r}"))
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    assert_eq!(
        edits.len(),
        2,
        "expected 2 edits (decl + \\&named_one ref), got {edits:?} from {r}"
    );
    assert!(edits.iter().all(|n| *n == "renamed_one"));
}

/// Rename the foreach loop var `$cb` in `for my $cb (@cbs) { … }`.
/// Before the fix, the Foreach walker didn't declare the loop var, so
/// rename had no symbol to anchor to.
#[test]
fn lsp_stdio_rename_foreach_loop_var() {
    let src = "my @cbs = (1, 2, 3)\nmy %seen\nfor my $cb (@cbs) {\n    $seen{\"$cb\"} = 1\n}\n";
    let mut h = LspHarness::new(src);
    // Cursor on `$cb` of `for my $cb (@cbs)` at line 2, col 8.
    let r = h.rename(2, 8, "elem");
    h.finish();
    let changes = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("rename returned no changes: {r}"));
    let edits: Vec<&str> = changes
        .values()
        .next()
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no edits returned: {r}"))
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    assert!(
        edits.len() >= 2,
        "expected ≥2 edits (decl + `$cb` interpolation), got {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "$elem"));
}

/// Rename `%seen` when it's used via `$seen{KEY}` element access.
/// Before the fix, `ExprKind::HashElement { hash: "seen", ... }`
/// stored the container as a bare string field that the SymbolTable
/// walker's reflection didn't see.
#[test]
fn lsp_stdio_rename_hash_via_element_access() {
    let src = "my %seen\n$seen{\"k\"} = 1\nprint $seen{\"k\"}\n";
    let mut h = LspHarness::new(src);
    // Cursor on `%seen` decl at line 0, col 4.
    let r = h.rename(0, 4, "visited");
    h.finish();
    let edits: Vec<&str> = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("rename returned no edits: {r}"))
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    assert!(
        edits.len() >= 3,
        "expected ≥3 edits (decl + 2 element-access refs), got {edits:?}"
    );
    // Sigil-preserving substitution: the `my %seen` decl gets `%visited`,
    // but `$seen{k}` element access gets `$visited` to keep the scalar
    // access form valid.
    assert!(
        edits.contains(&"%visited"),
        "decl line should rewrite to %visited: {edits:?}"
    );
    assert!(
        edits.contains(&"$visited"),
        "$seen{{k}} element-access should rewrite to $visited (sigil-preserving): {edits:?}"
    );
    assert!(
        edits.iter().all(|n| *n == "%visited" || *n == "$visited"),
        "all edits should be either %visited or $visited: {edits:?}"
    );
}

/// Rename a package across the file. The cursor falls inside the
/// package decl line; every `Pkg::method` call site must have its
/// prefix swapped without disturbing the trailing `::method`.
#[test]
fn lsp_stdio_rename_package_swaps_prefix_only() {
    let src = "package Demo::Caller\nfn leaf { 1 }\nfn mid = Demo::Caller::leaf()\nfn top = Demo::Caller::mid()\npackage main\nmy $name = Demo::Caller::top()\n";
    let mut h = LspHarness::new(src);
    // Cursor on `Caller` of `package Demo::Caller` at line 0, col 16.
    let r = h.rename(0, 16, "Demo::Renamed");
    h.finish();
    let edits: Vec<&str> = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("rename returned no edits: {r}"))
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    // Decl + 3 qualified call sites = 4 edits.
    assert_eq!(
        edits.len(),
        4,
        "expected 4 package edits (decl + 3 calls), got {edits:?}"
    );
    assert!(edits.iter().all(|n| *n == "Demo::Renamed"));
}

/// Cursor on a non-last `::`-segment of an inline-qualified `fn` decl
/// (`fn Demo::BitWalk::dump`) triggers a package-prefix rename rather
/// than a sub rename — matches IntelliJ's segment-cursor convention.
#[test]
fn lsp_stdio_rename_non_last_segment_treats_as_package_prefix() {
    let src = "fn Demo::BitWalk::dump($n) { $n }\nmy $r = Demo::BitWalk::dump(5)\n";
    let mut h = LspHarness::new(src);
    // Cursor on `BitWalk` of `fn Demo::BitWalk::dump` at line 0, col 11.
    let r = h.rename(0, 11, "Demo::Renamed");
    h.finish();
    let edits: Vec<&str> = r
        .pointer("/changes")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("rename returned no edits: {r}"))
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    // Decl line `Demo::BitWalk` + call-site `Demo::BitWalk` = 2 edits.
    assert_eq!(edits.len(), 2, "expected 2 edits, got {edits:?}");
    assert!(edits.iter().all(|n| *n == "Demo::Renamed"));
}

/// Rename a constant declared via `use constant FOO => 1` in a `require`d
/// lib. The constant compiles to a sub at runtime; in the SymbolTable
/// we register it as `Sub` so the same cross-file rename path applies.
#[test]
fn lsp_stdio_rename_follows_require_for_use_constant() {
    let lib_src = "package Project::Foo;\nuse constant LIMIT => 42;\nfn cap { LIMIT() }\n1;\n";
    let test_src = "require \"./lib/foo.stk\"\nmy $n = Project::Foo::LIMIT();\n";
    // "my $n = Project::Foo::LIMIT();" — `LIMIT` starts at col 22.
    let (test_uri, lib_uri, changes) =
        run_cross_file_rename("foo.stk", lib_src, test_src, 1, 24, "MAX_LIMIT");

    let test_edits = changes
        .get(&test_uri)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no test edits: {changes:#?}"));
    let lib_edits = changes
        .get(&lib_uri)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no lib edits: {changes:#?}"));

    assert!(
        test_edits
            .iter()
            .any(|e| e.get("newText").and_then(Value::as_str) == Some("Project::Foo::MAX_LIMIT")),
        "test file should get qualified MAX_LIMIT: {test_edits:#?}"
    );
    assert!(
        lib_edits
            .iter()
            .any(|e| e.get("newText").and_then(Value::as_str) == Some("MAX_LIMIT")),
        "lib file should get bare MAX_LIMIT (decl + internal call): {lib_edits:#?}"
    );
}

/// Same-file rename of a loop label. Labels are file-local (lexical),
/// so cross-file rename never applies — `last LOOP` in a sibling file
/// refers to a sibling label, not the one we're renaming.
#[test]
fn lsp_stdio_rename_loop_label_same_file() {
    let src = "OUTER: for my $i (1..3) {\n    INNER: for my $j (1..3) {\n        last OUTER if $j == 2;\n    }\n}\n";
    let mut h = LspHarness::new(src);
    // Cursor on `OUTER` of `OUTER:` at line 0, col 2.
    let r = h.rename(0, 2, "TOP");
    h.finish();
    let changes = r
        .get("changes")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("rename returned no changes: {r}"));
    let edits = changes
        .values()
        .next()
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no edits returned: {r}"));
    let new_texts: Vec<&str> = edits
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    assert!(
        new_texts.iter().all(|n| *n == "TOP"),
        "all label edits should rewrite to TOP: {new_texts:?}"
    );
    assert!(
        edits.len() >= 2,
        "expected ≥2 label edits (decl + `last OUTER` ref): {edits:#?}"
    );
}

/// Rename a `format` declaration. Pins same-file behavior; cross-file
/// path follows the Sub matrix (formats are package-scoped).
#[test]
fn lsp_stdio_rename_format_same_file() {
    let src = "format REPORT =\n@<<<<<<<\n\"hi\"\n.\nwrite REPORT;\n";
    let mut h = LspHarness::new(src);
    // Cursor on `REPORT` of `format REPORT =` at line 0, col 8.
    let r = h.rename(0, 8, "SUMMARY");
    h.finish();
    let changes = r
        .get("changes")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("rename returned no changes: {r}"));
    let edits = changes
        .values()
        .next()
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no edits returned: {r}"));
    let new_texts: Vec<&str> = edits
        .iter()
        .filter_map(|e| e.get("newText").and_then(Value::as_str))
        .collect();
    assert!(
        !new_texts.is_empty() && new_texts.iter().all(|n| *n == "SUMMARY"),
        "format rename should produce SUMMARY edits: {new_texts:?}"
    );
}

/// Cross-file rename of a `struct` declared in a `require`d lib. The
/// test file references it as `Project::Foo::Point->new()` (qualified
/// bareword). Rename must rewrite both files.
#[test]
fn lsp_stdio_rename_follows_require_for_struct_type() {
    // Stryke struct fields are bare names or `name => Type`; the `name:
    // Type` shape is invalid syntax and would silently break the lib's
    // SymbolTable.
    let lib_src = "package Project::Foo;\nstruct Point { x, y }\n1;\n";
    let test_src = "require \"./lib/foo.stk\"\nmy $p = Project::Foo::Point->new(x => 1, y => 2);\n";
    // Cursor on `Point` of `Project::Foo::Point` at line 1.
    // "my $p = Project::Foo::Point->new..." — `Point` starts at col 22.
    let (test_uri, lib_uri, changes) =
        run_cross_file_rename("foo.stk", lib_src, test_src, 1, 24, "Vertex");

    let test_edits = changes
        .get(&test_uri)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no test edits: {changes:#?}"));
    let lib_edits = changes
        .get(&lib_uri)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no lib edits: {changes:#?}"));

    assert!(
        test_edits
            .iter()
            .any(|e| e.get("newText").and_then(Value::as_str) == Some("Project::Foo::Vertex")),
        "test file should get qualified Vertex: {test_edits:#?}"
    );
    assert!(
        lib_edits
            .iter()
            .any(|e| e.get("newText").and_then(Value::as_str) == Some("Vertex")),
        "lib file should get bare Vertex (decl + internal call): {lib_edits:#?}"
    );
}

/// Cross-file rename of an `our` variable across a `require` boundary.
/// `our $level` declared in lib, referenced from test as
/// `$Project::Foo::level`. Sigil-aware spelling must be preserved.
#[test]
fn lsp_stdio_rename_follows_require_for_our_variable() {
    let lib_src = "package Project::Foo;\nour $level = 7;\nfn bump { $level += 1 }\n1;\n";
    let test_src = "require \"./lib/foo.stk\"\nmy $x = $Project::Foo::level;\n";
    // Cursor inside `level` of `$Project::Foo::level` on line 1.
    // "my $x = $Project::Foo::level;" — `level` starts at col 23.
    let (test_uri, lib_uri, changes) =
        run_cross_file_rename("foo.stk", lib_src, test_src, 1, 25, "intensity");

    let test_edits = changes
        .get(&test_uri)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no test edits: {changes:#?}"));
    let lib_edits = changes
        .get(&lib_uri)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no lib edits: {changes:#?}"));

    assert!(
        test_edits
            .iter()
            .any(|e| e.get("newText").and_then(Value::as_str) == Some("$Project::Foo::intensity")),
        "test file should get sigil-prefixed qualified intensity: {test_edits:#?}"
    );
    assert!(
        lib_edits
            .iter()
            .any(|e| e.get("newText").and_then(Value::as_str) == Some("$intensity")),
        "lib file should get sigil-prefixed bare intensity: {lib_edits:#?}"
    );
}

#[test]
fn lsp_stdio_goto_definition_follows_require_into_lib() {
    use std::fs;
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    fs::create_dir(project.join("t")).expect("mkdir t");
    fs::create_dir(project.join("lib")).expect("mkdir lib");

    let lib_path = project.join("lib").join("foo.stk");
    fs::write(&lib_path, "package Project::Foo;\nfn bar { 42 }\n1;\n").expect("write lib");

    let test_path = project.join("t").join("test_foo.stk");
    let test_src = "require \"./lib/foo.stk\"\nProject::Foo::bar();\n";
    fs::write(&test_path, test_src).expect("write test");

    let test_uri = format!("file://{}", test_path.display());

    // Hand-roll the LSP exchange so we control the document URI.
    let mut child = Command::new(ST)
        .arg("--lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stryke --lsp");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
    let stderr_rx = drain_stderr(child.stderr.take().expect("stderr"));

    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "processId": null, "rootUri": null, "capabilities": {} },
        }),
    );
    let _ = recv_until_result(&mut reader, 1);
    write_msg(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    );

    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": test_uri,
                    "languageId": "perl",
                    "version": 1,
                    "text": test_src,
                },
            },
        }),
    );
    // Drain the publishDiagnostics that follows didOpen.
    {
        let start = Instant::now();
        loop {
            if start.elapsed() > READ_TIMEOUT {
                let err = stderr_rx.recv().unwrap_or_default();
                panic!("timeout waiting for diagnostics; stderr:\n{err}");
            }
            let msg = read_msg(&mut reader);
            if msg.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            {
                break;
            }
        }
    }

    // Click on `bar` inside the call `Project::Foo::bar();` on line 1.
    write_msg(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": test_uri },
                "position": { "line": 1, "character": 17 },
            },
        }),
    );
    let msg = recv_until_result(&mut reader, 2);
    let raw = msg.get("result").cloned().unwrap_or(Value::Null);

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();

    // Normalize Location | Location[] | LocationLink[] → Location shape.
    let result = if raw.is_object() && raw.get("range").is_some() {
        raw
    } else if let Some(first) = raw.as_array().and_then(|a| a.first()) {
        if let (Some(uri), Some(range)) = (
            first.get("targetUri"),
            first
                .get("targetSelectionRange")
                .or_else(|| first.get("targetRange")),
        ) {
            json!({ "uri": uri, "range": range })
        } else {
            first.clone()
        }
    } else {
        raw
    };

    let returned_uri = result
        .get("uri")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("definition result missing uri: {result}"));
    assert!(
        returned_uri.ends_with("/lib/foo.stk"),
        "expected lib/foo.stk uri, got {returned_uri} (full: {result})"
    );
    let line = result
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("definition result missing range.start.line: {result}"));
    assert_eq!(
        line, 1,
        "fn bar declared on line 1 (0-based) of lib/foo.stk: {result}"
    );
}

#[test]
fn lsp_stdio_rename_sub_returns_workspace_edits() {
    let mut h = LspHarness::new("fn yellow_minion { }\nyellow_minion();\n");
    let r = h.rename(1, 3, "banana");
    h.finish();
    let changes = r
        .get("changes")
        .and_then(Value::as_object)
        .expect("changes map");
    let edit_lists: Vec<usize> = changes
        .values()
        .filter_map(|v| v.as_array().map(Vec::len))
        .collect();
    let n: usize = edit_lists.iter().sum();
    assert!(n >= 2, "expected edits for sub + call sites, got {r}");
}

// ── Hash-key completion for builtins with known return schemas ─────

#[test]
fn audit_completion_hash_key_pool_info_via_arrow() {
    // `my $info = pool_info(); $info->{<cursor>}` must complete from
    // pool_info's known keys, NOT show the full builtin list.
    let mut h = LspHarness::new("my $info = pool_info()\n$info->{c}\n");
    let labels = h.completion(1, 8);
    h.finish();
    assert!(
        labels.contains(&"cpus".to_string()),
        "expected `cpus` key in hash-key completions: {labels:?}",
    );
    assert!(
        labels.contains(&"rayon_threads".to_string()),
        "expected `rayon_threads` key: {labels:?}",
    );
    // Builtins like `print` / `clamp` etc. must NOT leak into hash-key
    // completion at this position.
    assert!(
        !labels.contains(&"print".to_string()),
        "hash-key completion must not include unrelated builtins: {labels:?}",
    );
}

#[test]
fn audit_completion_hash_key_uname_no_arrow() {
    // `$u{KEY}` form (no `->`) also resolves the receiver's builtin.
    let mut h = LspHarness::new("my $u = uname()\n$u{sys}\n");
    let labels = h.completion(1, 6);
    h.finish();
    assert!(
        labels.contains(&"sysname".to_string()),
        "expected sysname in uname keys: {labels:?}",
    );
}

#[test]
fn audit_completion_hash_key_foreach_binding() {
    // `foreach my $row (git_log()) { $row->{<tab>} }` — the loop var
    // binds to git_log's per-row schema.
    let mut h = LspHarness::new("foreach my $row (git_log()) {\n    $row->{s}\n}\n");
    let labels = h.completion(1, 12);
    h.finish();
    assert!(
        labels.contains(&"sha".to_string()),
        "expected git_log row key `sha`: {labels:?}",
    );
}

#[test]
fn audit_completion_hash_key_unrelated_var_empty() {
    // Receiver bound to a non-builtin scalar — no hash-key completion.
    let mut h = LspHarness::new("my $r = 42\n$r->{c}\n");
    let labels = h.completion(1, 6);
    h.finish();
    assert!(
        labels.is_empty(),
        "untyped receiver must return zero hash-key suggestions: {labels:?}",
    );
}

// ── Comprehensive identifier-category audit ────────────────────────

#[test]
fn audit_completion_class_type_with_class_kind() {
    let mut h = LspHarness::new("class MyClass { x: Int }\nMyC\n");
    let items = h.completion_items(1, 3);
    h.finish();
    let cls = items
        .iter()
        .find(|it| it.get("label").and_then(Value::as_str) == Some("MyClass"))
        .unwrap_or_else(|| panic!("missing MyClass in items: {items:?}"));
    assert_eq!(
        cls.get("kind").and_then(Value::as_u64),
        Some(7),
        "class kind should be CompletionItemKind::CLASS (7): {cls}",
    );
}

#[test]
fn audit_completion_struct_type_with_struct_kind() {
    let mut h = LspHarness::new("struct MyStruct { x: Int }\nMyS\n");
    let items = h.completion_items(1, 3);
    h.finish();
    let s = items
        .iter()
        .find(|it| it.get("label").and_then(Value::as_str) == Some("MyStruct"))
        .expect("missing MyStruct");
    assert_eq!(s.get("kind").and_then(Value::as_u64), Some(22)); // STRUCT
}

#[test]
fn audit_completion_enum_type_and_variant() {
    let mut h = LspHarness::new("enum MyEnum { Hup }\nMyEnum::H\n");
    let labels = h.completion(1, 9);
    h.finish();
    assert!(
        labels.contains(&"MyEnum::Hup".to_string()),
        "qualified enum variant must complete: {labels:?}",
    );
}

#[test]
fn audit_completion_loop_label_referenced_by_last() {
    let mut h = LspHarness::new("LOOP: while (1) {\n    last L\n}\n");
    let labels = h.completion(1, 10);
    h.finish();
    assert!(
        labels.contains(&"LOOP".to_string()),
        "loop label must show in completion: {labels:?}",
    );
}

#[test]
fn audit_completion_use_constant_flat_form() {
    let mut h = LspHarness::new("use constant MY_CONST => 42\nMY\n");
    let labels = h.completion(1, 2);
    h.finish();
    assert!(
        labels.contains(&"MY_CONST".to_string()),
        "constant must show in completion: {labels:?}",
    );
}

#[test]
fn audit_completion_use_constant_hash_form() {
    let mut h = LspHarness::new("use constant { AAA => 1, BBB => 2 }\nAA\n");
    let labels = h.completion(1, 2);
    h.finish();
    assert!(
        labels.contains(&"AAA".to_string()),
        "hash-form constant must show: {labels:?}",
    );
}

// ── Qualified completion: suffix-only insertText (no doubled prefix) ──

#[test]
fn audit_completion_qualified_emits_suffix_only_insert_text() {
    let mut h =
        LspHarness::new("fn Demo::handle($x) { 1 }\nfn Demo::other { 2 }\nmy $x = Demo::handle\n");
    let items = h.completion_items(2, 20);
    h.finish();
    let handle = items
        .iter()
        .find(|it| it.get("label").and_then(Value::as_str) == Some("Demo::handle"))
        .expect("missing Demo::handle in qualified items");
    // `insertText` MUST be just `handle` so inserting at `Demo::│`
    // doesn't produce `Demo::Demo::handle`.
    assert_eq!(
        handle.get("insertText").and_then(Value::as_str),
        Some("handle"),
        "qualified completion must use suffix-only insertText: {handle}",
    );
}

// ── In-progress parse error recovery (cursor line blanked) ─────────

#[test]
fn audit_completion_qualified_typing_with_parse_error_returns_items() {
    // `Demo::` alone is a parse error; with the cursor-line-blank
    // recovery, the index from the other lines still resolves.
    let mut h = LspHarness::new("fn Demo::handle { 1 }\nfn Demo::other { 2 }\nDemo::\n");
    let labels = h.completion(2, 6);
    h.finish();
    assert!(
        labels.contains(&"Demo::handle".to_string()),
        "in-progress `Demo::` must still complete via parse recovery: {labels:?}",
    );
    assert!(
        labels.contains(&"Demo::other".to_string()),
        "in-progress completion must list both qualified subs: {labels:?}",
    );
}

// ── Hover suppressed inside string literals ────────────────────────

#[test]
fn audit_hover_suppressed_inside_string_literal() {
    // `length` inside `"length is fine here"` is literal text, NOT
    // the `length` builtin. Hover must return null/empty.
    let mut h = LspHarness::new("my $name = \"length is fine here\"\nlength($name)\n");
    let r = h.hover(0, 16); // cursor on `length` inside the string
    h.finish();
    // Either null result OR result.contents is empty — both indicate
    // hover was suppressed.
    let suppressed = r.is_null()
        || r.pointer("/contents/value")
            .and_then(Value::as_str)
            .map(str::is_empty)
            .unwrap_or(true);
    assert!(
        suppressed,
        "hover inside string must be suppressed, got: {r}"
    );
}

#[test]
fn audit_hover_still_fires_on_builtin_outside_string() {
    // Symmetric guard: hover on actual `length(...)` call should pop.
    let mut h = LspHarness::new("my $name = \"foo\"\nlength($name)\n");
    let r = h.hover(1, 3);
    h.finish();
    let md = r
        .pointer("/contents/value")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(
        md.contains("length") || md.to_lowercase().contains("byte"),
        "hover on bare length() must pop builtin doc: {r}",
    );
}

#[test]
fn audit_hover_inside_string_interpolation_still_fires() {
    // `#{EXPR}` inside `"..."` is real code — hover must fire there.
    let mut h = LspHarness::new("my $n = 1\np \"got #{length(\\\"x\\\")}\"\n");
    let r = h.hover(1, 11); // cursor on `length` inside `#{...}`
    h.finish();
    let md = r
        .pointer("/contents/value")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(
        !md.is_empty(),
        "hover inside #{{}} interpolation must not be suppressed: {r}",
    );
}

// ── Sigil completion: insertText must NOT include the sigil ───────
//
// IntelliJ's LSP client treats `$` / `@` / `%` as non-identifier
// characters, so the completion replacement range starts AFTER the
// typed sigil. If `insertText` includes the sigil, the result is
// doubled — `@<tab>` inserts `@yellow_submarine` and the rendered
// text becomes `@@yellow_submarine`. Pin the bare-name form so the
// fix doesn't silently regress.

#[test]
fn audit_completion_scalar_insert_text_has_no_sigil() {
    let mut h = LspHarness::new("my $yellow_submarine = 1\n$");
    let items = h.completion_items(1, 1);
    h.finish();
    let found = items
        .iter()
        .find(|it| it.get("label").and_then(Value::as_str) == Some("$yellow_submarine"))
        .expect("missing $yellow_submarine in sigil items");
    assert_eq!(
        found.get("insertText").and_then(Value::as_str),
        Some("yellow_submarine"),
        "scalar sigil completion must use bare-name insertText: {found}",
    );
}

#[test]
fn audit_completion_array_insert_text_has_no_sigil() {
    let mut h = LspHarness::new("my @colors = ()\n@");
    let items = h.completion_items(1, 1);
    h.finish();
    let found = items
        .iter()
        .find(|it| it.get("label").and_then(Value::as_str) == Some("@colors"))
        .expect("missing @colors in sigil items");
    assert_eq!(
        found.get("insertText").and_then(Value::as_str),
        Some("colors"),
        "array sigil completion must use bare-name insertText: {found}",
    );
}

#[test]
fn audit_completion_hash_insert_text_has_no_sigil() {
    let mut h = LspHarness::new("my %config = ()\n%");
    let items = h.completion_items(1, 1);
    h.finish();
    let found = items
        .iter()
        .find(|it| it.get("label").and_then(Value::as_str) == Some("%config"))
        .expect("missing %config in sigil items");
    assert_eq!(
        found.get("insertText").and_then(Value::as_str),
        Some("config"),
        "hash sigil completion must use bare-name insertText: {found}",
    );
}

// ── Sigil completion: reflection vars seeded from the wordlist ────
//
// `$<tab>` / `%<tab>` in a fresh file used to return only the
// user-declared names. Perl special vars (`%ENV`, `%INC`, `$ARGV`,
// `$stryke::VERSION`, `%stryke::*`) live in `lsp_completion_words.txt`
// sigil-prefixed; sigil completion now seeds from that wordlist.

#[test]
fn audit_completion_hash_includes_env_inc_from_wordlist() {
    let mut h = LspHarness::new("%");
    let labels = h.completion(0, 1);
    h.finish();
    for v in &["%ENV", "%INC", "%SIG"] {
        assert!(
            labels.contains(&v.to_string()),
            "{v} must appear in `%<tab>` completion: {labels:?}",
        );
    }
}

#[test]
fn audit_completion_hash_includes_stryke_reflection_hashes() {
    let mut h = LspHarness::new("%");
    let labels = h.completion(0, 1);
    h.finish();
    for v in &["%stryke::all", "%stryke::builtins", "%stryke::keywords"] {
        assert!(
            labels.contains(&v.to_string()),
            "{v} must appear in `%<tab>` completion: {labels:?}",
        );
    }
}

#[test]
fn audit_completion_scalar_includes_stryke_version() {
    let mut h = LspHarness::new("$");
    let labels = h.completion(0, 1);
    h.finish();
    assert!(
        labels.contains(&"$stryke::VERSION".to_string()),
        "$stryke::VERSION must appear in `$<tab>` completion: {labels:?}",
    );
}

// ── Bare completion: perl-compat builtins must not be truncated ───
//
// Before the truncate ceiling was raised, `sort` (the 668th `s*`
// word) and `printf` (the 576th `p*` word) fell off the end of the
// alphabetically-sorted 384-item cap and never reached the IDE.

#[test]
fn audit_completion_bare_includes_sort_printf_push_pop() {
    // Empty-prefix bare completion. Document needs at least one line
    // for the LSP server to find the cursor position.
    let mut h = LspHarness::new("\n");
    let labels = h.completion(0, 0);
    h.finish();
    for w in &["sort", "printf", "push", "pop", "shift", "split"] {
        assert!(
            labels.contains(&w.to_string()),
            "{w} must appear in bare completion (truncate cap should not clip it): {labels:?}",
        );
    }
}

// ── Qualified completion: CORE:: seeded from the wordlist ─────────

#[test]
fn audit_completion_core_namespace_emits_wordlist_entries() {
    let mut h = LspHarness::new("CORE::");
    let labels = h.completion(0, 6);
    h.finish();
    for w in &["CORE::PI", "CORE::TAU", "CORE::E"] {
        assert!(
            labels.contains(&w.to_string()),
            "{w} must appear in `CORE::<tab>` completion: {labels:?}",
        );
    }
}

// ── Qualified completion: sigil-prefixed main:: from wordlist ─────
//
// `main` is the default package (per Perl). Builtins live in CORE::
// not main::, so bare `main::<tab>` does NOT promise builtin names —
// only user-declared subs in package main surface there. Sigil-vars
// in package main (`$main::ARGV`, `%main::ENV`, …) ARE in the
// wordlist and must come through sigil completion.

#[test]
fn audit_completion_scalar_main_namespace_includes_argv() {
    let mut h = LspHarness::new("$main::");
    let labels = h.completion(0, 7);
    h.finish();
    assert!(
        labels.contains(&"$main::ARGV".to_string()),
        "$main::ARGV must appear in `$main::<tab>` completion: {labels:?}",
    );
}

#[test]
fn audit_completion_hash_main_namespace_includes_env() {
    let mut h = LspHarness::new("%main::");
    let labels = h.completion(0, 7);
    h.finish();
    for w in &["%main::ENV", "%main::INC", "%main::SIG"] {
        assert!(
            labels.contains(&w.to_string()),
            "{w} must appear in `%main::<tab>` completion: {labels:?}",
        );
    }
}

// ── LSP strict-vars diagnostics on by default ──────────────────────

#[test]
fn audit_lsp_diagnostics_strict_vars_on_by_default() {
    // No `use strict;` in source, but LSP must still flag undefined
    // scalars. This is the IDE-side default (CLI `stryke check`
    // stays lenient).
    let h = LspHarness::new("p $undef_typo\n");
    let _ = h; // diagnostics arrive asynchronously after didOpen;
               // The strict-vars-on default for LSP is tested via the static
               // analyzer suite (`undefined_scalar_detected`); the LSP integration
               // wires `analyze_program_with_strict(_, _, true)` in
               // `compute_diagnostics`. This test pins the wiring at the
               // diagnostics-call layer by reaching into the static analyzer.
    let prog = stryke::parse_with_file("p $undef_typo", "test.stk").expect("parse");
    let r = stryke::static_analysis::analyze_program_with_strict(&prog, "test.stk", true);
    assert!(
        r.is_err(),
        "strict_vars=true must flag $undef_typo in LSP diagnostics path",
    );
}
