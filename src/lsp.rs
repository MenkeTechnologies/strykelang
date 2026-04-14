//! Language server protocol (stdio) for editors — `pe --lsp`.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use lsp_server::{
    Connection, ErrorCode, ExtractError, Message, Notification, ProtocolError, Request, Response,
};
use lsp_types::notification::Notification as NotificationTrait;
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, PublishDiagnostics,
};
use lsp_types::request::Completion;
use lsp_types::request::DocumentHighlightRequest;
use lsp_types::request::DocumentSymbolRequest;
use lsp_types::request::GotoDeclaration;
use lsp_types::request::GotoDefinition;
use lsp_types::request::HoverRequest;
use lsp_types::request::PrepareRenameRequest;
use lsp_types::request::References;
use lsp_types::request::Rename;
use lsp_types::request::Request as RequestTrait;
use lsp_types::request::ResolveCompletionItem;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    DeclarationCapability, Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentHighlight,
    DocumentHighlightKind, DocumentHighlightParams, DocumentSymbolParams, DocumentSymbolResponse,
    Documentation, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, Location, OneOf, Position, PrepareRenameResponse,
    PublishDiagnosticsParams, Range, ReferenceParams, RenameOptions, RenameParams,
    ServerCapabilities, SymbolInformation, SymbolKind, TextDocumentContentChangeEvent,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextEdit, Uri, WorkDoneProgressOptions, WorkspaceEdit,
};
use lsp_types::{InsertTextFormat, MarkupContent, MarkupKind};
use percent_encoding::percent_decode_str;

use crate::ast::MatchArrayElem;
use crate::ast::{Block, Sigil, Statement, StmtKind, SubSigParam, VarDecl};
use crate::error::{ErrorKind, PerlError};
use crate::interpreter::Interpreter;

pub(crate) fn run_stdio() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (conn, io_threads) = Connection::stdio();

    let (init_id, init_params) = conn.initialize_start()?;
    let _: lsp_types::InitializeParams = serde_json::from_value(init_params).unwrap_or_default();

    let caps = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                will_save: None,
                will_save_wait_until: None,
                save: None,
            },
        )),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(true),
            trigger_characters: Some(vec![
                "$".to_string(),
                "@".to_string(),
                "%".to_string(),
                ":".to_string(),
            ]),
            ..Default::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        declaration_provider: Some(DeclarationCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        document_highlight_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };

    let init_result = serde_json::json!({
        "capabilities": caps,
        "serverInfo": {
            "name": "perlrs",
            "version": env!("CARGO_PKG_VERSION"),
        }
    });
    conn.initialize_finish(init_id, init_result)?;

    let mut docs: HashMap<String, String> = HashMap::new();

    for msg in &conn.receiver {
        match msg {
            Message::Request(req) => {
                if conn.handle_shutdown(&req)? {
                    break;
                }
                dispatch_request(&conn, &mut docs, req)?;
            }
            Message::Notification(not) => {
                dispatch_notification(&conn, &mut docs, not)?;
            }
            Message::Response(_) => {}
        }
    }

    io_threads.join()?;
    Ok(())
}

fn dispatch_request(
    conn: &Connection,
    docs: &mut HashMap<String, String>,
    req: Request,
) -> Result<(), ProtocolError> {
    if req.method == DocumentSymbolRequest::METHOD {
        let req_id = req.id.clone();
        let (id, params) = match req.extract(DocumentSymbolRequest::METHOD) {
            Ok(p) => p,
            Err(ExtractError::JsonError { method, error }) => {
                let resp = Response::new_err(
                    req_id,
                    ErrorCode::InvalidParams as i32,
                    format!("{method}: {error}"),
                );
                conn.sender.send(resp.into()).expect("lsp channel");
                return Ok(());
            }
            Err(ExtractError::MethodMismatch(_)) => unreachable!(),
        };
        let result = document_symbols(docs, params);
        let resp = Response::new_ok(id, result);
        conn.sender.send(resp.into()).expect("lsp channel");
        return Ok(());
    }

    if req.method == Completion::METHOD {
        let req_id = req.id.clone();
        let (id, params) = match req.extract::<CompletionParams>(Completion::METHOD) {
            Ok(p) => p,
            Err(ExtractError::JsonError { method, error }) => {
                let resp = Response::new_err(
                    req_id,
                    ErrorCode::InvalidParams as i32,
                    format!("{method}: {error}"),
                );
                conn.sender.send(resp.into()).expect("lsp channel");
                return Ok(());
            }
            Err(ExtractError::MethodMismatch(_)) => unreachable!(),
        };
        let result = completions(docs, params);
        let resp = Response::new_ok(id, result);
        conn.sender.send(resp.into()).expect("lsp channel");
        return Ok(());
    }

    if req.method == ResolveCompletionItem::METHOD {
        let req_id = req.id.clone();
        let (id, item) = match req.extract::<CompletionItem>(ResolveCompletionItem::METHOD) {
            Ok(p) => p,
            Err(ExtractError::JsonError { method, error }) => {
                let resp = Response::new_err(
                    req_id,
                    ErrorCode::InvalidParams as i32,
                    format!("{method}: {error}"),
                );
                conn.sender.send(resp.into()).expect("lsp channel");
                return Ok(());
            }
            Err(ExtractError::MethodMismatch(_)) => unreachable!(),
        };
        let resolved = resolve_completion_item(item);
        let resp = Response::new_ok(id, resolved);
        conn.sender.send(resp.into()).expect("lsp channel");
        return Ok(());
    }

    if req.method == HoverRequest::METHOD {
        let req_id = req.id.clone();
        let (id, params) = match req.extract::<HoverParams>(HoverRequest::METHOD) {
            Ok(p) => p,
            Err(ExtractError::JsonError { method, error }) => {
                let resp = Response::new_err(
                    req_id,
                    ErrorCode::InvalidParams as i32,
                    format!("{method}: {error}"),
                );
                conn.sender.send(resp.into()).expect("lsp channel");
                return Ok(());
            }
            Err(ExtractError::MethodMismatch(_)) => unreachable!(),
        };
        let result = hover(docs, params);
        let resp = Response::new_ok(id, result);
        conn.sender.send(resp.into()).expect("lsp channel");
        return Ok(());
    }

    if req.method == GotoDefinition::METHOD {
        let req_id = req.id.clone();
        let (id, params) = match req.extract::<GotoDefinitionParams>(GotoDefinition::METHOD) {
            Ok(p) => p,
            Err(ExtractError::JsonError { method, error }) => {
                let resp = Response::new_err(
                    req_id,
                    ErrorCode::InvalidParams as i32,
                    format!("{method}: {error}"),
                );
                conn.sender.send(resp.into()).expect("lsp channel");
                return Ok(());
            }
            Err(ExtractError::MethodMismatch(_)) => unreachable!(),
        };
        let result = goto_definition(docs, params);
        let resp = Response::new_ok(id, result);
        conn.sender.send(resp.into()).expect("lsp channel");
        return Ok(());
    }

    if req.method == GotoDeclaration::METHOD {
        let req_id = req.id.clone();
        let (id, params) = match req.extract::<GotoDefinitionParams>(GotoDeclaration::METHOD) {
            Ok(p) => p,
            Err(ExtractError::JsonError { method, error }) => {
                let resp = Response::new_err(
                    req_id,
                    ErrorCode::InvalidParams as i32,
                    format!("{method}: {error}"),
                );
                conn.sender.send(resp.into()).expect("lsp channel");
                return Ok(());
            }
            Err(ExtractError::MethodMismatch(_)) => unreachable!(),
        };
        let result = goto_definition(docs, params);
        let resp = Response::new_ok(id, result);
        conn.sender.send(resp.into()).expect("lsp channel");
        return Ok(());
    }

    if req.method == References::METHOD {
        let req_id = req.id.clone();
        let (id, params) = match req.extract::<ReferenceParams>(References::METHOD) {
            Ok(p) => p,
            Err(ExtractError::JsonError { method, error }) => {
                let resp = Response::new_err(
                    req_id,
                    ErrorCode::InvalidParams as i32,
                    format!("{method}: {error}"),
                );
                conn.sender.send(resp.into()).expect("lsp channel");
                return Ok(());
            }
            Err(ExtractError::MethodMismatch(_)) => unreachable!(),
        };
        let result = references(docs, params);
        let resp = Response::new_ok(id, result);
        conn.sender.send(resp.into()).expect("lsp channel");
        return Ok(());
    }

    if req.method == DocumentHighlightRequest::METHOD {
        let req_id = req.id.clone();
        let (id, params) =
            match req.extract::<DocumentHighlightParams>(DocumentHighlightRequest::METHOD) {
                Ok(p) => p,
                Err(ExtractError::JsonError { method, error }) => {
                    let resp = Response::new_err(
                        req_id,
                        ErrorCode::InvalidParams as i32,
                        format!("{method}: {error}"),
                    );
                    conn.sender.send(resp.into()).expect("lsp channel");
                    return Ok(());
                }
                Err(ExtractError::MethodMismatch(_)) => unreachable!(),
            };
        let result = document_highlight(docs, params);
        let resp = Response::new_ok(id, result);
        conn.sender.send(resp.into()).expect("lsp channel");
        return Ok(());
    }

    if req.method == PrepareRenameRequest::METHOD {
        let req_id = req.id.clone();
        let (id, params) =
            match req.extract::<TextDocumentPositionParams>(PrepareRenameRequest::METHOD) {
                Ok(p) => p,
                Err(ExtractError::JsonError { method, error }) => {
                    let resp = Response::new_err(
                        req_id,
                        ErrorCode::InvalidParams as i32,
                        format!("{method}: {error}"),
                    );
                    conn.sender.send(resp.into()).expect("lsp channel");
                    return Ok(());
                }
                Err(ExtractError::MethodMismatch(_)) => unreachable!(),
            };
        let result = prepare_rename(docs, params);
        let resp = Response::new_ok(id, result);
        conn.sender.send(resp.into()).expect("lsp channel");
        return Ok(());
    }

    if req.method == Rename::METHOD {
        let req_id = req.id.clone();
        let (id, params) = match req.extract::<RenameParams>(Rename::METHOD) {
            Ok(p) => p,
            Err(ExtractError::JsonError { method, error }) => {
                let resp = Response::new_err(
                    req_id,
                    ErrorCode::InvalidParams as i32,
                    format!("{method}: {error}"),
                );
                conn.sender.send(resp.into()).expect("lsp channel");
                return Ok(());
            }
            Err(ExtractError::MethodMismatch(_)) => unreachable!(),
        };
        let result = rename_symbol(docs, params);
        let resp = Response::new_ok(id, result);
        conn.sender.send(resp.into()).expect("lsp channel");
        return Ok(());
    }

    let resp = Response::new_err(
        req.id,
        ErrorCode::MethodNotFound as i32,
        format!("perlrs LSP: unimplemented request {}", req.method),
    );
    conn.sender.send(resp.into()).expect("lsp channel");
    Ok(())
}

