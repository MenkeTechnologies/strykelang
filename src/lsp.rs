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
    doc_for_label_text(label).map(doc_markup)
}

/// Raw doc text lookup — single source of truth for both LSP hover and `pe docs`.
fn doc_for_label_text(label: &str) -> Option<&'static str> {
    let key = label.strip_suffix(" …").unwrap_or(label);
    let md: &'static str = match key {
        // ── Declarations & keywords ──
        "my" => "Declare a lexically scoped variable visible only within the enclosing block or file. `my` is the workhorse of Perl variable declarations — use it for scalars (`$`), arrays (`@`), and hashes (`%`). Variables declared with `my` are invisible outside their scope, which prevents accidental cross-scope mutation. In perlrs, `my` variables participate in pipe chains and can be destructured in list context. Uninitialized `my` variables default to `undef`.\n\n```perl\nmy $name = \"world\";\nmy @nums = 1..5;\nmy %cfg = (debug => 1, verbose => 0);\np \"hello $name\";\n@nums |> grep $_ > 2 |> e p;\n```\n\nUse `my` everywhere unless you specifically need `our` (package global), `local` (dynamic scope), or `state` (persistent).",
        "our" => "Declare a package-global variable that is accessible as a lexical alias in the current scope. Unlike `my`, `our` variables are visible across the entire package and can be accessed from other packages via their fully qualified name (e.g. `$Counter::total`). This is useful for package-level configuration, shared counters, or variables that need to survive across file boundaries. In perlrs, `our` variables are not mutex-protected — use `mysync` instead for parallel-safe globals.\n\n```perl\npackage Counter;\nour $total = 0;\nfn bump { $total++ }\npackage main;\nCounter::bump() for 1..5;\np $Counter::total;   # 5\n```\n\nPrefer `my` for local state; reach for `our` only when other packages need access.",
        "local" => "Temporarily override a global variable's value for the duration of the current dynamic scope. When the scope exits, the original value is automatically restored. This is essential for modifying Perl's special variables (like `$/`, `$\\`, `$,`) without permanently altering global state. Unlike `my` which creates a new variable, `local` saves and restores an existing global. In perlrs, `local` works with all special variables and respects the same restoration semantics during exception unwinding.\n\n```perl\nlocal $/ = undef;       # slurp mode\nopen my $fh, '<', 'data.txt' or die $!;\nmy $body = <$fh>;       # reads entire file\nclose $fh;\n# $/ restored to \"\\n\" when scope exits\n```\n\nCommon patterns: `local $/ = undef` (slurp), `local $, = \",\"` (join print args), `local %ENV` (temporary env).",
        "state" => "Declare a persistent lexical variable that retains its value across calls to the enclosing subroutine. Unlike `my`, which reinitializes on each call, `state` initializes only once — the first time execution reaches the declaration — and preserves the value for all subsequent calls. This is perfect for counters, caches, and memoization without resorting to globals or closures over external variables. In perlrs, `state` variables are per-thread when used inside `t { }` or `fan` blocks; they are not shared across workers.\n\n```perl\nfn counter { state $n = 0; ++$n }\np counter() for 1..5;   # 1 2 3 4 5\nfn memo($x) {\n    state %cache;\n    $cache{$x} //= expensive($x);\n}\n```\n\nRequires `use feature 'state'` in standard Perl, but is always available in perlrs.",
        "sub" => "Define a named or anonymous subroutine. In perlrs, the preferred shorthand is `fn`, which behaves identically but is shorter and supports optional typed parameters. Named subs are installed into the current package; anonymous subs (closures) capture their enclosing lexical scope. Subroutines are first-class values — assign them to scalars, store in arrays, pass as callbacks. The last expression evaluated is the implicit return value unless an explicit `return` is used.\n\n```perl\nfn greet($who) { p \"hello $who\" }\nmy $sq = fn ($x) { $x ** 2 };\n1..5 |> map $sq->($_) |> e p;\nfn apply($f, @args) { $f->(@args) }\napply($sq, 7) |> p;   # 49\n```\n\nUse `fn` for new perlrs code; `sub` is fully supported for Perl compatibility.",
        "package" => "Set the current package namespace for all subsequent declarations. Package names are conventionally `CamelCase` with `::` separators (e.g. `Math::Utils`). All unqualified sub and `our` variable names are installed into the current package. In perlrs, packages work identically to standard Perl — they provide namespace isolation but are not classes by themselves (use `struct` or `bless` for OOP). Switching packages mid-file is allowed but discouraged; prefer one package per file.\n\n```perl\npackage Math::Utils;\nfn factorial($n) { $n <= 1 ? 1 : $n * factorial($n - 1) }\nfn fib($n) { $n < 2 ? $n : fib($n - 1) + fib($n - 2) }\npackage main;\np Math::Utils::factorial(10);   # 3628800\n```",
        "use" => "Load and import a module at compile time: `use Module qw(func);`.\n\n```perl\nuse List::Util qw(sum max);\nmy @vals = 1..10;\np sum(@vals);   # 55\np max(@vals);   # 10\n```",
        "no" => "Unimport a module or pragma: `no strict 'refs';`.\n\n```perl\nno warnings 'experimental';\ngiven ($x) { when (1) { p \"one\" } }\n```",
        "require" => "Load a module at runtime: `require Module;`.\n\n```perl\nrequire JSON;\nmy $data = JSON::decode_json($text);\np $data->{name};\n```",
        "return" => "Return a value from a subroutine.\n\n```perl\nfn clamp($v, $lo, $hi) {\n    return $lo if $v < $lo;\n    return $hi if $v > $hi;\n    $v\n}\n```",
        "BEGIN" => "`BEGIN { }` — runs at compile time, before the rest of the program.\n\n```perl\nBEGIN { p \"compiling...\" }\np \"running\";\n# output: compiling... then running\n```",
        "END" => "`END { }` — runs after the program finishes (or on exit).\n\n```perl\nEND { p \"cleanup done\" }\np \"main work\";\n# output: main work then cleanup done\n```",

        // ── Control flow ──
        "if" => "`if (COND) { }` conditional block.\n\n```perl\nmy $n = 42;\nif ($n > 0) { p \"positive\" }\np \"big\" if $n > 10;   # postfix form\n```",
        "elsif" => "`elsif (COND) { }` — additional condition in an if chain.\n\n```perl\nif ($x < 0)    { p \"negative\" }\nelsif ($x == 0) { p \"zero\" }\nelsif ($x < 10) { p \"small\" }\n```",
        "else" => "`else { }` — default branch of an if/elsif chain.\n\n```perl\nif ($n % 2 == 0) { p \"even\" }\nelse             { p \"odd\" }\n```",
        "unless" => "`unless (COND) { }` — negated conditional.\n\n```perl\nunless ($ENV{QUIET}) { p \"verbose output\" }\np \"missing!\" unless -e $path;   # postfix\n```",
        "foreach" | "for" => "`for` / `foreach` loop over a list.\n\n```perl\nfor my $f (glob \"*.txt\") { p $f }\n1..10 |> grep $_ % 2 |> e p;   # pipe idiom\n```",
        "while" => "`while (COND) { }` loop.\n\n```perl\nwhile (my $line = <STDIN>) {\n    $line |> tm |> p;\n}\n```",
        "until" => "`until (COND) { }` — loop until condition is true.\n\n```perl\nmy $n = 1;\nuntil ($n > 100) { $n *= 2 }\np $n;   # 128\n```",
        "do" => "`do BLOCK` — execute block; `do FILE` — execute file.\n\n```perl\nmy $val = do { my $x = 10; $x ** 2 };\np $val;   # 100\ndo \"config.pl\";   # execute file\n```",
        "last" => "Exit the innermost loop (like `break` in C).\n\n```perl\nfor (1..100) {\n    last if $_ > 5;\n    p $_;\n}   # prints 1..5\n```",
        "next" => "Skip to the next iteration of the innermost loop.\n\n```perl\nfor (1..10) {\n    next if $_ % 2;   # skip odds\n    p $_;\n}   # 2 4 6 8 10\n```",
        "redo" => "Restart the current loop iteration without re-testing the condition.\n\n```perl\nfor my $attempt (1..3) {\n    my $ok = try_connect();\n    redo unless $ok;   # retry same $attempt\n    p \"connected on attempt $attempt\";\n}\n```",
        "continue" => "`continue { }` block executed after each loop iteration.\n\n```perl\nfor (1..5) {\n    next if $_ == 3;\n    p $_;\n} continue { p \"--\" }\n```",
        "given" => "Switch-like `given (EXPR) { when ... default ... }`.\n\n```perl\ngiven ($cmd) {\n    when (\"start\") { p \"starting\" }\n    when (\"stop\")  { p \"stopping\" }\n    default        { p \"unknown: $cmd\" }\n}\n```",
        "when" => "`when (EXPR) { }` — smartmatch case inside `given`.\n\n```perl\ngiven ($n) {\n    when (1)  { p \"one\" }\n    when (2)  { p \"two\" }\n}\n```",
        "default" => "`default { }` — fallback case in a `given` block.\n\n```perl\ngiven ($color) {\n    when (\"red\")  { p \"#f00\" }\n    default      { p \"unknown color\" }\n}\n```",

        // ── Exception handling ──
        "try" => "`try { } catch ($e) { } [ finally { } ]` exception handling.\n\n```perl\ntry {\n    my $data = fetch_json($url);\n    p $data->{name};\n} catch ($e) { warn \"failed: $e\" }\n```",
        "catch" => "`catch ($e) { }` — handle exceptions from a `try` block.\n\n```perl\ntry { die \"boom\" }\ncatch ($e) { p \"caught: $e\" }\n```",
        "finally" => "`finally { }` — always runs after try/catch, even on die.\n\n```perl\ntry { do_work() }\ncatch ($e) { warn $e }\nfinally { close $fh }\n```",
        "eval" => "`eval { }` or `eval \"...\"` — catch exceptions; error in `$@`.\n\n```perl\neval { die \"oops\" };\nif ($@) { p \"caught: $@\" }\n```",
        "die" => "Raise an exception: `die \"message\";` or `die $obj;`.\n\n```perl\nfn divide($a, $b) {\n    die \"division by zero\" unless $b;\n    $a / $b\n}\n```",
        "warn" => "Print a warning to STDERR: `warn \"message\";`.\n\n```perl\nwarn \"file not found: $path\" unless -e $path;\n```",
        "croak" => "Die from the caller's perspective (Carp): `croak \"message\";`.\n\n```perl\nuse Carp;\nfn parse($s) { croak \"empty input\" unless length $s; ... }\n```",
        "confess" => "Die with a full stack trace (Carp): `confess \"message\";`.\n\n```perl\nuse Carp;\nfn deep_call { confess \"unexpected state\" if $broken }\n```",

        // ── I/O ──
        "say" => "Print operands with newline (`-E` / perlrs).\n\n```perl\np \"hello world\";\n@names |> e p;\n```",
        "print" => "Print to the selected handle (default `STDOUT`).\n\n```perl\nprint \"no newline\";\nprint STDERR \"error msg\\n\";\n```",
        "printf" => "`printf FORMAT, LIST` — formatted print (like C printf).\n\n```perl\nprintf \"%-10s %5d\\n\", $name, $score;\nprintf STDERR \"error %d: %s\\n\", $code, $msg;\n```",
        "sprintf" => "`sprintf FORMAT, LIST` — return formatted string.\n\n```perl\nmy $hex = sprintf \"0x%04x\", 255;\nmy $padded = sprintf \"%08d\", $id;\n```",
        "open" => "`open my $fh, MODE, FILE` — open a filehandle.\n\n```perl\nopen my $fh, '<', 'data.txt' or die \"open: $!\";\nmy @lines = <$fh>;\nclose $fh;\n```",
        "close" => "Close a filehandle: `close $fh;`.\n\n```perl\nopen my $fh, '>', 'out.txt' or die $!;\nprint $fh \"done\\n\";\nclose $fh;\n```",
        "read" => "`read FH, SCALAR, LENGTH [, OFFSET]` — read bytes from a filehandle.\n\n```perl\nopen my $fh, '<:raw', $path or die $!;\nread $fh, my $buf, 1024;\np length $buf;\n```",
        "readline" => "Read a line from a filehandle: `readline($fh)` or `<$fh>`.\n\n```perl\nwhile (my $line = <$fh>) {\n    chomp $line;\n    p $line;\n}\n```",
        "eof" => "Test whether a filehandle is at end-of-file.\n\n```perl\nuntil (eof $fh) {\n    my $line = <$fh>;\n    p tm $line;\n}\n```",
        "seek" => "`seek FH, POSITION, WHENCE` — position a filehandle.\n\n```perl\nseek $fh, 0, 0;  # rewind to start\nmy $header = <$fh>;\n```",
        "tell" => "Return current position of a filehandle: `tell $fh`.\n\n```perl\nmy $pos = tell $fh;\np \"at byte $pos\";\n```",
        "binmode" => "Set binary mode on a filehandle: `binmode $fh;`.\n\n```perl\nopen my $fh, '<', $path or die $!;\nbinmode $fh, ':utf8';\n```",
        "fileno" => "Return the file descriptor number for a filehandle.\n\n```perl\nmy $fd = fileno STDOUT;\np \"stdout fd: $fd\";\n```",
        "truncate" => "Truncate a file to a specified length.\n\n```perl\ntruncate $fh, 0;  # empty the file\n```",
        "flock" => "File locking: `flock $fh, LOCK_EX;`.\n\n```perl\nuse Fcntl ':flock';\nflock $fh, LOCK_EX or die \"lock: $!\";\nprint $fh $data;\nflock $fh, LOCK_UN;\n```",
        "getc" => "Read a single character from a filehandle.\n\n```perl\nmy $ch = getc STDIN;\np \"you pressed: $ch\";\n```",
        "select" => "Set default output handle, or 4-arg I/O multiplexing.\n\n```perl\nmy $old = select STDERR;\np \"goes to stderr\";\nselect $old;\n```",
        "sysread" => "Low-level read: `sysread FH, SCALAR, LENGTH [, OFFSET]`.\n\n```perl\nsysread $fh, my $buf, 4096;\np \"read \" . length($buf) . \" bytes\";\n```",
        "syswrite" => "Low-level write: `syswrite FH, SCALAR [, LENGTH [, OFFSET]]`.\n\n```perl\nmy $n = syswrite $fh, \"hello\";\np \"wrote $n bytes\";\n```",
        "sysseek" => "Low-level seek: `sysseek FH, POSITION, WHENCE`.\n\n```perl\nsysseek $fh, 0, 0;  # rewind\n```",
        "sysopen" => "Low-level open with flags: `sysopen FH, FILE, FLAGS [, PERMS]`.\n\n```perl\nuse Fcntl;\nsysopen my $fh, 'log.txt', O_WRONLY|O_APPEND|O_CREAT, 0644;\n```",
        "write" => "Write a format record to a filehandle.\n\n```perl\nwrite;  # output the current format to selected handle\n```",
        "format" => "Declare a report format: `format NAME = ... .`.\n\n```perl\nformat STDOUT =\n@<<<< @>>>>\n$name, $value\n.\n```",

        // ── Strings ──
        "chomp" => "Remove trailing record separator from a string.\n\n```perl\nmy $line = <STDIN>;\nchomp $line;\np $line;\n```",
        "chop" => "Remove and return the last character of a string.\n\n```perl\nmy $s = \"hello!\";\nmy $last = chop $s;  # $last = \"!\", $s = \"hello\"\n```",
        "chr" => "Return the character for an ASCII/Unicode code point.\n\n```perl\np chr 65;       # A\np chr 0x1F600;  # smiley emoji\n```",
        "hex" => "Convert a hex string to a number: `hex(\"ff\")` → 255.\n\n```perl\nmy $n = hex \"deadbeef\";\nprintf \"0x%x = %d\\n\", $n, $n;\n```",
        "oct" => "Convert an octal/hex/binary string to a number.\n\n```perl\np oct \"0755\";    # 493\np oct \"0b1010\";  # 10\np oct \"0xff\";    # 255\n```",
        "index" => "`index STR, SUBSTR [, POS]` — find substring position.\n\n```perl\nmy $i = index \"hello world\", \"world\";  # 6\np $i;\n```",
        "rindex" => "`rindex STR, SUBSTR [, POS]` — find last substring position.\n\n```perl\nmy $i = rindex \"foo/bar/baz\", \"/\";  # 7\np substr \"foo/bar/baz\", $i + 1;     # baz\n```",
        "lc" => "Return lowercased string.\n\n```perl\np lc \"HELLO\";             # hello\n\"SHOUT\" |> t lc |> t rev;  # pipe-forward chain\n```",
        "lcfirst" => "Return string with first character lowercased.\n\n```perl\np lcfirst \"Hello\";  # hello\n```",
        "uc" => "Return uppercased string.\n\n```perl\np uc \"hello\";  # HELLO\n@words |> maps { uc $_ } |> e p;\n```",
        "ucfirst" => "Return string with first character uppercased.\n\n```perl\np ucfirst \"hello\";  # Hello\n```",
        "length" => "Return the length of a string (or array in some contexts).\n\n```perl\np length \"hello\";  # 5\nmy @a = (1..10);\np length @a;       # 10\n```",
        "substr" => "`substr STR, OFFSET [, LEN [, REPLACEMENT]]` — extract/replace substring.\n\n```perl\nmy $s = \"hello world\";\np substr $s, 0, 5;   # hello\nsubstr $s, 6, 5, \"perlrs\";  # $s = \"hello perlrs\"\n```",
        "quotemeta" => "Escape all non-alphanumeric characters with backslashes.\n\n```perl\nmy $safe = quotemeta \"file (1).txt\";\np $safe;  # file\\ \\(1\\)\\.txt\n```",
        "ord" => "Return the numeric value of the first character.\n\n```perl\np ord \"A\";   # 65\np ord \"\\n\";  # 10\n```",
        "join" => "`join SEPARATOR, LIST` — join list elements into a string.\n\n```perl\nmy $csv = join \",\", @fields;\n1..5 |> join \"-\" |> p;  # 1-2-3-4-5\n```",
        "split" => "`split /PATTERN/, STRING [, LIMIT]` — split string into list.\n\n```perl\nmy @parts = split /,/, \"a,b,c\";\n\"one:two:three\" |> split /:/ |> e p;\n```",
        "reverse" => "Reverse a list or string.\n\n```perl\np reverse \"hello\";       # olleh\nmy @r = reverse 1..5;   # (5,4,3,2,1)\n\"abc\" |> t rev |> p;     # cba\n```",
        "study" => "Hint for regex optimization on a string (mostly no-op in perlrs).\n\n```perl\nstudy $text;\nmy @hits = grep { /$pattern/ } @lines;\n```",

        // ── Arrays & lists ──
        "push" => "`push @array, LIST` — appends one or more elements to the end of an array and returns the new length. This is the primary way to grow arrays in perlrs and works identically to Perl's builtin. You can push scalars, lists, or even the result of a pipeline. In perlrs, `push` is O(1) amortized thanks to the underlying Rust `Vec`.\n\n```perl\nmy @q;\npush @q, 1..3;\npush @q, \"four\", \"five\";\np scalar @q;   # 5\n@q |> e p;     # 1 2 3 four five\n```\n\nReturns the new element count, so `my $len = push @arr, $val;` is valid.",
        "pop" => "Remove and return the last element of an array.\n\n```perl\nmy @stk = 1..5;\nmy $top = pop @stk;\np $top;   # 5\n```",
        "shift" => "Remove and return the first element of an array.\n\n```perl\nmy @args = @ARGV;\nmy $cmd = shift @args;\np $cmd;\n```",
        "unshift" => "`unshift @array, LIST` — prepend elements to array.\n\n```perl\nmy @log = (\"b\", \"c\");\nunshift @log, \"a\";\n@log |> e p;   # a b c\n```",
        "splice" => "`splice @array, OFFSET [, LENGTH [, LIST]]` — insert/remove elements.\n\n```perl\nmy @a = 1..5;\nsplice @a, 1, 2, 8, 9;\n@a |> e p;   # 1 8 9 4 5\n```",
        "sort" => "Sort a list (optional comparator block).\n\n```perl\nmy @nums = (3, 1, 4, 1, 5);\nmy @asc = sort { $_0 <=> $_1 } @nums;\n@asc |> e p;   # 1 1 3 4 5\n```",
        "map" => "Evaluate a block or expression for each list element.\n\n```perl\nmy @sq = map { $_ ** 2 } 1..5;\n@sq |> e p;   # 1 4 9 16 25\n```",
        "maps" => "Like `map`, but returns a lazy iterator (streams inputs; use in `|>` chains).\n\n```perl\n1..10 |> maps { $_ * 3 } |> take 4 |> e p;\n# 3 6 9 12\n```",
        "flat_maps" => "Like `flat_map` with lazy iterator output.\n\n```perl\n1..3 |> flat_maps { ($_, $_ * 10) } |> e p;\n# 1 10 2 20 3 30\n```",
        "grep" => "Filter a list by boolean block or expression (eager; Perl-compatible).\n\n```perl\nmy @evens = grep { $_ % 2 == 0 } 1..10;\n@evens |> e p;   # 2 4 6 8 10\n```",
        "greps" => "Like `grep`, but returns a lazy iterator (streams inputs; use in `|>` chains).\n\n```perl\n1..100 |> greps { $_ % 7 == 0 } |> take 3 |> e p;\n# 7 14 21\n```",
        "filter" => "perlrs: lazy filter — same shapes as `grep`, returns a pull iterator (use with `|>` / `foreach`).\n\n```perl\nmy @big = 1..1000 |> filter { $_ > 990 } |> collect;\n@big |> e p;   # 991..1000\n```",
        "compact" | "cpt" => "perlrs: Remove undef and empty string values from a list (streaming).\n\n```perl\nmy @raw = (1, undef, \"\", 2, undef, 3);\n@raw |> compact |> e p;   # 1 2 3\n```",
        "reject" => "perlrs: Inverse of filter — keep items where block returns false (streaming).\n\n```perl\n1..10 |> reject { $_ % 3 == 0 } |> e p;\n# 1 2 4 5 7 8 10\n```",
        "concat" | "chain" | "cat" => "perlrs: Concatenate multiple lists/iterators into one (streaming).\n\n```perl\nmy @a = 1..3; my @b = 7..9;\nconcat(\\@a, \\@b) |> e p;   # 1 2 3 7 8 9\n```",
        "scalar" => "Force scalar context: `scalar @arr` returns count.\n\n```perl\nmy @items = (\"a\", \"b\", \"c\");\np scalar @items;   # 3\n```",
        "defined" => "Test whether a value is defined (not `undef`).\n\n```perl\nmy $x = undef;\np defined($x) ? \"yes\" : \"no\";   # no\n$x = 0;\np defined($x) ? \"yes\" : \"no\";   # yes\n```",
        "exists" => "Test whether a hash key or array index exists.\n\n```perl\nmy %h = (a => 1, b => undef);\np exists $h{b} ? \"yes\" : \"no\";   # yes\np exists $h{c} ? \"yes\" : \"no\";   # no\n```",
        "delete" => "Remove a key from a hash or element from an array.\n\n```perl\nmy %h = (x => 1, y => 2);\ndelete $h{x};\np exists $h{x} ? \"yes\" : \"no\";   # no\n```",
        "each" => "Return next (key, value) pair from a hash.\n\n```perl\nmy %h = (a => 1, b => 2);\nwhile (my ($k, $v) = each %h) { p \"$k=$v\" }\n```",
        "keys" => "Return list of keys from a hash (or indices from an array).\n\n```perl\nmy %env = (HOME => \"/root\", USER => \"me\");\nkeys(%env) |> sort |> e p;   # HOME USER\n```",
        "values" => "Return list of values from a hash or array.\n\n```perl\nmy %scores = (alice => 90, bob => 85);\np sum(values %scores);   # 175\n```",
        "ref" => "Return the reference type of a value (e.g. `HASH`, `ARRAY`).\n\n```perl\nmy $r = [1, 2, 3];\np ref($r);   # ARRAY\np ref(\\%ENV);   # HASH\n```",
        "undef" => "The undefined value; `undef $var` undefines a variable.\n\n```perl\nmy $x = 42;\nundef $x;\np defined($x) ? \"def\" : \"undef\";   # undef\n```",
        "wantarray" => "Return true if the current context is list context.\n\n```perl\nfn ctx { wantarray() ? \"list\" : \"scalar\" }\nmy @r = ctx();  p $r[0];   # list\nmy $r = ctx();  p $r;      # scalar\n```",
        "caller" => "Return info about the calling subroutine (pkg, file, line).\n\n```perl\nfn trace { my ($pkg, $f, $ln) = caller(); p \"$f:$ln\" }\ntrace();   # prints current file:line\n```",
        "pos" => "Get/set the position of the last `m//g` match.\n\n```perl\nmy $s = \"abcabc\";\nwhile ($s =~ /a/g) { p pos($s) }\n# 1 4\n```",

        // ── List::Util & friends ──
        "all" => "`all { COND } @list` — true if all elements satisfy COND.\n\n```perl\nmy @nums = 2, 4, 6, 8;\np all { $_ % 2 == 0 } @nums;   # 1\n```",
        "any" => "`any { COND } @list` — true if at least one element satisfies COND.\n\n```perl\nmy @vals = 1, 3, 5, 8;\np any { $_ > 7 } @vals;   # 1\n```",
        "none" => "`none { COND } @list` — true if no elements satisfy COND.\n\n```perl\nmy @words = (\"cat\", \"dog\", \"bird\");\np none { /z/ } @words;   # 1\n```",
        "first" => "`first { COND } @list` — return first element satisfying COND.\n\n```perl\nmy $f = first { $_ > 10 } 3, 7, 12, 20;\np $f;   # 12\n```",
        "min" => "Return the minimum numeric value from a list.\n\n```perl\np min(5, 3, 9, 1);   # 1\n```",
        "max" => "Return the maximum numeric value from a list.\n\n```perl\np max(5, 3, 9, 1);   # 9\n```",
        "sum" | "sum0" => "Return the sum of a numeric list (`sum0` returns 0 for empty).\n\n```perl\np sum(1..100);    # 5050\np sum0();          # 0\n```",
        "product" => "Return the product of a numeric list.\n\n```perl\np product(1..5);   # 120\n```",
        "reduce" => "`reduce { $_0 OP $_1 } @list` — sequential left fold (`$a`/`$b` also supported).\n\n```perl\nmy $fac = reduce { $_0 * $_1 } 1..6;\np $fac;   # 720\n```",
        "fold" => "`fold { $_0 OP $_1 } INIT, @list` — left fold with initial value.\n\n```perl\nmy $total = fold { $_0 + $_1 } 100, 1..5;\np $total;   # 115\n```",
        "reductions" => "`reductions { $_0 OP $_1 } @list` — running reductions (scan).\n\n```perl\nmy @pfx = reductions { $_0 + $_1 } 1..4;\n@pfx |> e p;   # 1 3 6 10\n```",
        "mean" => "Return the arithmetic mean of a numeric list.\n\n```perl\np mean(2, 4, 6, 8);   # 5\n```",
        "median" => "Return the median of a numeric list.\n\n```perl\np median(1, 3, 5, 7, 9);   # 5\np median(1, 3, 5, 7);      # 4\n```",
        "mode" => "Return the most common value in a list.\n\n```perl\np mode(1, 2, 2, 3, 3, 3);   # 3\n```",
        "stddev" | "std" => "Return the population standard deviation of a numeric list.\n\n```perl\np stddev(2, 4, 4, 4, 5, 5, 7, 9);   # 2\n```",
        "variance" => "Return the population variance of a numeric list.\n\n```perl\np variance(2, 4, 4, 4, 5, 5, 7, 9);   # 4\n```",
        "sample" => "`sample N, @list` — random sample of N elements.\n\n```perl\nmy @pick = sample 3, 1..100;\n@pick |> e p;   # 3 random values\n```",
        "shuffle" => "Return a randomly shuffled copy of a list.\n\n```perl\nmy @deck = shuffle 1..52;\n@deck |> take 5 |> e p;   # 5 random cards\n```",
        "uniq" => "Remove duplicates from a list (preserving first occurrence).\n\n```perl\nmy @u = uniq 1, 2, 2, 3, 1, 3;\n@u |> e p;   # 1 2 3\n```",
        "uniqint" => "Remove duplicates comparing as integers.\n\n```perl\nmy @u = uniqint 1, 1.1, 1.9, 2;\n@u |> e p;   # 1 2\n```",
        "uniqnum" => "Remove duplicates comparing as numbers.\n\n```perl\nmy @u = uniqnum 1.0, 1.00, 2.5, 2.50;\n@u |> e p;   # 1 2.5\n```",
        "uniqstr" => "Remove duplicates comparing as strings.\n\n```perl\nmy @u = uniqstr \"a\", \"b\", \"a\", \"c\";\n@u |> e p;   # a b c\n```",
        "zip" => "`zip(\\@a, \\@b)` — interleave arrays element-wise.\n\n```perl\nmy @a = 1..3; my @b = (\"a\",\"b\",\"c\");\nzip(\\@a, \\@b) |> e p;   # [1,a] [2,b] [3,c]\n```",
        "zip_longest" => "Zip arrays, padding shorter with undef.\n\n```perl\nmy @a = 1..3; my @b = (\"x\");\nzip_longest(\\@a, \\@b) |> e p;   # [1,x] [2,undef] [3,undef]\n```",
        "zip_shortest" => "Zip arrays, stopping at the shortest.\n\n```perl\nmy @a = 1..5; my @b = (\"x\",\"y\");\nzip_shortest(\\@a, \\@b) |> e p;   # [1,x] [2,y]\n```",
        "mesh" => "Interleave multiple arrays (alias for zip in some contexts).\n\n```perl\nmy @k = (\"a\",\"b\"); my @v = (1,2);\nmy %h = mesh(\\@k, \\@v);\np $h{a};   # 1\n```",
        "mesh_longest" => "Interleave arrays, padding shorter with undef.\n\n```perl\nmy @a = 1..3; my @b = (\"x\");\nmy @r = mesh_longest(\\@a, \\@b);\n@r |> e p;   # 1 x 2 undef 3 undef\n```",
        "mesh_shortest" => "Interleave arrays, stopping at the shortest.\n\n```perl\nmy @a = 1..3; my @b = (\"x\",\"y\");\nmy @r = mesh_shortest(\\@a, \\@b);\n@r |> e p;   # 1 x 2 y\n```",
        "chunked" => "`chunked N, @list` — split list into chunks of N elements.\n\n```perl\nmy @ch = chunked 3, 1..7;\n@ch |> e p;   # [1,2,3] [4,5,6] [7]\n```",
        "windowed" => "`windowed N, @list` — sliding window of N elements.\n\n```perl\nmy @w = windowed 3, 1..5;\n@w |> e p;   # [1,2,3] [2,3,4] [3,4,5]\n```",
        "tail" | "tl" => "`tail N, @list` — return the last N elements.\n\n```perl\nmy @t = tail 2, 1..5;\n@t |> e p;   # 4 5\n```",
        "pairs" => "Return list as pairs: `([$k,$v], ...)`.\n\n```perl\nmy @p = pairs \"a\", 1, \"b\", 2;\n@p |> e { p \"$_->[0]=$_->[1]\" };   # a=1 b=2\n```",
        "unpairs" => "Flatten pairs back to a flat list.\n\n```perl\nmy @flat = unpairs [\"a\",1], [\"b\",2];\n@flat |> e p;   # a 1 b 2\n```",
        "pairkeys" => "Return keys from a pairlist.\n\n```perl\nmy @k = pairkeys \"a\", 1, \"b\", 2, \"c\", 3;\n@k |> e p;   # a b c\n```",
        "pairvalues" => "Return values from a pairlist.\n\n```perl\nmy @v = pairvalues \"a\", 1, \"b\", 2;\n@v |> e p;   # 1 2\n```",
        "pairmap" => "`pairmap { $_0, $_1 } @list` — map over key-value pairs (`$a`/`$b` also supported).\n\n```perl\nmy @out = pairmap { \"$_0=$_1\" } \"a\", 1, \"b\", 2;\n@out |> e p;   # a=1 b=2\n```",
        "pairgrep" => "`pairgrep { $_0, $_1 } @list` — filter key-value pairs (`$a`/`$b` also supported).\n\n```perl\nmy @big = pairgrep { $_1 > 5 } \"a\", 3, \"b\", 9, \"c\", 1;\n@big |> e p;   # b 9\n```",
        "pairfirst" => "`pairfirst { $_0, $_1 } @list` — first matching pair (`$a`/`$b` also supported).\n\n```perl\nmy @hit = pairfirst { $_1 > 5 } \"x\", 2, \"y\", 8;\np \"@hit\";   # y 8\n```",

        // ── Functional list ops ──
        "flatten" | "fl" => "Flatten nested arrays into a single list.\n\n```perl\nmy @flat = flatten([1,[2,3]],[4]);\n[1,[2,[3,4]]] |> fl |> e p;  # 1 2 3 4\n```",
        "distinct" => "Remove duplicates (alias for `uniq`).\n\n```perl\nmy @u = distinct(3,1,2,1,3);\n1,2,2,3,3,3 |> distinct |> e p;  # 1 2 3\n```",
        "collect" => "Collect pipeline/iterator results into a list.\n\n```perl\nmy @out = range(1,5) |> map { $_ * 2 } |> collect;\ngen { yield $_ for 1..3 } |> collect |> e p;\n```",
        "drop" | "skip" | "drp" => "`drop N, @list` — skip the first N elements (streaming).\n\n```perl\n1..10 |> drop 3 |> e p;  # 4 5 6 7 8 9 10\nmy @rest = drp 2, @data;\n```",
        "take" | "head" | "hd" => "`take N, @list` — take at most N elements (streaming).\n\n```perl\n1..100 |> take 5 |> e p;  # 1 2 3 4 5\nmy @top = hd 3, @sorted;\n```",
        "drop_while" => "`drop_while { COND } @list` — skip leading matching elements (streaming).\n\n```perl\n1..10 |> drop_while { $_ < 5 } |> e p;  # 5 6 7 8 9 10\n```",
        "skip_while" => "`skip_while { COND } @list` — skip leading matching elements (streaming, alias for `drop_while`).\n\n```perl\n1..10 |> skip_while { $_ < 5 } |> e p;  # 5 6 7 8 9 10\n```",
        "take_while" => "`take_while { COND } @list` — take leading matching elements (streaming).\n\n```perl\n1..10 |> take_while { $_ < 5 } |> e p;  # 1 2 3 4\n```",
        "first_or" => "`first_or DEFAULT, @list` — returns first element or DEFAULT if empty (streaming).\n\n```perl\nmy $v = first_or 0, @maybe_empty;\nmy $x = grep { $_ > 99 } @nums |> first_or -1;\n```",
        "lines" | "ln" => "`lines STRING` — split string into lines (streaming iterator).\n\n```perl\nslurp(\"data.txt\") |> lines |> e p;\nmy @rows = lines $multiline_str;\n```",
        "chars" | "ch" => "`chars STRING` — split string into characters (streaming iterator).\n\n```perl\n\"hello\" |> chars |> e p;  # h e l l o\nmy @c = chars \"abc\";\n```",
        "stdin" => "`stdin` — streaming iterator over lines from standard input.\n\n```perl\nstdin |> grep /error/i |> e p;\nstdin |> take 5 |> e p;\n```",
        "trim" | "tm" => "`trim STRING` or `trim @list` — strip whitespace (streaming on lists).\n\n```perl\n\" hello \" |> tm |> p;  # \"hello\"\n@raw |> tm |> e p;\n```",
        "pluck" => "`pluck KEY, @list_of_hashrefs` — extract key from each hashref (streaming).\n\n```perl\n@users |> pluck \"name\" |> e p;\nmy @ids = pluck \"id\", @records;\n```",
        "grep_v" => "`grep_v PATTERN, @list` — inverse grep, reject matching items (streaming).\n\n```perl\n@words |> grep_v /^#/ |> e p;  # drop comments\nmy @clean = grep_v qr/tmp/, @files;\n```",
        "with_index" | "wi" => "`with_index @list` — pairs each element with its index.\n\n```perl\nqw(a b c) |> wi |> e { p \"$_->[1]: $_->[0]\" };\n# 0: a  1: b  2: c\n```",
        "enumerate" | "en" => "`enumerate ITERATOR` — yields `[$index, $item]` pairs (streaming).\n\n```perl\nstdin |> en |> e { p \"$_->[0]: $_->[1]\" };\n1..5 |> en |> e { p \"$_->[0]: $_->[1]\" };\n```",
        "chunk" | "chk" => "`chunk N, ITERATOR` — yields N-element arrayrefs (streaming).\n\n```perl\n1..9 |> chk 3 |> e { p join \",\", @$_ };\n# 1,2,3  4,5,6  7,8,9\n```",
        "dedup" | "dup" => "`dedup ITERATOR` — drops consecutive duplicates (streaming).\n\n```perl\n1,1,2,2,3,1,1 |> dedup |> e p;  # 1 2 3 1\n```",
        "range" => "`range(START, END [, STEP])` — lazy integer iterator with optional step.\n\n```perl\nrange(1, 5) |> e p;       # 1 2 3 4 5\nrange(5, 1) |> e p;       # 5 4 3 2 1\nrange(0, 10, 2) |> e p;   # 0 2 4 6 8 10\nrange(10, 0, -2) |> e p;  # 10 8 6 4 2 0\n```",
        "tap" => "`tap { side_effect } @list` — execute block per element, return original list (streaming).\n\n```perl\n1..5 |> tap { log_debug \"saw: $_\" } |> map { $_ * 2 } |> e p;\n```",
        "tee" => "`tee FILE, ITERATOR` — write each item to file while passing through (streaming).\n\n```perl\n1..10 |> tee \"/tmp/log.txt\" |> map { $_ * 2 } |> e p;\n```",
        "nth" => "`nth N, LIST` — get Nth element (0-indexed).\n\n```perl\nmy $third = nth 2, @data;\n1..10 |> nth 4 |> p;  # 5\n```",
        "to_set" => "`to_set ITERATOR` — collect iterator/list to a set.\n\n```perl\nmy $s = 1..5 |> to_set;\n@words |> to_set;  # deduplicated set\n```",
        "to_hash" => "`to_hash ITERATOR` — collect pairs to a hash.\n\n```perl\nmy %h = qw(a 1 b 2) |> to_hash;\n@pairs |> to_hash;\n```",
        "set" => "Create a set (unique collection) from a list of elements.\n\n```perl\nmy $s = set(1, 2, 3, 2, 1);\np $s->contains(2);  # 1\n```",
        "deque" => "Create a double-ended queue.\n\n```perl\nmy $dq = deque(1, 2, 3);\n$dq->push_front(0);\n$dq->push_back(4);\n```",
        "heap" => "Create a min-heap (priority queue) from elements.\n\n```perl\nmy $h = heap(5, 3, 8, 1);\np $h->pop;  # 1 (smallest first)\n```",
        "peek" => "Peek at the next element without consuming it.\n\n```perl\nmy $g = gen { yield $_ for 1..5 };\np peek $g;    # 1 (not consumed)\np $g->next;   # 1\n```",

        // ── Parallel extensions (perlrs) ──
        "pmap" => "Parallel `map` powered by rayon's work-stealing thread pool. Every element of the input list is processed concurrently across all available CPU cores, and the output order is guaranteed to match the input order. This is the primary workhorse for CPU-bound transforms in perlrs — use it whenever you have a pure function and a large list. Pass `progress => 1` to get a live progress bar on STDERR for long-running jobs.\n\n```perl\nmy @out = pmap { $_ * 2 } 1..1_000_000;\nmy @hashes = pmap { sha256($_) } @blobs, progress => 1;\n1..100 |> pmap { fetch(\"https://api.example.com/item/$_\") } |> e p;\n```",
        "pmap_chunked" => "Parallel map that groups input into contiguous batches of N elements before distributing to threads. This reduces per-item scheduling overhead when the per-element work is very cheap (e.g. a few arithmetic ops). Each thread receives a slice of N consecutive items, processes them sequentially within the batch, then returns the batch result. Use this instead of `pmap` when profiling shows rayon overhead dominates the actual computation.\n\n```perl\nmy @out = pmap_chunked 100, { $_ ** 2 } 1..1_000_000;\nmy @parsed = pmap_chunked 50, { json_decode($_) } @json_strings;\n```",
        "pgrep" => "Parallel `grep` that evaluates the filter predicate concurrently across all CPU cores using rayon. The result preserves the original input order, so it is a drop-in replacement for `grep` on large lists. Best suited for predicates that do meaningful work per element — if the predicate is trivial (e.g. a single regex on short strings), sequential `grep` may be faster due to lower scheduling overhead.\n\n```perl\nmy @matches = pgrep { /complex_pattern/ } @big_list;\nmy @primes = pgrep { is_prime($_) } 2..1_000_000;\n@files |> pgrep { -s $_ > 1024 } |> e p;\n```",
        "pfor" => "Parallel `foreach` that executes a side-effecting block across all CPU cores with no return value. Use this when you need to perform work for each element (writing files, sending requests, updating shared state) but don't need to collect results. The block receives each element as `$_`. Iteration order is non-deterministic, so the block must be safe to run concurrently.\n\n```perl\npfor { write_report($_) } @records;\npfor { compress_file($_) } glob(\"*.log\");\n@urls |> pfor { fetch($_); p \"done: $_\" };\n```",
        "psort" => "Parallel sort that uses rayon's parallel merge-sort algorithm. Accepts an optional comparator block using `$_0`/`$_1` (or `$a`/`$b`). For large lists (10k+ elements), this significantly outperforms the sequential `sort` by splitting the array, sorting partitions in parallel, and merging. The sort is stable — equal elements retain their relative order.\n\n```perl\nmy @sorted = psort { $_0 <=> $_1 } @big_list;\nmy @by_name = psort { $_0->{name} cmp $_1->{name} } @records;\n@nums |> psort { $a <=> $b } |> e p;\n```",
        "pcache" => "Parallel memoized map — each element is processed concurrently, but results are cached by the stringified value of `$_` so duplicate inputs are computed only once. This is ideal when your input list contains many repeated values and the computation is expensive. The cache is a concurrent hash map shared across all threads, so there is no lock contention on reads after the first computation.\n\n```perl\nmy @out = pcache { expensive_lookup($_) } @list_with_dupes;\nmy @resolved = pcache { dns_resolve($_) } @hostnames;\n```",
        "preduce" => "Parallel tree-fold using rayon's `reduce` — splits the list into chunks, reduces each chunk independently, then merges partial results. The combining operation **must be associative** (e.g. `+`, `*`, `max`); non-associative ops will produce incorrect results. Much faster than sequential `reduce` on large numeric lists because the tree structure allows O(log n) merge depth across cores.\n\n```perl\nmy $total = preduce { $_0 + $_1 } @nums;\nmy $biggest = preduce { $_0 > $_1 ? $_0 : $_1 } @vals;\nmy $product = preduce { $a * $b } 1..100;\n```",
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
        "serve" => "Start a blocking HTTP server.\n\n```perl\nserve 8080, fn ($req) {\n    # $req = { method, path, query, headers, body, peer }\n    { status => 200, body => \"hello\" }\n};\n\nserve 3000, fn ($req) {\n    my $data = { name => \"perlrs\", version => \"0.4\" };\n    { status => 200, body => json_encode($data) }\n}, { workers => 8 };\n```\n\nHandler returns: hashref `{ status, body, headers }`, string (200 OK), or undef (404).\nJSON content-type auto-detected when body starts with `{` or `[`.",
        "par_csv_read" => "Parallel CSV reader: read multiple CSV files in parallel.",

        // ── Typing (perlrs) ──
        "typed" => "`typed` adds optional runtime type annotations to lexical variables and subroutine parameters. When a `typed` declaration is in effect, perlrs inserts a lightweight check at assignment time that verifies the value matches the declared type (`Int`, `Str`, `Float`, `Bool`, `ArrayRef`, `HashRef`, or a user-defined `struct` name). This is especially useful for catching accidental type mismatches at function boundaries in larger programs. The annotation is purely a runtime guard — it has zero impact on pipeline performance because the check is only performed once at the point of assignment, not on every read.\n\n```perl\ntyped my $x : Int = 42;\ntyped my $name : Str = \"hello\";\ntyped my $pi : Float = 3.14;\nmy $add = fn ($a: Int, $b: Int) { $a + $b };\np $add->(3, 4);   # 7\n```\n\nNote: assigning a value of the wrong type raises a runtime exception immediately.",
        "struct" => "`struct` declares a named record type with typed fields, giving perlrs lightweight struct semantics similar to Rust structs or Python dataclasses. Each field is specified as `name => 'Type'`, and perlrs generates a constructor (`->new`), per-field accessors, and a debug-print representation automatically. Structs integrate with `typed` — you can use a struct name as a type annotation on variables and parameters. Field access is checked at construction time, so misspelled field names or missing required fields are caught immediately rather than silently producing undef.\n\n```perl\nstruct Point { x => 'Int', y => 'Int' }\nmy $p = Point->new(x => 3, y => 4);\np $p->x;           # 3\np $p->y;           # 4\ntyped my $origin : Point = Point->new(x => 0, y => 0);\n```\n\nNote: structs are value objects — constructing with an unknown field name is a fatal error.",

        // ── Data encoding / codecs ──
        "json_encode" => "`json_encode` serializes any Perl data structure — hashrefs, arrayrefs, nested combinations, numbers, strings, booleans, and undef — into a compact JSON string. It uses a fast Rust-backed serializer so it is significantly faster than `JSON::XS` for large payloads. The output is always valid UTF-8 JSON suitable for writing to files, sending over HTTP, or piping to other tools. Use `json_decode` to round-trip back.\n\n```perl\nmy %cfg = (debug => 1, paths => [\"/tmp\", \"/var\"]);\nmy $j = json_encode(\\%cfg);\np $j;   # {\"debug\":1,\"paths\":[\"/tmp\",\"/var\"]}\n$j |> spurt \"/tmp/cfg.json\";\n```\n\nNote: undef becomes JSON `null`; Perl booleans serialize as `true`/`false`.",
        "json_decode" => "`json_decode` parses a JSON string and returns the corresponding Perl data structure — hashrefs for objects, arrayrefs for arrays, and native scalars for strings/numbers/booleans. It is strict by default: malformed JSON raises an exception rather than returning partial data. This makes it safe to use in pipelines where corrupt input should halt processing. The Rust parser underneath handles large documents efficiently and supports full Unicode.\n\n```perl\nmy $data = json_decode('{\"name\":\"perlrs\",\"ver\":1}');\np $data->{name};   # perlrs\nslurp(\"data.json\") |> json_decode |> dd;\n```\n\nNote: JSON `null` becomes Perl `undef`; trailing commas and comments are not allowed.",
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
        "sha224" => "SHA-224 hex digest: `sha224($data)`.",
        "sha384" => "SHA-384 hex digest: `sha384($data)`.",
        "sha512" => "SHA-512 hex digest: `sha512($data)`.",
        "sha1" => "SHA-1 hex digest: `sha1($data)`.",
        "crc32" => "CRC-32 checksum: `crc32($data)` — returns integer.",
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
        "read_lines" | "rl" => "Read a file and return its contents as a list of lines with trailing newlines stripped. This is the idiomatic way to slurp a file line-by-line in perlrs without manually opening a filehandle. The short alias `rl` keeps one-liners concise. If the file does not exist, the program dies with an error message.\n\n```perl\nmy @lines = rl(\"data.txt\");\np scalar @lines;               # line count\n@lines |> grep /ERROR/ |> e p; # print error lines\nmy $first = (rl \"config.ini\")[0];\n```\n\nNote: returns an empty list for an empty file.",
        "append_file" | "af" => "Append a string to the end of a file, creating it if it does not exist. This is the safe way to add content without overwriting — useful for log files, CSV accumulation, or incremental output. The short alias `af` is convenient in pipelines. The file is opened, written, and closed atomically per call.\n\n```perl\naf(\"log.txt\", \"started at \" . datetime_utc() . \"\\n\");\n1..5 |> e { af(\"nums.txt\", \"$_\\n\") };\nmy @data = (\"a\",\"b\",\"c\");\n@data |> e { af \"out.txt\", \"$_\\n\" };\n```",
        "to_file" => "Write a string to a file, truncating any existing content. Unlike `append_file`, this replaces the file entirely. Returns the written content so it can be used in a pipeline — write to disk and continue processing in one expression. Creates the file if it does not exist.\n\n```perl\nmy $csv = \"name,age\\nAlice,30\\nBob,25\";\n$csv |> to_file(\"people.csv\") |> p;\nto_file(\"empty.txt\", \"\");  # truncate a file\n```\n\nNote: the return-value-for-piping behavior distinguishes this from a plain write.",
        "tempfile" | "tf" => "Create a temporary file in the system temp directory and return its absolute path as a string. The file is created with a unique name and exists on disk immediately. Use `tf` as a short alias for quick scratch files in one-liners. The caller is responsible for cleanup, though OS temp-directory reaping will eventually reclaim it.\n\n```perl\nmy $tmp = tf();\nto_file($tmp, \"scratch data\\n\");\np rl($tmp);           # scratch data\nmy @all = map { tf() } 1..3;  # three temp files\n```",
        "tempdir" | "tdr" => "Create a temporary directory in the system temp directory and return its absolute path. The directory is created with a unique name and is ready for use immediately. The short alias `tdr` mirrors `tf` for files. Useful for isolating multi-file operations like test fixtures, build artifacts, or staged output.\n\n```perl\nmy $dir = tdr();\nto_file(\"$dir/a.txt\", \"hello\");\nto_file(\"$dir/b.txt\", \"world\");\nmy @files = glob(\"$dir/*.txt\");\np scalar @files;   # 2\n```",
        "read_json" | "rj" => "Read a JSON file from disk and parse it into a perlrs data structure (hash ref or array ref). The short alias `rj` keeps JSON-config one-liners terse. Dies if the file does not exist or contains malformed JSON. This is the complement of `write_json`/`wj`.\n\n```perl\nmy $cfg = rj(\"config.json\");\np $cfg->{database}{host};\nmy @items = @{ rj(\"list.json\") };\n@items |> e { p $_->{name} };\n```\n\nNote: numeric strings remain strings; use `+0` to coerce if needed.",
        "write_json" | "wj" => "Serialize a perlrs data structure (hash ref or array ref) as pretty-printed JSON and write it to a file. Creates or overwrites the target file. The short alias `wj` pairs with `rj` for round-trip JSON workflows. Useful for persisting configuration, caching API responses, or generating fixture data.\n\n```perl\nmy %data = (name => \"Alice\", scores => [98, 87, 95]);\nwj(\"out.json\", \\%data);\nmy $back = rj(\"out.json\");\np $back->{name};   # Alice\n```",

        // ── Compression ──
        "gzip" => "Compress a string or byte buffer using the gzip (RFC 1952) format and return the compressed bytes. Useful for shrinking data before writing to disk or sending over the network. Pairs with `gunzip` for decompression. The compression level is chosen automatically for a good speed/size tradeoff.\n\n```perl\nmy $raw = \"hello world\" x 1000;\nmy $gz = gzip($raw);\nto_file(\"data.gz\", $gz);\np length($gz);       # much smaller than original\np gunzip($gz) eq $raw;  # 1\n```",
        "gunzip" => "Decompress gzip-compressed data (RFC 1952) and return the original bytes. Dies if the input is not valid gzip. Use this to read `.gz` files or decompress data received from HTTP responses with `Content-Encoding: gzip`. Always the inverse of `gzip`.\n\n```perl\nmy $compressed = rl(\"archive.gz\");\nmy $text = gunzip($compressed);\np $text;\n# round-trip in a pipeline\n\"payload\" |> gzip |> gunzip |> p;  # payload\n```",
        "zstd" => "Compress a string or byte buffer using the Zstandard algorithm and return the compressed bytes. Zstandard offers significantly better compression ratios and speed compared to gzip, making it ideal for large datasets, IPC buffers, and caching. Pairs with `zstd_decode` for decompression.\n\n```perl\nmy $big = \"x]\" x 100_000;\nmy $compressed = zstd($big);\np length($compressed);  # fraction of original\nto_file(\"data.zst\", $compressed);\np zstd_decode($compressed) eq $big;  # 1\n```",
        "zstd_decode" => "Decompress Zstandard-compressed data and return the original bytes. Dies if the input is not valid Zstandard. This is the inverse of `zstd`. Use it to read `.zst` files or decompress cached buffers that were compressed with `zstd`.\n\n```perl\nmy $packed = zstd(\"important data\\n\" x 500);\nmy $original = zstd_decode($packed);\np $original;\n# file round-trip\nto_file(\"cache.zst\", zstd($payload));\np zstd_decode(rl(\"cache.zst\"));\n```",

        // ── URL encoding ──
        "url_encode" | "uri_escape" => "Percent-encode a string so it is safe to embed in a URL query parameter or path segment. Unreserved characters (alphanumeric, `-`, `_`, `.`, `~`) are left as-is; everything else becomes `%XX`. The alias `uri_escape` matches the classic `URI::Escape` name for Perl muscle-memory.\n\n```perl\nmy $q = \"hello world & friends\";\nmy $safe = url_encode($q);\np $safe;   # hello%20world%20%26%20friends\nmy $url = \"https://example.com/search?q=\" . url_encode($q);\np $url;\n```\n\nNote: does not encode the full URL structure — encode individual components, not the whole URL.",
        "url_decode" | "uri_unescape" => "Decode a percent-encoded string back to its original form, converting `%XX` sequences to the corresponding bytes and `+` to space. The alias `uri_unescape` matches `URI::Escape` conventions. Use this when parsing query strings from incoming URLs or reading URL-encoded form data.\n\n```perl\nmy $encoded = \"hello%20world%20%26%20friends\";\np url_decode($encoded);   # hello world & friends\n# round-trip\nmy $orig = \"café ☕\";\np url_decode(url_encode($orig)) eq $orig;  # 1\n```",

        // ── Logging ──
        "log_info" => "Log a message at INFO level to stderr with a timestamp prefix. INFO is the default visible level and is appropriate for normal operational messages — startup notices, progress milestones, summary statistics. Messages are suppressed if the current log level is set higher than INFO.\n\n```perl\nlog_info(\"server started on port $port\");\nmy @rows = rl(\"data.csv\");\nlog_info(\"loaded \" . scalar(@rows) . \" rows\");\n1..5 |> e { log_info(\"processing item $_\") };\n```",
        "log_warn" => "Log a message at WARN level to stderr. Warnings indicate unexpected but recoverable situations — missing optional config, deprecated usage, slow operations. WARN messages appear at the default log level and are visually distinct from INFO in structured log output.\n\n```perl\nlog_warn(\"config file not found, using defaults\");\nlog_warn(\"query took ${elapsed}s, exceeds threshold\");\nunless (-e $path) {\n    log_warn(\"$path missing, skipping\");\n}\n```",
        "log_error" => "Log a message at ERROR level to stderr. Use this for failures that prevent an operation from completing but do not necessarily terminate the program — failed network requests, invalid input, permission errors. ERROR is always visible regardless of log level.\n\n```perl\nlog_error(\"failed to connect to $host: $!\");\neval { rj(\"bad.json\") };\nlog_error(\"parse failed: $@\") if $@;\nlog_error(\"missing required field 'name'\");\n```",
        "log_debug" => "Log a message at DEBUG level to stderr. Debug messages are hidden by default and only appear when the log level is lowered to DEBUG or TRACE via `log_level`. Use for detailed internal state that helps during development — variable dumps, branch decisions, intermediate values.\n\n```perl\nlog_level(\"debug\");\nlog_debug(\"cache key: $key\");\nmy $result = compute($x);\nlog_debug(\"compute($x) => $result\");\n@items |> e { log_debug(\"item: $_\") };\n```",
        "log_trace" => "Log a message at TRACE level to stderr. This is the most verbose level, producing very fine-grained output — loop iterations, function entry/exit, raw payloads. Only visible when `log_level(\"trace\")` is set. Use sparingly in production code; primarily for deep debugging sessions.\n\n```perl\nlog_level(\"trace\");\nfn process($x) {\n    log_trace(\"entering process($x)\");\n    my $r = $x * 2;\n    log_trace(\"leaving process => $r\");\n    $r\n}\n1..3 |> map { process($_) } |> e p;\n```",
        "log_json" => "Emit a structured JSON log line to stderr containing the message plus any additional key-value metadata. This is designed for machine-parseable logging pipelines — centralized log collectors, JSON-based monitoring, or `jq`-friendly output. Each call emits exactly one JSON object per line.\n\n```perl\nlog_json(\"request\", method => \"GET\", path => \"/api\");\nlog_json(\"metric\", name => \"latency_ms\", value => 42);\nlog_json(\"error\", msg => $@, file => $0);\n```\n\nNote: all values are serialized as JSON strings.",
        "log_level" => "Get or set the current minimum log level. When called with no arguments, returns the current level as a string. When called with a level name, sets it for all subsequent log calls. Valid levels from most to least verbose: `trace`, `debug`, `info`, `warn`, `error`. The default level is `info`.\n\n```perl\np log_level();         # info\nlog_level(\"debug\");    # enable debug output\nlog_debug(\"now visible\");\nlog_level(\"error\");    # suppress everything below error\nlog_info(\"hidden\");    # not printed\n```",

        // ── Datetime ──
        "datetime_utc" => "Return current UTC datetime as ISO 8601 string.",
        "datetime_from_epoch" => "Convert epoch seconds to ISO 8601 string.",
        "datetime_strftime" => "`datetime_strftime($format, $epoch)` — format epoch as datetime.",
        "datetime_now_tz" => "`datetime_now_tz($tz)` — current time in a timezone.",
        "datetime_format_tz" => "Format an epoch in a specific timezone.",
        "datetime_parse_local" => "Parse a local datetime string to epoch.",
        "datetime_parse_rfc3339" => "Parse an RFC 3339 datetime string to epoch.",
        "datetime_add_seconds" => "Add seconds to an ISO 8601 datetime string.",
        "elapsed" | "el" => "Seconds since process start (monotonic): `elapsed()`.",
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
        "gethostbyaddr" => "Reverse DNS lookup: `gethostbyaddr($addr)`.",
        "getpwent" => "Read the next entry from the password database.",
        "getgrent" => "Read the next entry from the group database.",
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
        "not" => "`not EXPR` — low-precedence logical negation (same as `!` but binds looser).",
        "syscall" => "`syscall NUMBER, LIST` — call a system call by number (platform-specific).",

        // ── perlrs extensions (syntax / macros) ──
        "thread" | "t" => "Clojure-inspired threading macro — chain stages without repeating `|>`.\n\n```perl\nthread @data grep { $_ > 5 } map { $_ * 2 } sort { $_0 <=> $_1 } |> join \",\" |> p\nt \" hello \" tm uc rv lc ufc sc cc kc tj p  # short aliases\n```\n\nStages: bare function, function with block, `>{}` anonymous block.\n`|>` terminates the thread macro.",
        "fn" => "Alias for `sub` — define a function.\n\n```perl\nfn double($x) { $x * 2 }\nmy $f = fn { $_ * 2 };\nmy $add = fn ($a: Int, $b: Int) { $a + $b };\n```",
        "mysync" => "Declare shared variables for parallel blocks (`Arc<Mutex>`).\n\n```perl\nmysync $counter = 0;\nfan 10000 { $counter++ };   # always exactly 10000\nmysync @results;\nmysync %histogram;\n```\n\nCompound ops (`++`, `+=`, `.=`, `|=`, `&=`) are fully atomic.",
        "frozen" => "Declare an immutable lexical variable.\n\n```perl\nfrozen my $pi = 3.14159;\n# $pi = 3;  # ERROR: cannot assign to frozen variable\n```",
        "match" => "Algebraic pattern matching (perlrs extension).\n\n```perl\nmatch ($val) {\n    /^\\d+$/ => p \"number: $val\",\n    [1, 2, _] => p \"array starting with 1,2\",\n    { name => $n } => p \"name is $n\",\n    _ => p \"default\",\n}\n```\n\nPatterns: regex, array, hash, literal, wildcard `_`. Optional `if` guard per arm.",
        "|>" => "Pipe-forward operator — threads LHS as first argument of RHS call.\n\n```perl\n\"hello\" |> uc |> rev |> p;              # OLLEH\n1..10 |> grep $_ > 5 |> map $_ * 2 |> e p;\n$url |> fetch_json |> json_jq '.name' |> p;\n\"hello world\" |> s/world/perl/ |> p;     # hello perl\n```\n\nZero runtime cost (parse-time desugaring). Binds looser than `||`, tighter than `?:`.",
        "pipe" | "CORE::pipe" => "Create a pipe between two filehandles: `pipe(READ, WRITE)`.",
        "gen" => "Create a generator — lazy `yield` values on demand.\n\n```perl\nmy $g = gen { yield $_ for 1..5 };\nmy ($val, $more) = @{$g->next};\n```",
        "yield" => "Yield a value from inside a `gen { }` generator block.",
        "trace" => "Trace `mysync` mutations to stderr (tagged with worker index under `fan`).\n\n```perl\ntrace { fan 10 { $counter++ } };\n```",
        "timer" => "Measure wall-clock milliseconds for a block.\n\n```perl\nmy $ms = timer { heavy_work() };\n```",
        "bench" => "Benchmark a block N times; returns `\"min/mean/p99\"`.\n\n```perl\nmy $report = bench { work() } 1000;\n```",
        "eval_timeout" => "Run a block with a wall-clock timeout (seconds).\n\n```perl\neval_timeout 5 { slow_operation() };\n```",
        "retry" => "Retry a block on failure.\n\n```perl\nretry { http_call() } times => 3, backoff => 'exponential';\n```",
        "rate_limit" => "Limit invocations per time window.\n\n```perl\nrate_limit(10, \"1s\") { hit_api() };\n```",
        "every" => "Run a block at a fixed interval.\n\n```perl\nevery \"500ms\" { tick() };\n```",
        "fore" | "e" => "Side-effect-only list iterator (like `map` but void, returns item count).\n\n```perl\nqw(a b c) |> e p;           # prints a, b, c; returns 3\n1..5 |> map $_ * 2 |> e p;  # prints 2,4,6,8,10\n```",
        "p" => "`p` — alias for `say` (print with newline).\n\n```perl\np \"hello\";       # hello\\n\np 42;            # 42\\n\n1..5 |> e p;     # prints each on its own line\n```",
        "watch" => "Watch a single file for changes (non-parallel).\n\n```perl\nwatch \"/tmp/x\", sub { process($_) };\n```",
        "glob_par" => "Parallel recursive glob: `\"**/*.log\" |> glob_par`.",
        "par_find_files" => "`par_find_files DIR, GLOB` — parallel recursive file search by glob.",
        "par_line_count" => "`par_line_count @files` — parallel line count across files.",
        "capture" => "Run a command and capture structured output.\n\n```perl\nmy $r = capture(\"ls -la\");\np $r->stdout, $r->stderr, $r->exit;\n```",
        "input" => "Slurp all of stdin (or a filehandle) as one string.\n\n```perl\nmy $all = input;          # slurp stdin\nmy $fh_data = input($fh); # slurp filehandle\n```",
        "slurp" | "sl" => "Read an entire file as a string: `slurp(\"file.txt\")`.",

        _ => return None,
    };
    Some(md)
}

/// Public entry point for `pe docs TOPIC` — returns raw markdown doc text.
pub fn doc_text_for(label: &str) -> Option<&'static str> {
    doc_for_label_text(label)
}

/// List all documented topic names (sorted, deduplicated).
pub fn doc_topics() -> Vec<&'static str> {
    include_str!("lsp_completion_words.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|l| doc_for_label_text(l).is_some())
        .collect()
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
