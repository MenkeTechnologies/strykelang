//! Drive `pe --lsp` over JSON-RPC stdio and assert `textDocument/completion` results.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const PE: &str = env!("CARGO_BIN_EXE_pe");
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

fn handshake<R: Read>(stdin: &mut impl Write, reader: &mut BufReader<R>) {
    write_msg(
        stdin,
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
    let init = recv_until_result(reader, 1);
    assert!(init.get("result").is_some(), "initialize: {init}");

    write_msg(
        stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {},
        }),
    );
}

fn open_doc(stdin: &mut impl Write, text: &str) {
    write_msg(
        stdin,
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

fn request_completion<R: Read>(
    stdin: &mut impl Write,
    reader: &mut BufReader<R>,
    line: u32,
    character: u32,
) -> Vec<String> {
    write_msg(
        stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": URI },
                "position": { "line": line, "character": character },
            },
        }),
    );
    let msg = recv_until_result(reader, 2);
    let result = msg.get("result").expect("completion result");
    labels_from_completion_result(result)
}

fn run_completion_case(source: &str, line: u32, character: u32) -> Vec<String> {
    let mut child = Command::new(PE)
        .arg("--lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pe --lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout_raw = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout_raw);
    let stderr_rx = drain_stderr(child.stderr.take().expect("stderr"));

    handshake(&mut stdin, &mut reader);
    open_doc(&mut stdin, source);

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

    let labels = request_completion(&mut stdin, &mut reader, line, character);
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let err = stderr_rx.recv().unwrap_or_default();
    if err.contains("panic") {
        panic!("pe --lsp stderr:\n{err}");
    }
    labels
}

#[test]
fn lsp_stdio_completion_lists_sub_in_buffer() {
    let src = "sub yellow_minion { }\nyell";
    let labels = run_completion_case(src, 1, 4);
    assert!(
        labels.iter().any(|l| l == "yellow_minion"),
        "expected yellow_minion in {:?}",
        labels
    );
}

#[test]
fn lsp_stdio_completion_scalar_after_sigil() {
    let src = "my $yellow_submarine;\n$yellow";
    let labels = run_completion_case(src, 1, 7);
    assert!(
        labels.iter().any(|l| l == "$yellow_submarine"),
        "expected $yellow_submarine in {:?}",
        labels
    );
}

#[test]
fn lsp_stdio_completion_qualified_sub() {
    let src = "package Foo;\nsub barbaz { }\npackage main;\nFoo::bar";
    let labels = run_completion_case(src, 3, 8);
    assert!(
        labels.iter().any(|l| l == "Foo::barbaz"),
        "expected Foo::barbaz in {:?}",
        labels
    );
}