fn dispatch_notification(
    conn: &Connection,
    docs: &mut HashMap<String, String>,
    not: Notification,
) -> Result<(), ProtocolError> {
    if let Ok(params) = not
        .clone()
        .extract::<DidOpenTextDocumentParams>(DidOpenTextDocument::METHOD)
    {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        docs.insert(uri.to_string(), text.clone());
        publish_diagnostics(conn, &uri, &text, &uri_to_path(&uri))?;
        return Ok(());
    }
    if let Ok(params) = not
        .clone()
        .extract::<DidChangeTextDocumentParams>(DidChangeTextDocument::METHOD)
    {
        let uri = params.text_document.uri;
        let text = merge_full_change(docs.get(uri.as_str()), params.content_changes);
        docs.insert(uri.to_string(), text.clone());
        publish_diagnostics(conn, &uri, &text, &uri_to_path(&uri))?;
        return Ok(());
    }
    if let Ok(params) = not.extract::<DidCloseTextDocumentParams>(DidCloseTextDocument::METHOD) {
        let uri = params.text_document.uri;
        docs.remove(uri.as_str());
        let n = Notification::new(
            PublishDiagnostics::METHOD.to_string(),
            PublishDiagnosticsParams::new(uri, Vec::new(), None),
        );
        conn.sender.send(n.into()).expect("lsp channel");
        return Ok(());
    }
    Ok(())
}

fn merge_full_change(
    prev: Option<&String>,
    changes: Vec<TextDocumentContentChangeEvent>,
) -> String {
    if let Some(c) = changes.into_iter().last() {
        return c.text;
    }
    prev.cloned().unwrap_or_default()
}

fn uri_to_path(uri: &Uri) -> String {
    let s = uri.as_str();
    if let Some(rest) = s.strip_prefix("file://") {
        let path_bytes = if rest.starts_with('/') {
            rest.as_bytes()
        } else if let Some(i) = rest.find('/') {
            &rest.as_bytes()[i..]
        } else {
            rest.as_bytes()
        };
        return percent_decode_str(std::str::from_utf8(path_bytes).unwrap_or(""))
            .decode_utf8_lossy()
            .to_string();
    }
    s.to_string()
}

fn publish_diagnostics(
    conn: &Connection,
    uri: &Uri,
    text: &str,
    path: &str,
) -> Result<(), ProtocolError> {
    let diagnostics = compute_diagnostics(text, path);
    let n = Notification::new(
        PublishDiagnostics::METHOD.to_string(),
        PublishDiagnosticsParams::new(uri.clone(), diagnostics, None),
    );
    conn.sender.send(n.into()).expect("lsp channel");
    Ok(())
}

fn compute_diagnostics(text: &str, path: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    match crate::parse_with_file(text, path) {
        Err(e) => out.push(perror_to_diagnostic(&e, text)),
        Ok(program) => {
            let mut interp = Interpreter::new();
            interp.file = path.to_string();
            if let Err(e) = crate::lint_program(&program, &mut interp) {
                out.push(perror_to_diagnostic(&e, text));
            }
        }
    }
    out
}

fn perror_to_diagnostic(e: &PerlError, source: &str) -> Diagnostic {
    let severity = Some(match e.kind {
        ErrorKind::Syntax
        | ErrorKind::Runtime
        | ErrorKind::Type
        | ErrorKind::UndefinedVariable
        | ErrorKind::UndefinedSubroutine
        | ErrorKind::Regex
        | ErrorKind::DivisionByZero
        | ErrorKind::Die => DiagnosticSeverity::ERROR,
        ErrorKind::FileNotFound | ErrorKind::IO => DiagnosticSeverity::WARNING,
        ErrorKind::Exit(_) => DiagnosticSeverity::HINT,
    });
    let range = line_range_utf16(source, e.line.max(1));
    Diagnostic {
        range,
        severity,
        code: None,
        code_description: None,
        source: Some("perlrs".to_string()),
        message: e.message.clone(),
        related_information: None,
        tags: None,
        data: None,
    }
}

/// Map1-based line to a single-line range; `character` is UTF-16 offset per LSP.
fn line_range_utf16(source: &str, line_1based: usize) -> Range {
    let lines: Vec<&str> = source.lines().collect();
    let n = lines.len().max(1);
    let idx = line_1based.saturating_sub(1).min(n.saturating_sub(1));
    let line = lines.get(idx).copied().unwrap_or("");
    let end16 = line.encode_utf16().count() as u32;
    lsp_types::Range {
        start: lsp_types::Position {
            line: idx as u32,
            character: 0,
        },
        end: lsp_types::Position {
            line: idx as u32,
            character: end16,
        },
    }
}

fn document_symbols(
    docs: &HashMap<String, String>,
    params: DocumentSymbolParams,
) -> DocumentSymbolResponse {
    let uri = params.text_document.uri;
    let Some(text) = docs.get(uri.as_str()) else {
        return DocumentSymbolResponse::Flat(Vec::new());
    };
    let path = uri_to_path(&uri);
    let Ok(program) = crate::parse_with_file(text, &path) else {
        return DocumentSymbolResponse::Flat(Vec::new());
    };
    let mut symbols = Vec::new();
    for stmt in &program.statements {
        walk_stmt(stmt, &uri, text, &mut symbols, None);
    }
    DocumentSymbolResponse::Flat(symbols)
}

fn walk_stmt(
    stmt: &Statement,
    uri: &Uri,
    source: &str,
    symbols: &mut Vec<SymbolInformation>,
    container: Option<&str>,
) {
    match &stmt.kind {
        StmtKind::SubDecl { name, body, .. } => {
            symbols.push(sym(
                format!("sub {name}"),
                SymbolKind::FUNCTION,
                uri,
                source,
                stmt.line,
                container,
            ));
            walk_block(body, uri, source, symbols, Some(name.as_str()));
        }
        StmtKind::Package { name } => {
            symbols.push(sym(
                format!("package {name}"),
                SymbolKind::MODULE,
                uri,
                source,
                stmt.line,
                container,
            ));
        }
        StmtKind::StructDecl { def } => {
            symbols.push(sym(
                format!("struct {}", def.name),
                SymbolKind::STRUCT,
                uri,
                source,
                stmt.line,
                container,
            ));
        }
        StmtKind::FormatDecl { name, .. } => {
            symbols.push(sym(
                format!("format {name}"),
                SymbolKind::METHOD,
                uri,
                source,
                stmt.line,
                container,
            ));
        }
        StmtKind::Block(b)
        | StmtKind::StmtGroup(b)
        | StmtKind::Begin(b)
        | StmtKind::End(b)
        | StmtKind::UnitCheck(b)
        | StmtKind::Check(b)
        | StmtKind::Init(b)
        | StmtKind::Continue(b) => walk_block(b, uri, source, symbols, container),
        StmtKind::If {
            body,
            elsifs,
            else_block,
            ..
        } => {
            walk_block(body, uri, source, symbols, container);
            for (_, b) in elsifs {
                walk_block(b, uri, source, symbols, container);
            }
            if let Some(b) = else_block {
                walk_block(b, uri, source, symbols, container);
            }
        }
        StmtKind::Unless {
            body, else_block, ..
        } => {
            walk_block(body, uri, source, symbols, container);
            if let Some(b) = else_block {
                walk_block(b, uri, source, symbols, container);
            }
        }
        StmtKind::While {
            body,
            continue_block,
            ..
        }
        | StmtKind::Until {
            body,
            continue_block,
            ..
        } => {
            walk_block(body, uri, source, symbols, container);
            if let Some(b) = continue_block {
                walk_block(b, uri, source, symbols, container);
            }
        }
        StmtKind::DoWhile { body, .. } => walk_block(body, uri, source, symbols, container),
        StmtKind::For {
            init,
            body,
            continue_block,
            ..
        } => {
            if let Some(init) = init {
                walk_stmt(init, uri, source, symbols, container);
            }
            walk_block(body, uri, source, symbols, container);
            if let Some(b) = continue_block {
                walk_block(b, uri, source, symbols, container);
            }
        }
        StmtKind::Foreach {
            body,
            continue_block,
            ..
        } => {
            walk_block(body, uri, source, symbols, container);
            if let Some(b) = continue_block {
                walk_block(b, uri, source, symbols, container);
            }
        }
        StmtKind::EvalTimeout { body, .. } => walk_block(body, uri, source, symbols, container),
        StmtKind::TryCatch {
            try_block,
            catch_block,
            finally_block,
            ..
        } => {
            walk_block(try_block, uri, source, symbols, container);
            walk_block(catch_block, uri, source, symbols, container);
            if let Some(b) = finally_block {
                walk_block(b, uri, source, symbols, container);
            }
        }
        StmtKind::Given { body, .. } => walk_block(body, uri, source, symbols, container),
        StmtKind::When { body, .. } | StmtKind::DefaultCase { body } => {
            walk_block(body, uri, source, symbols, container);
        }
        _ => {}
    }
}

fn walk_block(
    block: &Block,
    uri: &Uri,
    source: &str,
    symbols: &mut Vec<SymbolInformation>,
    container: Option<&str>,
) {
    for stmt in block {
        walk_stmt(stmt, uri, source, symbols, container);
    }
}

#[allow(deprecated)]
fn sym(
    name: String,
    kind: SymbolKind,
    uri: &Uri,
    source: &str,
    line: usize,
    container_name: Option<&str>,
) -> SymbolInformation {
    let range = line_range_utf16(source, line.max(1));
    SymbolInformation {
        name,
        kind,
        tags: None,
        deprecated: None,
        location: Location {
            uri: uri.clone(),
            range,
        },
        container_name: container_name.map(|s| s.to_string()),
    }
}

// ── hover & go-to-definition (subs in open document + builtin docs) ───────

fn collect_sub_fqn_map(program: &crate::ast::Program) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    let mut pkg = String::from("main");
    for stmt in &program.statements {
        collect_sub_fqns_stmt(stmt, &mut pkg, &mut m);
    }
    m
}

fn collect_sub_fqns_block(block: &Block, pkg: &mut String, m: &mut HashMap<String, usize>) {
    for stmt in block {
        collect_sub_fqns_stmt(stmt, pkg, m);
    }
}

