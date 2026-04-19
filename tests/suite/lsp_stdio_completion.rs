//! Drive `fo --lsp` over JSON-RPC stdio: completion, hover, and go-to-definition.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const FO: &str = env!("CARGO_BIN_EXE_fo");
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
        let mut child = Command::new(FO)
            .arg("--lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn fo --lsp");

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
            panic!("fo --lsp stderr:\n{err}");
        }
    }
}

#[test]
fn lsp_stdio_completion_lists_sub_in_buffer() {
    let mut h = LspHarness::new("sub yellow_minion { }\nyell");
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
    let mut h = LspHarness::new("package Foo;\nsub barbaz { }\npackage main;\nFoo::bar");
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
    let mut h = LspHarness::new("say 1;\n");
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
    let mut h = LspHarness::new("sub yellow_minion { }\nyellow_minion();\n");
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

#[test]
fn lsp_stdio_goto_definition_sub() {
    let mut h = LspHarness::new("sub yellow_minion { }\nyellow_minion();\n");
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
    let mut h = LspHarness::new("sub yellow_minion { }\n1;\n");
    let result = h.document_symbols();
    h.finish();
    let arr = result.as_array().expect("documentSymbol array");
    let names: Vec<&str> = arr
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
        .collect();
    assert!(
        names.contains(&"sub yellow_minion"),
        "expected sub in {:?}",
        names
    );
}

#[test]
fn lsp_stdio_resolve_completion_adds_function_doc() {
    let mut h = LspHarness::new("sub x { }\n");
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
    let src = "sub yellow_minion { }\nyellow_minion();\nyellow_minion();\n";
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
    let src = "sub yellow_minion { }\nyellow_minion();\nyellow_minion();\n";
    let mut h = LspHarness::new(src);
    let result = h.references(1, 3, false);
    h.finish();
    let arr = result.as_array().expect("references array");
    assert!(arr.len() >= 2, "expected multiple refs, got {:?}", arr);
}

#[test]
fn lsp_stdio_declaration_same_as_definition_for_sub() {
    let src = "sub yellow_minion { }\nyellow_minion();\n";
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
    let mut h = LspHarness::new("sub yellow_minion { }\nyellow_minion();\n");
    let r = h.prepare_rename(1, 3);
    h.finish();
    assert_eq!(
        r.get("placeholder").and_then(Value::as_str),
        Some("yellow_minion"),
        "prepareRename: {r}"
    );
}

#[test]
fn lsp_stdio_rename_sub_returns_workspace_edits() {
    let mut h = LspHarness::new("sub yellow_minion { }\nyellow_minion();\n");
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
