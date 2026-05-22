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
        msg.get("result").cloned().unwrap_or(Value::Null)
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
        msg.get("result").cloned().unwrap_or(Value::Null)
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

/// Go to Declaration on a struct field name inside a constructor call
/// `Rectangle(width => -1, height => 5)` must land on the struct's
/// field declaration. Currently the SymbolTable doesn't register
/// individual fields — only the struct's name as a Type — so goto-def
/// returns null. (Marked `ignore` until field indexing lands.)
#[test]
#[ignore]
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
    let test_src = "require \"./lib/foo.stk\"\nProject::Foo::bar();\nmy $x = Project::Foo::bar();\n";
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
            if msg.get("method").and_then(Value::as_str)
                == Some("textDocument/publishDiagnostics")
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
    assert!(
        lines.iter().any(|l| *l == 0),
        "expected decl line 0: lines={lines:?}"
    );
    assert!(
        lines.iter().any(|l| *l == 1),
        "expected reference at line 1 (Op::Add): lines={lines:?}"
    );
    assert!(
        !lines.iter().any(|l| *l == 2),
        "must NOT match `Op` inside the double-quoted string at line 2: lines={lines:?}"
    );
    assert!(
        !lines.iter().any(|l| *l == 3),
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
        lines.iter().any(|l| *l == 1),
        "expected ref at line 1 (Demo::handle decl): lines={lines:?}"
    );
    assert!(
        lines.iter().any(|l| *l == 2),
        "expected ref at line 2 (Demo::handle qualified call): lines={lines:?}"
    );
    assert!(
        !lines.iter().any(|l| *l == 4),
        "must NOT include line 4 (main::handle decl): lines={lines:?}"
    );
    assert!(
        !lines.iter().any(|l| *l == 5),
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
        edits.iter().any(|n| *n == "%visited"),
        "decl line should rewrite to %visited: {edits:?}"
    );
    assert!(
        edits.iter().any(|n| *n == "$visited"),
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
        test_edits.iter().any(|e| e
            .get("newText")
            .and_then(Value::as_str)
            == Some("Project::Foo::MAX_LIMIT")),
        "test file should get qualified MAX_LIMIT: {test_edits:#?}"
    );
    assert!(
        lib_edits.iter().any(|e| e
            .get("newText")
            .and_then(Value::as_str)
            == Some("MAX_LIMIT")),
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
        test_edits.iter().any(|e| e
            .get("newText")
            .and_then(Value::as_str)
            == Some("Project::Foo::Vertex")),
        "test file should get qualified Vertex: {test_edits:#?}"
    );
    assert!(
        lib_edits.iter().any(|e| e
            .get("newText")
            .and_then(Value::as_str)
            == Some("Vertex")),
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
        test_edits.iter().any(|e| e
            .get("newText")
            .and_then(Value::as_str)
            == Some("$Project::Foo::intensity")),
        "test file should get sigil-prefixed qualified intensity: {test_edits:#?}"
    );
    assert!(
        lib_edits.iter().any(|e| e
            .get("newText")
            .and_then(Value::as_str)
            == Some("$intensity")),
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
    fs::write(
        &lib_path,
        "package Project::Foo;\nfn bar { 42 }\n1;\n",
    )
    .expect("write lib");

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
            if msg.get("method").and_then(Value::as_str)
                == Some("textDocument/publishDiagnostics")
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
    let result = msg.get("result").cloned().unwrap_or(Value::Null);

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();

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