fn collect_sub_fqns_stmt(stmt: &Statement, pkg: &mut String, m: &mut HashMap<String, usize>) {
    match &stmt.kind {
        StmtKind::Package { name } => {
            *pkg = name.clone();
        }
        StmtKind::SubDecl { name, body, .. } => {
            let fqn = if name.contains("::") {
                name.clone()
            } else {
                format!("{}::{}", pkg.as_str(), name)
            };
            m.insert(fqn, stmt.line);
            collect_sub_fqns_block(body, pkg, m);
        }
        StmtKind::My(_)
        | StmtKind::Our(_)
        | StmtKind::Local(_)
        | StmtKind::State(_)
        | StmtKind::MySync(_)
        | StmtKind::LocalExpr { .. }
        | StmtKind::Expression(_)
        | StmtKind::Return(_)
        | StmtKind::Last(_)
        | StmtKind::Next(_)
        | StmtKind::Redo(_)
        | StmtKind::Use { .. }
        | StmtKind::UsePerlVersion { .. }
        | StmtKind::UseOverload { .. }
        | StmtKind::No { .. }
        | StmtKind::Goto { .. }
        | StmtKind::Tie { .. }
        | StmtKind::Empty
        | StmtKind::StructDecl { .. }
        | StmtKind::FormatDecl { .. } => {}
        StmtKind::Foreach {
            body,
            continue_block,
            ..
        } => {
            collect_sub_fqns_block(body, pkg, m);
            if let Some(b) = continue_block {
                collect_sub_fqns_block(b, pkg, m);
            }
        }
        StmtKind::Block(b)
        | StmtKind::StmtGroup(b)
        | StmtKind::Begin(b)
        | StmtKind::End(b)
        | StmtKind::UnitCheck(b)
        | StmtKind::Check(b)
        | StmtKind::Init(b)
        | StmtKind::Continue(b) => collect_sub_fqns_block(b, pkg, m),
        StmtKind::If {
            body,
            elsifs,
            else_block,
            ..
        } => {
            collect_sub_fqns_block(body, pkg, m);
            for (_, b) in elsifs {
                collect_sub_fqns_block(b, pkg, m);
            }
            if let Some(b) = else_block {
                collect_sub_fqns_block(b, pkg, m);
            }
        }
        StmtKind::Unless {
            body, else_block, ..
        } => {
            collect_sub_fqns_block(body, pkg, m);
            if let Some(b) = else_block {
                collect_sub_fqns_block(b, pkg, m);
            }
        }
        StmtKind::While {
            body,
            continue_block,
            ..
        }
        | StmtKind::Until {
            body,
            continue_block,
            ..
        } => {
            collect_sub_fqns_block(body, pkg, m);
            if let Some(b) = continue_block {
                collect_sub_fqns_block(b, pkg, m);
            }
        }
        StmtKind::DoWhile { body, .. } => collect_sub_fqns_block(body, pkg, m),
        StmtKind::For {
            init,
            body,
            continue_block,
            ..
        } => {
            if let Some(init) = init {
                collect_sub_fqns_stmt(init, pkg, m);
            }
            collect_sub_fqns_block(body, pkg, m);
            if let Some(b) = continue_block {
                collect_sub_fqns_block(b, pkg, m);
            }
        }
        StmtKind::EvalTimeout { body, .. } => collect_sub_fqns_block(body, pkg, m),
        StmtKind::TryCatch {
            try_block,
            catch_block,
            finally_block,
            ..
        } => {
            collect_sub_fqns_block(try_block, pkg, m);
            collect_sub_fqns_block(catch_block, pkg, m);
            if let Some(b) = finally_block {
                collect_sub_fqns_block(b, pkg, m);
            }
        }
        StmtKind::Given { body, .. } => collect_sub_fqns_block(body, pkg, m),
        StmtKind::When { body, .. } | StmtKind::DefaultCase { body } => {
            collect_sub_fqns_block(body, pkg, m);
        }
    }
}

fn resolve_sub_decl_line(sub_map: &HashMap<String, usize>, word: &str) -> Option<usize> {
    if word.is_empty() {
        return None;
    }
    if word.contains("::") {
        return sub_map.get(word).copied();
    }
    let suffix = format!("::{word}");
    let hits: Vec<_> = sub_map
        .iter()
        .filter(|(k, _)| k.len() >= suffix.len() && k.ends_with(&suffix))
        .collect();
    if hits.len() == 1 {
        return Some(*hits[0].1);
    }
    sub_map.get(word).copied()
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == ':'
}

fn identifier_span_bytes(line: &str, byte_col: usize) -> Option<(usize, usize)> {
    let b = byte_col.min(line.len());
    let mut start = b;
    for (i, c) in line[..b].char_indices().rev() {
        if is_ident_char(c) {
            start = i;
        } else {
            break;
        }
    }
    let mut end = b;
    for (i, c) in line[b..].char_indices() {
        if is_ident_char(c) {
            end = b + i + c.len_utf8();
        } else {
            break;
        }
    }
    if start < end {
        Some((start, end))
    } else {
        None
    }
}

fn range_on_line(line_text: &str, line0: u32, start_byte: usize, end_byte: usize) -> Range {
    let s = start_byte.min(line_text.len());
    let e = end_byte.min(line_text.len());
    let c0 = line_text[..s].encode_utf16().count() as u32;
    let c1 = line_text[..e].encode_utf16().count() as u32;
    Range {
        start: Position {
            line: line0,
            character: c0,
        },
        end: Position {
            line: line0,
            character: c1,
        },
    }
}

fn documentation_to_string(doc: &Documentation) -> String {
    match doc {
        Documentation::String(s) => s.clone(),
        Documentation::MarkupContent(m) => m.value.clone(),
    }
}

fn hover_markdown_for_word(word: &str, text: &str, path: &str) -> Option<String> {
    let program = crate::parse_with_file(text, path).ok()?;
    let sub_map = collect_sub_fqn_map(&program);
    if let Some(ln) = resolve_sub_decl_line(&sub_map, word) {
        return Some(format!("Subroutine `{word}` — declared at line {ln}."));
    }
    if let Some(doc) = doc_for_label(word) {
        return Some(documentation_to_string(&doc));
    }
    None
}

fn hover(docs: &HashMap<String, String>, params: HoverParams) -> Option<Hover> {
    let tdp = params.text_document_position_params;
    let uri = tdp.text_document.uri;
    let text = docs.get(uri.as_str())?;
    let pos = tdp.position;
    let path = uri_to_path(&uri);
    let lines: Vec<&str> = text.lines().collect();
    let line_text = lines.get(pos.line as usize).copied()?;
    let byte_col = utf16_col_to_byte_idx(line_text, pos.character);
    let (start, end) = identifier_span_bytes(line_text, byte_col)?;
    let word = line_text.get(start..end)?.to_string();
    if word.is_empty() {
        return None;
    }
    let md = hover_markdown_for_word(&word, text, &path)?;
    let range = range_on_line(line_text, pos.line, start, end);
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: Some(range),
    })
}

fn goto_definition(
    docs: &HashMap<String, String>,
    params: GotoDefinitionParams,
) -> Option<GotoDefinitionResponse> {
    let tdp = params.text_document_position_params;
    let uri = tdp.text_document.uri;
    let text = docs.get(uri.as_str())?;
    let pos = tdp.position;
    let path = uri_to_path(&uri);
    let lines: Vec<&str> = text.lines().collect();
    let line_text = lines.get(pos.line as usize).copied()?;
    let byte_col = utf16_col_to_byte_idx(line_text, pos.character);
    let (start, end) = identifier_span_bytes(line_text, byte_col)?;
    let word = line_text.get(start..end)?.to_string();
    if word.is_empty() {
        return None;
    }
    let program = crate::parse_with_file(text, &path).ok()?;
    let sub_map = collect_sub_fqn_map(&program);
    let decl_line = resolve_sub_decl_line(&sub_map, &word)?;
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: uri.clone(),
        range: line_range_utf16(text, decl_line),
    }))
}

fn highlights_for_identifier(source: &str, needle: &str) -> Vec<DocumentHighlight> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (line0, line) in source.lines().enumerate() {
        let line0 = line0 as u32;
        let mut search_from = 0usize;
        while search_from <= line.len() {
            let hay = &line[search_from..];
            let Some(rel) = hay.find(needle) else {
                break;
            };
            let abs_start = search_from + rel;
            let abs_end = abs_start + needle.len();
            let before_ok = abs_start == 0
                || line[..abs_start]
                    .chars()
                    .last()
                    .is_none_or(|c| !is_ident_char(c));
            let after_ok = abs_end >= line.len()
                || line[abs_end..]
                    .chars()
                    .next()
                    .is_none_or(|c| !is_ident_char(c));
            if before_ok && after_ok {
                out.push(DocumentHighlight {
                    range: range_on_line(line, line0, abs_start, abs_end),
                    kind: Some(DocumentHighlightKind::TEXT),
                });
            }
            search_from = abs_start + needle.len().max(1);
        }
    }
    out
}

fn document_highlight(
    docs: &HashMap<String, String>,
    params: DocumentHighlightParams,
) -> Option<Vec<DocumentHighlight>> {
    let tdp = params.text_document_position_params;
    let uri = tdp.text_document.uri;
    let text = docs.get(uri.as_str())?;
    let pos = tdp.position;
    let lines: Vec<&str> = text.lines().collect();
    let line_text = lines.get(pos.line as usize).copied()?;
    let byte_col = utf16_col_to_byte_idx(line_text, pos.character);
    let (start, end) = identifier_span_bytes(line_text, byte_col)?;
    let needle = line_text.get(start..end)?;
    if needle.is_empty() {
        return None;
    }
    let v = highlights_for_identifier(text, needle);
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn references(docs: &HashMap<String, String>, params: ReferenceParams) -> Option<Vec<Location>> {
    let tdp = params.text_document_position;
    let uri = tdp.text_document.uri;
    let text = docs.get(uri.as_str())?;
    let pos = tdp.position;
    let lines: Vec<&str> = text.lines().collect();
    let line_text = lines.get(pos.line as usize).copied()?;
    let byte_col = utf16_col_to_byte_idx(line_text, pos.character);
    let (start, end) = identifier_span_bytes(line_text, byte_col)?;
    let needle = line_text.get(start..end)?;
    if needle.is_empty() {
        return None;
    }
    let path = uri_to_path(&uri);
    let mut locs: Vec<Location> = highlights_for_identifier(text, needle)
        .into_iter()
        .map(|h| Location {
            uri: uri.clone(),
            range: h.range,
        })
        .collect();

    if params.context.include_declaration {
        if let Ok(program) = crate::parse_with_file(text, &path) {
            let sub_map = collect_sub_fqn_map(&program);
            if let Some(decl_line) = resolve_sub_decl_line(&sub_map, needle) {
                let decl_range = line_range_utf16(text, decl_line);
                let has_same_line = locs
                    .iter()
                    .any(|l| l.range.start.line == decl_range.start.line);
                if !has_same_line {
                    locs.insert(
                        0,
                        Location {
                            uri: uri.clone(),
                            range: decl_range,
                        },
                    );
                }
            }
        }
    }

    if locs.is_empty() {
        None
    } else {
        Some(locs)
    }
}

fn identifier_needle_at_position(text: &str, pos: Position) -> Option<(String, Range)> {
    let lines: Vec<&str> = text.lines().collect();
    let line_text = lines.get(pos.line as usize).copied()?;
    let byte_col = utf16_col_to_byte_idx(line_text, pos.character);
    let (start, end) = identifier_span_bytes(line_text, byte_col)?;
    let needle = line_text.get(start..end)?;
    if needle.is_empty() {
        return None;
    }
    Some((
        needle.to_string(),
        range_on_line(line_text, pos.line, start, end),
    ))
}

fn sub_map_for_doc(text: &str, path: &str) -> Option<HashMap<String, usize>> {
    let program = crate::parse_with_file(text, path).ok()?;
    Some(collect_sub_fqn_map(&program))
}

fn is_valid_rename_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

fn cmp_range_start_desc(a: &Range, b: &Range) -> Ordering {
    b.start
        .line
        .cmp(&a.start.line)
        .then_with(|| b.start.character.cmp(&a.start.character))
}

fn prepare_rename(
    docs: &HashMap<String, String>,
    params: TextDocumentPositionParams,
) -> Option<PrepareRenameResponse> {
    let uri = params.text_document.uri;
    let text = docs.get(uri.as_str())?;
    let path = uri_to_path(&uri);
    let (needle, range) = identifier_needle_at_position(text, params.position)?;
    let sub_map = sub_map_for_doc(text, &path)?;
    resolve_sub_decl_line(&sub_map, needle.as_str())?;
    Some(PrepareRenameResponse::RangeWithPlaceholder {
        range,
        placeholder: needle,
    })
}

fn rename_symbol(docs: &HashMap<String, String>, params: RenameParams) -> Option<WorkspaceEdit> {
    if !is_valid_rename_ident(&params.new_name) {
        return None;
    }
    let uri = params.text_document_position.text_document.uri;
    let text = docs.get(uri.as_str())?;
    let path = uri_to_path(&uri);
    let (needle, _) = identifier_needle_at_position(text, params.text_document_position.position)?;
    let sub_map = sub_map_for_doc(text, &path)?;
    resolve_sub_decl_line(&sub_map, needle.as_str())?;
    if needle == params.new_name {
        return Some(WorkspaceEdit::default());
    }
    let mut edits: Vec<TextEdit> = highlights_for_identifier(text, needle.as_str())
        .into_iter()
        .map(|h| TextEdit {
            range: h.range,
            new_text: params.new_name.clone(),
        })
        .collect();
    edits.sort_by(|a, b| cmp_range_start_desc(&a.range, &b.range));
    if edits.is_empty() {
        return None;
    }
    #[allow(clippy::mutable_key_type)]
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri, edits);
    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

// ── completion: builtins, file subs/vars, sigils, `Foo::`, snippets, resolve ─

static COMPLETION_WORDS: OnceLock<Vec<String>> = OnceLock::new();

#[derive(Default)]
struct CompletionIndex {
    scalars: BTreeSet<String>,
    arrays: BTreeSet<String>,
    hashes: BTreeSet<String>,
    subs_short: BTreeSet<String>,
    subs_qualified: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LineCompletionMode {
    Bare(String),
    Scalar(String),
    Array(String),
    Hash(String),
}

fn completion_words() -> &'static Vec<String> {
    COMPLETION_WORDS.get_or_init(|| {
        let mut v: Vec<String> = include_str!("lsp_completion_words.txt")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|s| s.to_string())
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    })
}

fn perl_keyword_kind(word: &str) -> Option<CompletionItemKind> {
    const KW: &[&str] = &[
        "and", "async", "await", "catch", "continue", "default", "do", "else", "elsif", "eval",
        "finally", "for", "foreach", "given", "if", "last", "local", "my", "next", "no", "not",
        "or", "our", "package", "redo", "return", "spawn", "state", "struct", "sub", "try",
        "typed", "unless", "until", "use", "when", "while",
    ];
    KW.binary_search(&word).ok()?;
    Some(CompletionItemKind::KEYWORD)
}

fn doc_markup(text: &'static str) -> Documentation {
    Documentation::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value: text.to_string(),
    })
}

/// Short markdown for common symbols; `completionItem/resolve` fills this when missing.
fn doc_for_label(label: &str) -> Option<Documentation> {
    let key = label.strip_suffix(" …").unwrap_or(label);
    let md: &'static str = match key {
        // ── Declarations & keywords ──
        "my" => "Declare a lexical: `my $x;`, `my @a;`, `my %h;`.",
        "our" => "Declare a package-global lexical alias in the current package.",
        "local" => "Temporarily localize a global variable within the current scope.",
        "state" => "Declare a persistent lexical (retains value across calls): `state $n = 0;`.",
        "sub" => "Define a subroutine: `sub name { ... }`.",
        "package" => "Set the current package for subsequent declarations.",
        "use" => "Load and import a module at compile time: `use Module qw(func);`.",
        "no" => "Unimport a module or pragma: `no strict 'refs';`.",
        "require" => "Load a module at runtime: `require Module;`.",
        "return" => "Return a value from a subroutine.",
        "BEGIN" => "`BEGIN { }` — runs at compile time, before the rest of the program.",
        "END" => "`END { }` — runs after the program finishes (or on exit).",

        // ── Control flow ──
        "if" => "`if (COND) { }` conditional block.",
        "elsif" => "`elsif (COND) { }` — additional condition in an if chain.",
        "else" => "`else { }` — default branch of an if/elsif chain.",
        "unless" => "`unless (COND) { }` — negated conditional.",
        "foreach" | "for" => "`for` / `foreach` loop over a list.",
        "while" => "`while (COND) { }` loop.",
        "until" => "`until (COND) { }` — loop until condition is true.",
        "do" => "`do BLOCK` — execute block; `do FILE` — execute file.",
        "last" => "Exit the innermost loop (like `break` in C).",
        "next" => "Skip to the next iteration of the innermost loop.",
        "redo" => "Restart the current loop iteration without re-testing the condition.",
        "continue" => "`continue { }` block executed after each loop iteration.",
        "given" => "Switch-like `given (EXPR) { when ... default ... }`.",
        "when" => "`when (EXPR) { }` — smartmatch case inside `given`.",
        "default" => "`default { }` — fallback case in a `given` block.",

        // ── Exception handling ──
        "try" => "`try { } catch ($e) { } [ finally { } ]` exception handling.",
        "catch" => "`catch ($e) { }` — handle exceptions from a `try` block.",
        "finally" => "`finally { }` — always runs after try/catch, even on die.",
        "eval" => "`eval { }` or `eval \"...\"` — catch exceptions; error in `$@`.",
        "die" => "Raise an exception: `die \"message\";` or `die $obj;`.",
        "warn" => "Print a warning to STDERR: `warn \"message\";`.",
        "croak" => "Die from the caller's perspective (Carp): `croak \"message\";`.",
        "confess" => "Die with a full stack trace (Carp): `confess \"message\";`.",

        // ── I/O ──
        "say" => "Print operands with newline (`-E` / perlrs).",
        "print" => "Print to the selected handle (default `STDOUT`).",
        "printf" => "`printf FORMAT, LIST` — formatted print (like C printf).",
        "sprintf" => "`sprintf FORMAT, LIST` — return formatted string.",
        "open" => "`open my $fh, MODE, FILE` — open a filehandle.",
        "close" => "Close a filehandle: `close $fh;`.",
        "read" => "`read FH, SCALAR, LENGTH [, OFFSET]` — read bytes from a filehandle.",
        "readline" => "Read a line from a filehandle: `readline($fh)` or `<$fh>`.",
        "eof" => "Test whether a filehandle is at end-of-file.",
        "seek" => "`seek FH, POSITION, WHENCE` — position a filehandle.",
        "tell" => "Return current position of a filehandle: `tell $fh`.",
        "binmode" => "Set binary mode on a filehandle: `binmode $fh;`.",
        "fileno" => "Return the file descriptor number for a filehandle.",
        "truncate" => "Truncate a file to a specified length.",
        "flock" => "File locking: `flock $fh, LOCK_EX;`.",
        "getc" => "Read a single character from a filehandle.",
        "select" => "Set default output handle, or 4-arg I/O multiplexing.",
        "sysread" => "Low-level read: `sysread FH, SCALAR, LENGTH [, OFFSET]`.",
        "syswrite" => "Low-level write: `syswrite FH, SCALAR [, LENGTH [, OFFSET]]`.",
        "sysseek" => "Low-level seek: `sysseek FH, POSITION, WHENCE`.",
        "sysopen" => "Low-level open with flags: `sysopen FH, FILE, FLAGS [, PERMS]`.",
        "write" => "Write a format record to a filehandle.",
        "format" => "Declare a report format: `format NAME = ... .`.",

        // ── Strings ──
        "chomp" => "Remove trailing record separator from a string.",
        "chop" => "Remove and return the last character of a string.",
        "chr" => "Return the character for an ASCII/Unicode code point.",
        "hex" => "Convert a hex string to a number: `hex(\"ff\")` → 255.",
        "oct" => "Convert an octal/hex/binary string to a number.",
        "index" => "`index STR, SUBSTR [, POS]` — find substring position.",
        "rindex" => "`rindex STR, SUBSTR [, POS]` — find last substring position.",
        "lc" => "Return lowercased string.",
        "lcfirst" => "Return string with first character lowercased.",
        "uc" => "Return uppercased string.",
        "ucfirst" => "Return string with first character uppercased.",
        "length" => "Return the length of a string (or array in some contexts).",
        "substr" => "`substr STR, OFFSET [, LEN [, REPLACEMENT]]` — extract/replace substring.",
        "quotemeta" => "Escape all non-alphanumeric characters with backslashes.",
        "ord" => "Return the numeric value of the first character.",
        "join" => "`join SEPARATOR, LIST` — join list elements into a string.",
        "split" => "`split /PATTERN/, STRING [, LIMIT]` — split string into list.",
        "reverse" => "Reverse a list or string.",
        "study" => "Hint for regex optimization on a string (mostly no-op in perlrs).",

        // ── Arrays & lists ──
        "push" => "`push @array, LIST` — append elements to array.",
        "pop" => "Remove and return the last element of an array.",
        "shift" => "Remove and return the first element of an array.",
        "unshift" => "`unshift @array, LIST` — prepend elements to array.",
        "splice" => "`splice @array, OFFSET [, LENGTH [, LIST]]` — insert/remove elements.",
        "sort" => "Sort a list (optional comparator block).",
        "map" => "Evaluate a block or expression for each list element.",
        "maps" => "Like `map`, but returns a lazy iterator (streams inputs; use in `|>` chains).",
        "flat_maps" => "Like `flat_map` with lazy iterator output.",
        "grep" => "Filter a list by boolean block or expression (eager; Perl-compatible).",
        "greps" => "Like `grep`, but returns a lazy iterator (streams inputs; use in `|>` chains).",
        "filter" => "perlrs: lazy filter — same shapes as `grep`, returns a pull iterator (use with `|>` / `foreach`).",
        "compact" | "cpt" => "perlrs: Remove undef and empty string values from a list (streaming).",
        "reject" => "perlrs: Inverse of filter — keep items where block returns false (streaming).",
        "concat" | "chain" | "cat" => "perlrs: Concatenate multiple lists/iterators into one (streaming).",
        "scalar" => "Force scalar context: `scalar @arr` returns count.",
        "defined" => "Test whether a value is defined (not `undef`).",
        "exists" => "Test whether a hash key or array index exists.",
        "delete" => "Remove a key from a hash or element from an array.",
        "each" => "Return next (key, value) pair from a hash.",
        "keys" => "Return list of keys from a hash (or indices from an array).",
        "values" => "Return list of values from a hash or array.",
        "ref" => "Return the reference type of a value (e.g. `HASH`, `ARRAY`).",
        "undef" => "The undefined value; `undef $var` undefines a variable.",
        "wantarray" => "Return true if the current context is list context.",
        "caller" => "Return info about the calling subroutine (pkg, file, line).",
        "pos" => "Get/set the position of the last `m//g` match.",

        // ── List::Util & friends ──
        "all" => "`all { COND } @list` — true if all elements satisfy COND.",
        "any" => "`any { COND } @list` — true if at least one element satisfies COND.",
        "none" => "`none { COND } @list` — true if no elements satisfy COND.",
        "first" => "`first { COND } @list` — return first element satisfying COND.",
        "min" => "Return the minimum numeric value from a list.",
        "max" => "Return the maximum numeric value from a list.",
        "sum" | "sum0" => "Return the sum of a numeric list (`sum0` returns 0 for empty).",
        "product" => "Return the product of a numeric list.",
        "reduce" => "`reduce { $_0 OP $_1 } @list` — sequential left fold (`$a`/`$b` also supported).",
        "fold" => "`fold { $_0 OP $_1 } INIT, @list` — left fold with initial value.",
        "reductions" => "`reductions { $_0 OP $_1 } @list` — running reductions (scan).",
        "mean" => "Return the arithmetic mean of a numeric list.",
        "median" => "Return the median of a numeric list.",
        "mode" => "Return the most common value in a list.",
        "stddev" | "std" => "Return the population standard deviation of a numeric list.",
        "variance" => "Return the population variance of a numeric list.",
        "sample" => "`sample N, @list` — random sample of N elements.",
        "shuffle" => "Return a randomly shuffled copy of a list.",
        "uniq" => "Remove duplicates from a list (preserving first occurrence).",
        "uniqint" => "Remove duplicates comparing as integers.",
        "uniqnum" => "Remove duplicates comparing as numbers.",
        "uniqstr" => "Remove duplicates comparing as strings.",
        "zip" => "`zip(\\@a, \\@b)` — interleave arrays element-wise.",
        "zip_longest" => "Zip arrays, padding shorter with undef.",
        "zip_shortest" => "Zip arrays, stopping at the shortest.",
        "mesh" => "Interleave multiple arrays (alias for zip in some contexts).",
        "mesh_longest" => "Interleave arrays, padding shorter with undef.",
        "mesh_shortest" => "Interleave arrays, stopping at the shortest.",
        "chunked" => "`chunked N, @list` — split list into chunks of N elements.",
        "windowed" => "`windowed N, @list` — sliding window of N elements.",
        "tail" | "tl" => "`tail N, @list` — return the last N elements.",
        "pairs" => "Return list as pairs: `([$k,$v], ...)`.",
        "unpairs" => "Flatten pairs back to a flat list.",
        "pairkeys" => "Return keys from a pairlist.",
        "pairvalues" => "Return values from a pairlist.",
        "pairmap" => "`pairmap { $_0, $_1 } @list` — map over key-value pairs (`$a`/`$b` also supported).",
        "pairgrep" => "`pairgrep { $_0, $_1 } @list` — filter key-value pairs (`$a`/`$b` also supported).",
        "pairfirst" => "`pairfirst { $_0, $_1 } @list` — first matching pair (`$a`/`$b` also supported).",

        // ── Functional list ops ──
        "flatten" | "fl" => "Flatten nested arrays into a single list.",
        "distinct" => "Remove duplicates (alias for `uniq`).",
        "collect" => "Collect pipeline/iterator results into a list.",
        "drop" | "skip" | "drp" => "`drop N, @list` — skip the first N elements (streaming).",
        "take" | "head" | "hd" => "`take N, @list` — take at most N elements (streaming).",
        "drop_while" => "`drop_while { COND } @list` — skip leading matching elements (streaming).",
        "skip_while" => "`skip_while { COND } @list` — skip leading matching elements (streaming, alias for `drop_while`).",
        "take_while" => "`take_while { COND } @list` — take leading matching elements (streaming).",
        "first_or" => "`first_or DEFAULT, @list` — returns first element or DEFAULT if empty (streaming).",
        "lines" | "ln" => "`lines STRING` — split string into lines (streaming iterator).",
        "chars" | "ch" => "`chars STRING` — split string into characters (streaming iterator).",
        "stdin" => "`stdin` — streaming iterator over lines from standard input.",
        "trim" | "tm" => "`trim STRING` or `trim @list` — strip whitespace (streaming on lists).",
        "pluck" => "`pluck KEY, @list_of_hashrefs` — extract key from each hashref (streaming).",
        "grep_v" => "`grep_v PATTERN, @list` — inverse grep, reject matching items (streaming).",
        "with_index" | "wi" => "`with_index @list` — pairs each element with its index.",
        "enumerate" | "en" => "`enumerate ITERATOR` — yields `[$index, $item]` pairs (streaming).",
        "chunk" | "chk" => "`chunk N, ITERATOR` — yields N-element arrayrefs (streaming).",
        "dedup" | "dup" => "`dedup ITERATOR` — drops consecutive duplicates (streaming).",
        "range" => "`range(START, END [, STEP])` — lazy integer iterator with optional step.\n\n```perl\nrange(1, 5) |> e p;       # 1 2 3 4 5\nrange(5, 1) |> e p;       # 5 4 3 2 1\nrange(0, 10, 2) |> e p;   # 0 2 4 6 8 10\nrange(10, 0, -2) |> e p;  # 10 8 6 4 2 0\n```",
        "tap" => "`tap { side_effect } @list` — execute block per element, return original list (streaming).",
        "tee" => "`tee FILE, ITERATOR` — write each item to file while passing through (streaming).",
        "nth" => "`nth N, LIST` — get Nth element (0-indexed).",
        "to_set" => "`to_set ITERATOR` — collect iterator/list to a set.",
        "to_hash" => "`to_hash ITERATOR` — collect pairs to a hash.",
        "set" => "Create a set (unique collection) from a list of elements.",
        "deque" => "Create a double-ended queue.",
        "heap" => "Create a min-heap (priority queue) from elements.",
        "peek" => "Peek at the next element without consuming it.",

        // ── Parallel extensions (perlrs) ──
        "pmap" => "Parallel `map` (rayon); order preserved.\n\n```perl\nmy @out = pmap { $_*2 } @list;\nmy @out = pmap { $_*2 } @list, progress => 1;\n```",
        "pmap_chunked" => "Parallel map in batches of N items per thread.\n\n```perl\nmy @out = pmap_chunked 100, { process($_) } @list;\n```",
        "pgrep" => "Parallel `grep` across all cores.\n\n```perl\nmy @matches = pgrep { /pattern/ } @list;\n```",
        "pfor" => "Parallel `foreach` across all cores (no return value).\n\n```perl\npfor { process($_) } @list;\n```",
        "psort" => "Parallel sort across all cores.\n\n```perl\nmy @sorted = psort { $_0 <=> $_1 } @list;  # or $a <=> $b\n```",
        "pcache" => "Parallel memoize (key = stringified `$_`).\n\n```perl\nmy @out = pcache { expensive($_) } @list;\n```",
        "preduce" => "Parallel tree fold (rayon; associative ops only).\n\n```perl\nmy $total = preduce { $_0 + $_1 } @list;  # or $a + $b\n```",
        "preduce_init" => "Parallel fold with identity value.\n\n```perl\nmy $total = preduce_init 0, { $_0 + $_1 } @list;\n```",
        "pmap_reduce" => "Fused parallel map + tree reduce.\n\n```perl\nmy $sum = pmap_reduce { $_*2 } { $_0 + $_1 } @list;\n```",
        "pany" => "`pany { COND } @list` — parallel short-circuit `any`.",
        "pfirst" => "`pfirst { COND } @list` — parallel first matching element.",
        "puniq" => "`puniq @list` — parallel unique elements.",
        "pselect" => "`pselect(@channels)` — wait on multiple `pchannel` receivers.",
        "pflat_map" => "Parallel flat-map: map + flatten results.\n\n```perl\nmy @out = pflat_map { expand($_) } @list;\n```",
        "pflat_map_on" => "Distributed parallel flat-map over `cluster`.",
        "fan" => "Execute BLOCK N times in parallel (`$_` = index).\n\n```perl\nfan 8 { work($_) };\nfan { work($_) } progress => 1;\n```",
        "fan_cap" => "Like `fan` but captures return values (index order).\n\n```perl\nmy @results = fan_cap 8 { compute($_) };\n```",

        // ── Cluster / distributed ──
        "cluster" => "Build an SSH worker pool: `cluster([\"host:8\", ...])`.",
        "pmap_on" => "Distributed `pmap` over `cluster` via `pe --remote-worker`.",
        "ssh" => "Execute a command on a remote host: `ssh(\"host\", \"cmd\")`.",

        // ── Async / concurrency ──
        "async" => "Run a block on a worker thread; returns a task handle.\n\n```perl\nmy $task = async { long_compute() };\nmy $val = await $task;\n```",
        "spawn" => "Same as `async` (Rust-style alias); join with `await`.\n\n```perl\nmy $task = spawn { work() };\n```",
        "await" => "Join an async task and return its value.\n\n```perl\nmy $result = await $task;\n```",
        "pchannel" => "Create a bounded MPMC channel: `my ($tx, $rx) = pchannel(N);`.",
        "barrier" => "Create a synchronization barrier for N threads.",
        "ppool" => "Create a persistent thread pool: `ppool(N, sub { ... })`.",
        "pwatch" => "Watch a file/directory for changes: `pwatch(PATH, sub { ... })`.",

        // ── Pipeline / lazy iterators ──
        "pipeline" => "Lazy iterator chain: `pipeline(@list)->map{...}->filter{...}->collect`.\n\n```perl\nmy @out = pipeline(@data)\n  ->filter { $_ > 0 }\n  ->map { $_*2 }\n  ->take(10)\n  ->collect;\n```",
        "par_pipeline" => "Parallel pipeline: filter/map run in parallel on `collect`; order preserved.\n\n```perl\npar_pipeline(source => \\@data, stages => [...], workers => 4)\n```",
        "par_pipeline_stream" => "Streaming parallel pipeline with channel-based stages.",

        // ── Parallel I/O ──
        "par_lines" => "`par_lines PATH, { code }` — mmap + parallel line scan.\n\n```perl\npar_lines \"data.txt\", sub { process($_) };\n```",
        "par_walk" => "`par_walk PATH, { code }` — parallel recursive directory walk.\n\n```perl\npar_walk \"./src\", sub { say $_ if /\\.rs$/ };\n```",
        "par_sed" => "`par_sed PATTERN, REPLACEMENT, @files` — parallel in-place regex replace.",
        "par_fetch" => "Parallel HTTP fetch across a list of URLs.",
        "par_csv_read" => "Parallel CSV reader: read multiple CSV files in parallel.",

        // ── Typing (perlrs) ──
        "typed" => "Optional type annotation with runtime checks.\n\n```perl\ntyped my $x : Int = 42;\ntyped my $name : Str = \"hello\";\ntyped my $pi : Float = 3.14;\n\n# Typed sub parameters\nmy $add = fn ($a: Int, $b: Int) { $a + $b };\nsub greet ($name: Str) { \"Hello, $name!\" }\n```",
        "struct" => "Declare a struct type (perlrs extension).\n\n```perl\nstruct Point { x => 'Int', y => 'Int' }\nmy $p = Point->new(x => 1, y => 2);\n```",

        // ── Data encoding / codecs ──
        "json_encode" => "Encode a Perl data structure as a JSON string.",
        "json_decode" => "Decode a JSON string into a Perl data structure.",
        "stringify" | "str" => "Convert any value to a parseable perlrs literal.\n\n```perl\nmy $s = str {a => 1};  # +{a => 1}\nmy $copy = eval $s;    # round-trip\n```",
        "ddump" | "dd" => "Debug dump (Data::Dumper style) — diagnostic output, not for eval round-trip.",
        "to_json" | "tj" => "Convert data structure to JSON string.",
        "to_csv" | "tc" => "Convert list of hashes/arrays to CSV string.",
        "to_toml" | "tt" => "Convert hash to TOML string.",
        "to_yaml" | "ty" => "Convert data structure to YAML string.",
        "to_xml" | "tx" => "Convert data structure to XML string.",
        "frequencies" | "freq" | "frq" => "Count occurrences of each element in a list.",
        "interleave" | "il" => "`interleave \\@a, \\@b` — merge arrays alternating elements.",
        "words" | "wd" => "`words STRING` — split string on whitespace.",
        "count" | "len" | "size" | "cnt" => "Return count/length of list, string, hash, or set.",
        "list_count" | "list_size" => "Return element count of flattened list.",
        "clamp" | "clp" => "`clamp MIN, MAX, @list` — constrain values to range.",
        "normalize" | "nrm" => "Normalize numeric list to 0-1 range.",
        "snake_case" | "sc" => "Convert string to snake_case.",
        "camel_case" | "cc" => "Convert string to camelCase.",
        "kebab_case" | "kc" => "Convert string to kebab-case.",
        "json_jq" => "`json_jq($data, \".path.to.value\")` — jq-style JSON query.",
        "toml_decode" => "Decode a TOML string into a Perl hash.",
        "toml_encode" => "Encode a Perl hash as a TOML string.",
        "xml_decode" => "Decode an XML string into a Perl data structure.",
        "xml_encode" => "Encode a Perl data structure as an XML string.",
        "yaml_decode" => "Decode a YAML string into a Perl data structure.",
        "yaml_encode" => "Encode a Perl data structure as a YAML string.",
        "csv_read" => "Read a CSV file/string into an array of arrays.",
        "csv_write" => "Write an array of arrays as CSV.",
        "dataframe" => "Create a columnar dataframe from data.",
        "sqlite" => "`sqlite($path, $sql, @bind)` — execute SQL on an SQLite database.",

        // ── HTTP / networking ──
        "fetch" => "`fetch($url)` — HTTP GET; returns body as string.",
        "fetch_json" => "`fetch_json($url)` — HTTP GET; returns decoded JSON.",
        "fetch_async" => "Non-blocking HTTP GET; returns a task (use `await`).",
        "fetch_async_json" => "Non-blocking HTTP GET + JSON decode; returns a task.",
        "http_request" => "Full HTTP request: `http_request(method => 'POST', url => ..., body => ...)`.",

        // ── Crypto / hashing ──
        "sha256" => "SHA-256 hex digest: `sha256($data)`.",
        "sha1" => "SHA-1 hex digest: `sha1($data)`.",
        "hmac_sha256" | "hmac" => "HMAC-SHA256: `hmac_sha256($data, $key)`.",
        "base64_encode" => "Encode bytes as Base64 string.",
        "base64_decode" => "Decode a Base64 string to bytes.",
        "hex_encode" => "Encode bytes as hex string.",
        "hex_decode" => "Decode a hex string to bytes.",
        "uuid" => "Generate a random UUID v4 string.",
        "jwt_encode" => "Encode a JWT: `jwt_encode($payload, $secret [, $algo])`.",
        "jwt_decode" => "Decode and verify a JWT: `jwt_decode($token, $secret)`.",
        "jwt_decode_unsafe" => "Decode a JWT **without** signature verification.",

        // ── File I/O helpers ──
        "read_lines" | "rl" => "Read file and return list of lines.",
        "append_file" | "af" => "Append string to file.",
        "to_file" => "Write string to file (returns content for piping).",
        "tempfile" | "tf" => "Create a temporary file, return path.",
        "tempdir" | "tdr" => "Create a temporary directory, return path.",
        "read_json" | "rj" => "Read and parse JSON file.",
        "write_json" | "wj" => "Write data as JSON to file.",

        // ── Compression ──
        "gzip" => "Gzip-compress a string/bytes.",
        "gunzip" => "Decompress gzip data.",
        "zstd" => "Zstandard-compress a string/bytes.",
        "zstd_decode" => "Decompress Zstandard data.",

        // ── URL encoding ──
        "url_encode" | "uri_escape" => "Percent-encode a string for URLs.",
        "url_decode" | "uri_unescape" => "Decode a percent-encoded URL string.",

        // ── Logging ──
        "log_info" => "Log at INFO level: `log_info(\"message\");`.",
        "log_warn" => "Log at WARN level: `log_warn(\"message\");`.",
        "log_error" => "Log at ERROR level: `log_error(\"message\");`.",
        "log_debug" => "Log at DEBUG level: `log_debug(\"message\");`.",
        "log_trace" => "Log at TRACE level: `log_trace(\"message\");`.",
        "log_json" => "Log a structured JSON message.",
        "log_level" => "Get or set the current log level.",

        // ── Datetime ──
        "datetime_utc" => "Return current UTC datetime as ISO 8601 string.",
        "datetime_from_epoch" => "Convert epoch seconds to ISO 8601 string.",
        "datetime_strftime" => "`datetime_strftime($format, $epoch)` — format epoch as datetime.",
        "datetime_now_tz" => "`datetime_now_tz($tz)` — current time in a timezone.",
        "datetime_format_tz" => "Format an epoch in a specific timezone.",
        "datetime_parse_local" => "Parse a local datetime string to epoch.",
        "datetime_parse_rfc3339" => "Parse an RFC 3339 datetime string to epoch.",
        "datetime_add_seconds" => "Add seconds to an ISO 8601 datetime string.",
        "time" => "Return current Unix epoch seconds.",
        "times" => "Return user/system CPU times: `($user, $system, $cuser, $csys)`.",
        "localtime" => "Convert epoch to `(sec, min, hour, mday, mon, year, wday, yday, isdst)`.",
        "gmtime" => "Like `localtime` but for UTC.",
        "sleep" => "`sleep N` — pause execution for N seconds.",
        "alarm" => "`alarm N` — schedule a SIGALRM after N seconds.",

        // ── File / path utilities ──
        "basename" | "bn" => "Return the filename part of a path.",
        "dirname" | "dn" => "Return the directory part of a path.",
        "fileparse" => "`fileparse($path)` — split path into (name, dir, suffix).",
        "canonpath" => "Canonicalize a file path (clean up `.`, `..`).",
        "realpath" | "rp" => "Return the resolved absolute path (following symlinks).",
        "getcwd" | "pwd" => "Return the current working directory.",
        "gethostname" | "hn" => "Return system hostname.",
        "which" => "`which(\"cmd\")` — find full path of an executable in PATH.",
        "which_all" | "wha" => "Return all paths for command in PATH.",
        "glob_match" => "Test if filename matches glob pattern.",
        "copy" => "`copy($src, $dst)` — copy a file.",
        "move" | "mv" => "`move($src, $dst)` — rename/move a file.",
        "read_bytes" | "slurp_raw" => "Read an entire file as raw bytes.",
        "spurt" | "write_file" | "wf" => "Write a string to a file: `spurt($path, $content)`.",
        "mkdir" => "Create a directory: `mkdir $path [, $mode]`.",
        "rmdir" => "Remove an empty directory.",
        "unlink" => "Delete one or more files.",
        "rename" => "`rename $old, $new` — rename a file.",
        "link" => "Create a hard link.",
        "symlink" => "Create a symbolic link.",
        "readlink" => "Return the target of a symbolic link.",
        "stat" => "Return file status: `($dev, $ino, $mode, ..., $ctime)`.",
        "chmod" => "Change file permissions.",
        "chown" => "Change file ownership.",
        "chdir" => "Change the current working directory.",
        "glob" => "`glob(\"*.pl\")` — expand a file glob pattern.",
        "opendir" => "Open a directory handle for reading.",
        "readdir" => "Read entries from a directory handle.",
        "closedir" => "Close a directory handle.",
        "seekdir" => "Set position in a directory handle.",
        "telldir" => "Return current position in a directory handle.",
        "rewinddir" => "Reset a directory handle to the beginning.",
        "utime" => "`utime $atime, $mtime, @files` — set file timestamps.",
        "umask" => "Get or set the file creation mask.",
        "uname" => "Return system info: `($sysname, $nodename, $release, $version, $machine)`.",

        // ── Networking / sockets ──
        "socket" => "Create a socket: `socket(SOCK, DOMAIN, TYPE, PROTOCOL)`.",
        "bind" => "Bind a socket to an address.",
        "listen" => "Listen for connections on a socket.",
        "accept" => "Accept a connection on a socket.",
        "connect" => "Connect a socket to an address.",
        "send" => "Send data on a socket.",
        "recv" => "Receive data from a socket.",
        "shutdown" => "Shut down a socket connection.",
        "setsockopt" => "Set a socket option.",
        "getsockopt" => "Get a socket option.",
        "getpeername" => "Return the remote address of a connected socket.",
        "getsockname" => "Return the local address of a socket.",
        "gethostbyname" => "Look up a host by name.",
        "getprotobyname" => "Look up a protocol by name.",
        "getservbyname" => "Look up a service by name and protocol.",

        // ── Process ──
        "fork" => "Fork the current process; returns child PID to parent, 0 to child.",
        "exec" => "Replace the current process with a new command.",
        "system" => "Execute a command in a subshell; returns exit status.",
        "wait" => "Wait for any child process to terminate.",
        "waitpid" => "`waitpid $pid, $flags` — wait for a specific child process.",
        "kill" => "`kill $signal, @pids` — send a signal to processes.",
        "exit" => "Exit the program with a status code.",
        "getlogin" => "Return the login name of the current user.",
        "getpwuid" => "Look up user info by UID.",
        "getpwnam" => "Look up user info by name.",
        "getgrgid" => "Look up group info by GID.",
        "getgrnam" => "Look up group info by name.",
        "getppid" => "Return the parent process ID.",
        "getpgrp" => "Return the process group ID.",
        "setpgrp" => "Set the process group ID.",
        "getpriority" => "Get the scheduling priority of a process.",
        "setpriority" => "Set the scheduling priority of a process.",

        // ── Misc builtins ──
        "pack" => "`pack TEMPLATE, LIST` — pack values into a binary string.",
        "unpack" => "`unpack TEMPLATE, EXPR` — unpack a binary string into values.",
        "vec" => "Treat a string as a bit vector; get/set individual bits.",
        "tie" => "Bind a variable to an object class (tied variables).",
        "prototype" => "Return the prototype string of a function.",
        "bless" => "`bless $ref, $class` — associate a reference with a class.",
        "rand" => "`rand [N]` — random float in [0, N) (default N=1).",
        "srand" => "Seed the random number generator.",
        "int" => "Truncate a number to its integer part.",
        "abs" => "Return the absolute value.",
        "sqrt" => "Return the square root.",
        "squared" | "sq" => "Return the square of a number (N * N).",
        "cubed" | "cb" => "Return the cube of a number (N * N * N).",
        "expt" => "`expt(BASE, EXP)` — return BASE raised to power EXP (BASE ** EXP).",
        "exp" => "Return e raised to a power.",
        "log" => "Return the natural logarithm.",
        "sin" => "Return the sine.",
        "cos" => "Return the cosine.",
        "atan2" => "`atan2 Y, X` — arctangent of Y/X.",
        "formline" => "Format a picture line for `write`/`format`.",

        _ => return None,
    };
    Some(doc_markup(md))
}

fn resolve_completion_item(mut item: CompletionItem) -> CompletionItem {
    if item.documentation.is_some() {
        return item;
    }
    let label = item.label.strip_suffix(" …").unwrap_or(item.label.as_str());
    if let Some(doc) = doc_for_label(label) {
        item.documentation = Some(doc);
        return item;
    }
    if label.contains("::") {
        let base = label.rsplit("::").next().unwrap_or(label);
        if let Some(doc) = doc_for_label(base) {
            item.documentation = Some(doc);
            return item;
        }
    }
    if item.kind == Some(CompletionItemKind::FUNCTION) {
        let md = if label.contains("::") {
            "Subroutine in this document (`Package::name`)."
        } else {
            "Subroutine declared in this document."
        };
        item.documentation = Some(doc_markup(md));
    }
    item
}

fn completions(
    docs: &HashMap<String, String>,
    params: CompletionParams,
) -> Option<CompletionResponse> {
    let uri = params.text_document_position.text_document.uri;
    let text = docs.get(uri.as_str())?;
    let pos = params.text_document_position.position;
    let path = uri_to_path(&uri);

    let lines: Vec<&str> = text.lines().collect();
    let line_text = lines.get(pos.line as usize).copied()?;
    let byte_col = utf16_col_to_byte_idx(line_text, pos.character);
    let mode = line_completion_mode(line_text, byte_col);

    let mut idx = CompletionIndex::default();
    if let Ok(program) = crate::parse_with_file(text, &path) {
        let mut pkg = String::from("main");
        for stmt in &program.statements {
            visit_stmt_for_index(stmt, &mut pkg, &mut idx);
        }
        idx.arrays.insert("_".to_string());
    }

    let mut items: Vec<CompletionItem> = match &mode {
        LineCompletionMode::Scalar(f) => sigil_completions(f, '$', &idx.scalars, "scalar"),
        LineCompletionMode::Array(f) => sigil_completions(f, '@', &idx.arrays, "array"),
        LineCompletionMode::Hash(f) => sigil_completions(f, '%', &idx.hashes, "hash"),
        LineCompletionMode::Bare(f) => {
            if let Some((pkg_p, partial)) = split_qualified_prefix(f) {
                qualified_sub_completions(&pkg_p, &partial, &idx)
            } else {
                bare_completions(f, &idx)
            }
        }
    };

    if matches!(mode, LineCompletionMode::Bare(_)) {
        let f = match &mode {
            LineCompletionMode::Bare(s) => s.as_str(),
            _ => "",
        };
        push_snippet_completions(f, &mut items);
    }

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.truncate(384);
    Some(CompletionResponse::Array(items))
}

fn line_completion_mode(line: &str, byte_col: usize) -> LineCompletionMode {
    let b = byte_col.min(line.len());
    let before = &line[..b];
    let mut start = b;
    for (i, c) in before.char_indices().rev() {
        if c.is_ascii_alphanumeric() || c == '_' || c == ':' {
            start = i;
        } else {
            break;
        }
    }
    let raw = before[start..b].to_string();
    if start > 0 {
        let prev = line[..start].chars().last();
        match prev {
            Some('$') => return LineCompletionMode::Scalar(raw),
            Some('@') => return LineCompletionMode::Array(raw),
            Some('%') => return LineCompletionMode::Hash(raw),
            _ => {}
        }
    }
    LineCompletionMode::Bare(raw)
}

fn split_qualified_prefix(raw: &str) -> Option<(String, String)> {
    if !raw.contains("::") {
        return None;
    }
    let (pkg, tail) = raw.rsplit_once("::")?;
    if pkg.is_empty() {
        return None;
    }
    Some((format!("{pkg}::"), tail.to_string()))
}

fn fqn_matches(pkg_prefix: &str, partial: &str, fqn: &str) -> bool {
    let Some(rest) = fqn.strip_prefix(pkg_prefix) else {
        return false;
    };
    partial.is_empty() || rest.starts_with(partial)
}

fn qualified_sub_completions(
    pkg_prefix: &str,
    partial: &str,
    idx: &CompletionIndex,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for fqn in &idx.subs_qualified {
        if !fqn_matches(pkg_prefix, partial, fqn) {
            continue;
        }
        let doc = doc_for_label(fqn.rsplit("::").next().unwrap_or(fqn.as_str()));
        items.push(CompletionItem {
            label: fqn.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some("sub (qualified)".to_string()),
            documentation: doc,
            ..Default::default()
        });
    }
    items
}

fn sigil_completions(
    filter: &str,
    sigil: char,
    names: &BTreeSet<String>,
    kind: &'static str,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for n in names {
        if !filter.is_empty() && !n.starts_with(filter) {
            continue;
        }
        let insert = format!("{sigil}{n}");
        items.push(CompletionItem {
            label: insert.clone(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(kind.to_string()),
            filter_text: Some(n.clone()),
            insert_text: Some(insert),
            ..Default::default()
        });
    }
    items
}

fn bare_completions(filter: &str, idx: &CompletionIndex) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for s in &idx.subs_short {
        if !filter.is_empty() && !s.starts_with(filter) {
            continue;
        }
        let doc = doc_for_label(s.as_str());
        items.push(CompletionItem {
            label: s.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some("sub".to_string()),
            documentation: doc,
            ..Default::default()
        });
    }

    for w in completion_words() {
        if !filter.is_empty() && !w.starts_with(filter) {
            continue;
        }
        if idx.subs_short.contains(w.as_str()) {
            continue;
        }
        let kind = perl_keyword_kind(w).unwrap_or(CompletionItemKind::FUNCTION);
        let detail = if kind == CompletionItemKind::KEYWORD {
            "keyword"
        } else {
            "builtin"
        };
        let doc = doc_for_label(w);
        items.push(CompletionItem {
            label: w.clone(),
            kind: Some(kind),
            detail: Some(detail.to_string()),
            documentation: doc,
            ..Default::default()
        });
    }
    items
}

fn push_snippet_completions(filter: &str, items: &mut Vec<CompletionItem>) {
    const SNIPS: &[(&str, &str, &str)] = &[
        (
            "my",
            "my \\$${1:name} = ${2:undef};",
            "Lexical declaration (snippet)",
        ),
        (
            "sub",
            "sub ${1:name} {\n\t${0}\n}\n",
            "New subroutine (snippet)",
        ),
        ("say", "say ${1:expr};", "say with placeholder (snippet)"),
        (
            "foreach",
            "foreach my \\$${1:var} (@${2:list}) {\n\t${0}\n}\n",
            "foreach loop (snippet)",
        ),
        (
            "if",
            "if (${1:condition}) {\n\t${0}\n}\n",
            "if block (snippet)",
        ),
        (
            "while",
            "while (${1:condition}) {\n\t${0}\n}\n",
            "while loop (snippet)",
        ),
        (
            "unless",
            "unless (${1:condition}) {\n\t${0}\n}\n",
            "unless block (snippet)",
        ),
        (
            "try",
            "try {\n\t${1}\n} catch (\\$${2:e}) {\n\t${0}\n}\n",
            "try/catch exception handling (snippet)",
        ),
        (
            "given",
            "given (${1:expr}) {\n\twhen (${2:val}) { ${3} }\n\tdefault { ${0} }\n}\n",
            "given/when switch (snippet)",
        ),
        (
            "pmap",
            "my @${1:out} = pmap { ${0} } @${2:list};",
            "Parallel map (snippet)",
        ),
        (
            "pgrep",
            "my @${1:out} = pgrep { ${0} } @${2:list};",
            "Parallel grep (snippet)",
        ),
        (
            "pfor",
            "pfor { ${0} } @${1:list};",
            "Parallel foreach (snippet)",
        ),
        (
            "psort",
            "my @${1:out} = psort { \\$a <=> \\$b } @${2:list};",
            "Parallel sort (snippet)",
        ),
        (
            "pipeline",
            "my @${1:out} = pipeline(@${2:data})\n\t->filter { ${3} }\n\t->map { ${4} }\n\t->collect;",
            "Lazy pipeline chain (snippet)",
        ),
        (
            "async",
            "my \\$${1:task} = async {\n\t${0}\n};\nmy \\$${2:result} = await \\$${1:task};",
            "async/await task (snippet)",
        ),
        (
            "fan",
            "fan ${1:N} {\n\t${0}\n};",
            "Parallel fan-out (snippet)",
        ),
        (
            "par_lines",
            "par_lines \"${1:file}\", sub {\n\t${0}\n};",
            "Parallel line scan (snippet)",
        ),
        (
            "par_walk",
            "par_walk \"${1:dir}\", sub {\n\t${0}\n};",
            "Parallel directory walk (snippet)",
        ),
        (
            "typed",
            "typed my \\$${1:name} : ${2|Int,Str,Float|} = ${0};",
            "Typed variable declaration (snippet)",
        ),
        (
            "struct",
            "struct ${1:Name} {\n\t${2:field} => '${3|Int,Str,Float|}',\n}\n",
            "Struct type declaration (snippet)",
        ),
        (
            "pchannel",
            "my (\\$${1:tx}, \\$${2:rx}) = pchannel(${3:100});",
            "Bounded MPMC channel (snippet)",
        ),
        (
            "open",
            "open my \\$${1:fh}, '${2|<,>,>>|}', '${3:file}' or die \"Cannot open: \\$!\";",
            "Open filehandle (snippet)",
        ),
    ];
    for (kw, body, detail) in SNIPS {
        if !filter.is_empty() && !kw.starts_with(filter) {
            continue;
        }
        items.push(CompletionItem {
            label: format!("{kw} …"),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some((*detail).to_string()),
            filter_text: Some((*kw).to_string()),
            insert_text: Some((*body).to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        });
    }
}

fn collect_var_decls(decls: &[VarDecl], idx: &mut CompletionIndex) {
    for d in decls {
        match d.sigil {
            Sigil::Scalar => {
                idx.scalars.insert(d.name.clone());
            }
            Sigil::Array => {
                idx.arrays.insert(d.name.clone());
            }
            Sigil::Hash => {
                idx.hashes.insert(d.name.clone());
            }
            Sigil::Typeglob => {}
        }
    }
}

fn collect_array_pattern(elems: &[MatchArrayElem], idx: &mut CompletionIndex) {
    for e in elems {
        match e {
            MatchArrayElem::CaptureScalar(n) => {
                idx.scalars.insert(n.clone());
            }
            MatchArrayElem::RestBind(n) => {
                idx.arrays.insert(n.clone());
            }
            MatchArrayElem::Expr(_) | MatchArrayElem::Rest => {}
        }
    }
}

fn collect_sub_params(params: &[SubSigParam], idx: &mut CompletionIndex) {
    for p in params {
        match p {
            SubSigParam::Scalar(n, _ty) => {
                idx.scalars.insert(n.clone());
            }
            SubSigParam::ArrayDestruct(elems) => collect_array_pattern(elems, idx),
            SubSigParam::HashDestruct(pairs) => {
                for (_key, name) in pairs {
                    idx.scalars.insert(name.clone());
                }
            }
        }
    }
}

fn visit_block_for_index(block: &Block, pkg: &mut String, idx: &mut CompletionIndex) {
    for stmt in block {
        visit_stmt_for_index(stmt, pkg, idx);
    }
}

fn visit_stmt_for_index(stmt: &Statement, pkg: &mut String, idx: &mut CompletionIndex) {
    match &stmt.kind {
        StmtKind::Package { name } => {
            *pkg = name.clone();
        }
        StmtKind::SubDecl {
            name, params, body, ..
        } => {
            let fqn = if name.contains("::") {
                name.clone()
            } else {
                format!("{}::{}", pkg.as_str(), name)
            };
            idx.subs_qualified.insert(fqn);
            idx.subs_short.insert(name.clone());
            idx.subs_short.insert(
                name.rsplit("::")
                    .next()
                    .unwrap_or(name.as_str())
                    .to_string(),
            );
            collect_sub_params(params, idx);
            visit_block_for_index(body, pkg, idx);
        }
        StmtKind::My(decls)
        | StmtKind::Our(decls)
        | StmtKind::Local(decls)
        | StmtKind::MySync(decls) => {
            collect_var_decls(decls, idx);
        }
        StmtKind::Foreach {
            var,
            body,
            continue_block,
            ..
        } => {
            idx.scalars.insert(var.clone());
            visit_block_for_index(body, pkg, idx);
            if let Some(b) = continue_block {
                visit_block_for_index(b, pkg, idx);
            }
        }
        StmtKind::Block(b)
        | StmtKind::StmtGroup(b)
        | StmtKind::Begin(b)
        | StmtKind::End(b)
        | StmtKind::UnitCheck(b)
        | StmtKind::Check(b)
        | StmtKind::Init(b)
        | StmtKind::Continue(b) => visit_block_for_index(b, pkg, idx),
        StmtKind::If {
            body,
            elsifs,
            else_block,
            ..
        } => {
            visit_block_for_index(body, pkg, idx);
            for (_, b) in elsifs {
                visit_block_for_index(b, pkg, idx);
            }
            if let Some(b) = else_block {
                visit_block_for_index(b, pkg, idx);
            }
        }
        StmtKind::Unless {
            body, else_block, ..
        } => {
            visit_block_for_index(body, pkg, idx);
            if let Some(b) = else_block {
                visit_block_for_index(b, pkg, idx);
            }
        }
        StmtKind::While {
            body,
            continue_block,
            ..
        }
        | StmtKind::Until {
            body,
            continue_block,
            ..
        } => {
            visit_block_for_index(body, pkg, idx);
            if let Some(b) = continue_block {
                visit_block_for_index(b, pkg, idx);
            }
        }
        StmtKind::DoWhile { body, .. } => visit_block_for_index(body, pkg, idx),
        StmtKind::For {
            init,
            body,
            continue_block,
            ..
        } => {
            if let Some(init) = init {
                visit_stmt_for_index(init, pkg, idx);
            }
            visit_block_for_index(body, pkg, idx);
            if let Some(b) = continue_block {
                visit_block_for_index(b, pkg, idx);
            }
        }
        StmtKind::EvalTimeout { body, .. } => visit_block_for_index(body, pkg, idx),
        StmtKind::TryCatch {
            try_block,
            catch_block,
            finally_block,
            ..
        } => {
            visit_block_for_index(try_block, pkg, idx);
            visit_block_for_index(catch_block, pkg, idx);
            if let Some(b) = finally_block {
                visit_block_for_index(b, pkg, idx);
            }
        }
        StmtKind::Given { body, .. } => visit_block_for_index(body, pkg, idx),
        StmtKind::When { body, .. } | StmtKind::DefaultCase { body } => {
            visit_block_for_index(body, pkg, idx);
        }
        _ => {}
    }
}

fn utf16_col_to_byte_idx(line: &str, col16: u32) -> usize {
    let mut acc = 0u32;
    for (b, ch) in line.char_indices() {
        let w = ch.len_utf16() as u32;
        if acc + w > col16 {
            return b;
        }
        acc += w;
    }
    line.len()
}

#[cfg(test)]
mod completion_tests {
    use super::{
        highlights_for_identifier, identifier_span_bytes, line_completion_mode,
        resolve_sub_decl_line, split_qualified_prefix, utf16_col_to_byte_idx, LineCompletionMode,
    };
    use std::collections::HashMap;

    fn raw_at(line: &str, col16: u32) -> (LineCompletionMode, String) {
        let b = utf16_col_to_byte_idx(line, col16);
        let m = line_completion_mode(line, b);
        let s = match &m {
            LineCompletionMode::Bare(x)
            | LineCompletionMode::Scalar(x)
            | LineCompletionMode::Array(x)
            | LineCompletionMode::Hash(x) => x.clone(),
        };
        (m, s)
    }

    #[test]
    fn bare_prefix_is_word_before_cursor_ascii() {
        let (m, s) = raw_at("ba", 2);
        assert!(matches!(m, LineCompletionMode::Bare(_)));
        assert_eq!(s, "ba");
        let (_, s) = raw_at("foo", 2);
        assert_eq!(s, "fo");
    }

    #[test]
    fn bare_prefix_empty_when_cursor_after_space() {
        let (_, s) = raw_at("print ", 6);
        assert_eq!(s, "");
    }

    #[test]
    fn sigil_modes() {
        let (m, s) = raw_at("$abc", 4);
        assert!(matches!(m, LineCompletionMode::Scalar(_)));
        assert_eq!(s, "abc");
        let (m, s) = raw_at("@things", 7);
        assert!(matches!(m, LineCompletionMode::Array(_)));
        assert_eq!(s, "things");
        let (m, s) = raw_at("%h", 2);
        assert!(matches!(m, LineCompletionMode::Hash(_)));
        assert_eq!(s, "h");
    }

    #[test]
    fn qualified_split() {
        assert_eq!(
            split_qualified_prefix("Foo::ba"),
            Some(("Foo::".to_string(), "ba".to_string()))
        );
        assert_eq!(
            split_qualified_prefix("Foo::"),
            Some(("Foo::".to_string(), "".to_string()))
        );
        assert_eq!(split_qualified_prefix("foo"), None);
        assert_eq!(split_qualified_prefix("::foo"), None);
    }

    #[test]
    fn identifier_span_finds_word_at_cursor() {
        let line = "yellow_minion();";
        let (s, e) = identifier_span_bytes(line, 5).unwrap();
        assert_eq!(&line[s..e], "yellow_minion");
    }

    #[test]
    fn resolve_sub_decl_prefers_unique_fqn_suffix() {
        let mut m = HashMap::new();
        m.insert("Foo::barbaz".to_string(), 2usize);
        assert_eq!(resolve_sub_decl_line(&m, "barbaz"), Some(2));
        assert_eq!(resolve_sub_decl_line(&m, "Foo::barbaz"), Some(2));
    }

    #[test]
    fn highlights_skip_shorter_prefix_matches() {
        let src = "my $xx;\n$xxa = 1;\n";
        let h = highlights_for_identifier(src, "xx");
        assert_eq!(h.len(), 1);
    }
}
