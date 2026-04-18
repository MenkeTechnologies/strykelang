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
        StmtKind::EnumDecl { def } => {
            symbols.push(sym(
                format!("enum {}", def.name),
                SymbolKind::ENUM,
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
        | StmtKind::EnumDecl { .. }
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
        "my" => "Declare a lexically scoped variable visible only within the enclosing block or file. `my` is the workhorse of Perl variable declarations — use it for scalars (`$`), arrays (`@`), and hashes (`%`). Variables declared with `my` are invisible outside their scope, which prevents accidental cross-scope mutation. In perlrs, `my` variables participate in pipe chains and can be destructured in list context. Uninitialized `my` variables default to `undef`.\n\n```perl\nmy $name = \"world\"\nmy @nums = 1..5\nmy %cfg = (debug => 1, verbose => 0)\np \"hello $name\"\n@nums |> grep $_ > 2 |> e p$1\n\nUse `my` everywhere unless you specifically need `our` (package global), `local` (dynamic scope), or `state` (persistent).",
        "our" => "Declare a package-global variable that is accessible as a lexical alias in the current scope. Unlike `my`, `our` variables are visible across the entire package and can be accessed from other packages via their fully qualified name (e.g. `$Counter::total`). This is useful for package-level configuration, shared counters, or variables that need to survive across file boundaries. In perlrs, `our` variables are not mutex-protected — use `mysync` instead for parallel-safe globals.\n\n```perl\npackage Counter\nour $total = 0\nfn bump { $total++ }\npackage main\nCounter::bump() for 1..5\np $Counter::total;   # 5\n```\n\nPrefer `my` for local state; reach for `our` only when other packages need access.",
        "local" => "Temporarily override a global variable's value for the duration of the current dynamic scope. When the scope exits, the original value is automatically restored. This is essential for modifying Perl's special variables (like `$/`, `$\\`, `$,`) without permanently altering global state. Unlike `my` which creates a new variable, `local` saves and restores an existing global. In perlrs, `local` works with all special variables and respects the same restoration semantics during exception unwinding.\n\n```perl\nlocal $/ = undef;       # slurp mode\nopen my $fh, '<', 'data.txt' or die $!\nmy $body = <$fh>       # reads entire file\nclose $fh\n# $/ restored to \"\\n\" when scope exits\n```\n\nCommon patterns: `local $/ = undef` (slurp), `local $, = \",\"` (join print args), `local %ENV` (temporary env).",
        "state" => "Declare a persistent lexical variable that retains its value across calls to the enclosing subroutine. Unlike `my`, which reinitializes on each call, `state` initializes only once — the first time execution reaches the declaration — and preserves the value for all subsequent calls. This is perfect for counters, caches, and memoization without resorting to globals or closures over external variables. In perlrs, `state` variables are per-thread when used inside `t { }` or `fan` blocks; they are not shared across workers.\n\n```perl\nfn counter { state $n = 0; ++$n }\np counter() for 1..5;   # 1 2 3 4 5\nfn memo($x) {\n    state %cache\n    $cache{$x} //= expensive($x)\n}\n```\n\nRequires `use feature 'state'` in standard Perl, but is always available in perlrs.",
        "sub" => "Define a named or anonymous subroutine. In perlrs, the preferred shorthand is `fn`, which behaves identically but is shorter and supports optional typed parameters. Named subs are installed into the current package; anonymous subs (closures) capture their enclosing lexical scope. Subroutines are first-class values — assign them to scalars, store in arrays, pass as callbacks. The last expression evaluated is the implicit return value unless an explicit `return` is used.\n\n```perl\nfn greet($who) { p \"hello $who\" }\nmy $sq = fn ($x) { $x ** 2 }\n1..5 |> map $sq->($_) |> e p\nfn apply($f, @args) { $f->(@args) }\napply($sq, 7) |> p;   # 49\n```\n\nUse `fn` for new perlrs code; `sub` is fully supported for Perl compatibility.",
        "package" => "Set the current package namespace for all subsequent declarations. Package names are conventionally `CamelCase` with `::` separators (e.g. `Math::Utils`). All unqualified sub and `our` variable names are installed into the current package. In perlrs, packages work identically to standard Perl — they provide namespace isolation but are not classes by themselves (use `struct` or `bless` for OOP). Switching packages mid-file is allowed but discouraged; prefer one package per file.\n\n```perl\npackage Math::Utils\nfn factorial($n) { $n <= 1 ? 1 : $n * factorial($n - 1) }\nfn fib($n) { $n < 2 ? $n : fib($n - 1) + fib($n - 2) }\npackage main\np Math::Utils::factorial(10);   # 3628800\n```",
        "use" => "Load and import a module at compile time: `use Module qw(func);`.\n\n```perl\nuse List::Util qw(sum max)\nmy @vals = 1..10\np sum(@vals);   # 55\np max(@vals);   # 10\n```",
        "no" => "Unimport a module or pragma: `no strict 'refs';`.\n\n```perl\nno warnings 'experimental'\ngiven ($x) { when (1) { p \"one\" } }\n```",
        "require" => "Load a module at runtime: `require Module;`.\n\n```perl\nrequire JSON\nmy $data = JSON::decode_json($text)\np $data->{name}\n```",
        "return" => "Return a value from a subroutine.\n\n```perl\nfn clamp($v, $lo, $hi) {\n    return $lo if $v < $lo\n    return $hi if $v > $hi\n    $v\n}\n```",
        "BEGIN" => "`BEGIN { }` — runs at compile time, before the rest of the program.\n\n```perl\nBEGIN { p \"compiling...\" }\np \"running\"\n# output: compiling... then running\n```",
        "END" => "`END { }` — runs after the program finishes (or on exit).\n\n```perl\nEND { p \"cleanup done\" }\np \"main work\"\n# output: main work then cleanup done\n```",

        // ── Control flow ──
        "if" => "The fundamental conditional construct. Evaluates its condition in boolean context and executes the block if true. perlrs supports both the block form `if (COND) { BODY }` and the postfix form `EXPR if COND`, which is idiomatic for single-statement guards. The condition can be any expression — numbers (0 is false), strings (empty and `\"0\"` are false), undef (false), and references (always true). Postfix `if` cannot have `elsif`/`else` — use the block form for multi-branch logic.\n\n```perl\nmy $n = 42\nif ($n > 0) { p \"positive\" }\np \"big\" if $n > 10;          # postfix — clean one-liner\nmy $label = \"even\" if $n % 2 == 0\np \"got: $n\" if defined $n;   # guard against undef\n```",
        "elsif" => "Chain additional conditions after an `if` block without nesting. Each `elsif` is tested in order; the first one whose condition is true has its block executed, and the rest are skipped. There is no limit on the number of `elsif` branches. In perlrs, prefer `match` for complex multi-branch dispatch since it supports pattern matching, destructuring, and guards — but `elsif` remains the right tool for simple linear condition chains. Note: it is `elsif`, not `elseif` or `else if` — the latter is a syntax error.\n\n```perl\nfn classify($n) {\n    if    ($n < 0)   { \"negative\" }\n    elsif ($n == 0)  { \"zero\" }\n    elsif ($n < 10)  { \"small\" }\n    elsif ($n < 100) { \"medium\" }\n    else             { \"large\" }\n}\n1..200 |> map classify |> frequencies |> dd\n```",
        "else" => "The final fallback branch of an `if`/`elsif` chain, executed when no preceding condition was true. Every `if` can have at most one `else`, and it must come last. For ternary-style expressions, use `COND ? A : B` instead of an `if`/`else` block — it composes better in `|>` pipes and assignments.\n\n```perl\nif ($n % 2 == 0) { p \"even\" }\nelse             { p \"odd\" }\n\n# Ternary is often cleaner in pipes:\n1..10 |> map { $_ % 2 == 0 ? \"even\" : \"odd\" } |> e p\n```",
        "unless" => "A negated conditional — executes the block when the condition is *false*. This reads more naturally than `if (!COND)` for guard clauses and early returns. perlrs supports both block and postfix forms. Convention: use `unless` for simple negative guards; avoid `unless` with complex compound conditions, as double-negatives hurt readability. There is no `unlessif` — use `if`/`elsif` chains for multi-branch logic.\n\n```perl\nunless ($ENV{QUIET}) { p \"verbose output\" }\np \"missing!\" unless -e $path;   # postfix guard\ndie \"no input\" unless @ARGV\nreturn unless defined $val;    # early return pattern\n```",
        "foreach" | "for" => "Iterate over a list, binding each element to a loop variable (or `$_` by default). `for` and `foreach` are interchangeable keywords. The loop variable is automatically localized and aliases the original element — modifications to `$_` inside the loop mutate the list in-place. In perlrs, `for` loops work with ranges, arrays, hash slices, and iterator results. For parallel iteration, see `pfor`; for pipeline-style processing, prefer `|> e` or `|> map`. C-style `for (INIT; COND; STEP)` is also supported.\n\n```perl\nfor my $f (glob \"*.txt\") { p $f }\nfor (1..5) { p $_ * 2 }            # $_ is default\nmy @names = qw(alice bob carol)\nfor (@names) { $_ = uc $_ }         # mutates in-place\np join \", \", @names;                 # ALICE, BOB, CAROL\n```",
        "while" => "Loop that re-evaluates its condition before each iteration and continues as long as it is true. Commonly used for reading input line-by-line, polling, and indefinite iteration. The condition is tested in boolean context. `while` integrates naturally with the diamond operator `<>` for reading filehandles. The loop variable can be declared in the condition with `my`, scoping it to the loop body. Postfix form is also supported: `EXPR while COND;`.\n\n```perl\nwhile (my $line = <STDIN>) {\n    $line |> tm |> p;             # trim + print each line\n}\nmy $i = 0\nwhile ($i < 5) { p $i; $i++ }    # counted loop\nwhile (1) { last if done(); }     # infinite loop with break\n```",
        "until" => "Loop that continues as long as its condition is *false* — the logical inverse of `while`. Useful when the termination condition is more naturally expressed as a positive assertion (\"keep going until X happens\"). Supports both block and postfix forms. Prefer `while` with a negated condition if `until` makes the logic harder to read.\n\n```perl\nmy $n = 1\nuntil ($n > 1000) { $n *= 2 }\np $n;   # 1024\n\nmy $tries = 0\nuntil (connected()) {\n    $tries++\n    sleep 1\n}\np \"connected after $tries tries\"\n```",
        "do" => "Execute a block and return its value, or execute a file. As a block, `do { ... }` creates an expression scope — the last expression in the block is the return value, making it useful for complex initializations. As a file operation, `do \"file.pl\"` executes the file in the current scope and returns its last expression. Unlike `require`, `do` does not cache and re-executes on each call. `do { ... } while (COND)` creates a loop that always runs at least once.\n\n```perl\nmy $val = do { my $x = 10; $x ** 2 };   # 100\np $val\nmy $cfg = do \"config.pl\";               # load config\n# do-while: body runs at least once\nmy $input\ndo { $input = readline(STDIN) |> tm } while ($input eq \"\")\n```",
        "last" => "Immediately exit the innermost enclosing loop (equivalent to `break` in C/Rust). Execution continues after the loop. `last LABEL` can target a labeled outer loop to break out of nested loops. Works in `for`, `foreach`, `while`, `until`, and `do-while`. Does *not* work inside `map`, `grep`, or `|>` pipeline stages — use `take`, `first`, or `take_while` for early termination in functional contexts.\n\n```perl\nfor (1..1_000_000) {\n    last if $_ > 5\n    p $_\n}   # prints 1 2 3 4 5\n\nOUTER: for my $i (1..10) {\n    for my $j (1..10) {\n        last OUTER if $i * $j > 50;   # break both loops\n    }\n}\n```\n\nFor pipeline early-exit: `1..1000 |> take_while { $_ < 50 } |> e p`.",
        "next" => "Skip the rest of the current loop iteration and jump to the next one. The loop condition (for `while`/`until`) or the next element (for `for`/`foreach`) is evaluated immediately. Like `last`, `next` supports labeled loops with `next LABEL` for skipping in nested loops. This is the primary tool for filtering within imperative loops. In perlrs, consider `grep`/`filter` or `|> reject` for functional-style filtering instead.\n\n```perl\nfor (1..10) {\n    next if $_ % 2;        # skip odds\n    p $_\n}   # 2 4 6 8 10\n\nfor my $file (glob \"*\") {\n    next unless -f $file;   # skip non-files\n    next if $file =~ /^\\./  # skip hidden\n    p $file\n}\n```",
        "redo" => "Restart the current loop iteration from the top of the loop body *without* re-evaluating the loop condition or advancing to the next element. The loop variable retains its current value. This is a niche but powerful tool for retry logic within loops — when an iteration fails, `redo` lets you try again with the same input. Use sparingly, as it can create infinite loops if the retry condition never resolves. Always pair with a guard or counter. For automated retry with backoff, prefer `retry { ... } times => N, backoff => 'exponential'`.\n\n```perl\nfor my $url (@urls) {\n    my $body = eval { fetch($url) }\n    if ($@) {\n        warn \"retry $url: $@\"\n        sleep 1\n        redo;   # try same URL again\n    }\n    p length($body)\n}\n```",
        "continue" => "A block attached to a `for`/`foreach`/`while` loop that executes after each iteration, even when `next` is called. Analogous to the increment expression in a C-style `for` loop. The `continue` block does *not* run when `last` or `redo` is used. Useful for unconditional per-iteration bookkeeping like incrementing counters, logging progress, or flushing buffers. Rarely used but fully supported in perlrs.\n\n```perl\nmy $count = 0\nfor my $item (@work) {\n    next if $item->{skip}\n    process($item)\n} continue {\n    $count++\n    p \"processed $count so far\" if $count % 100 == 0\n}\n```",
        "given" => "A switch-like construct that evaluates an expression and dispatches to `when` blocks via smartmatch semantics. The topic variable `$_` is set to the `given` expression for the duration of the block. Each `when` clause is tested in order; the first match executes its block and control passes out of the `given` (implicit break). A `default` block handles the no-match case. In perlrs, prefer the `match` keyword for new code — it offers pattern destructuring, typed patterns, array/hash shape matching, and `if` guards that `given`/`when` cannot express.\n\n```perl\ngiven ($cmd) {\n    when (\"start\")   { p \"starting up\" }\n    when (\"stop\")    { p \"shutting down\" }\n    when (/^re/)     { p \"restarting\" }\n    default          { p \"unknown: $cmd\" }\n}\n```\n\nSee `match` for perlrs-native pattern matching with destructuring.",
        "when" => "A case clause inside a `given` block. The expression is matched against the topic `$_` using smartmatch semantics: strings match exactly, regexes match against `$_`, arrayrefs check membership, coderefs are called with `$_` as argument, and numbers compare numerically. When a `when` clause matches, its block executes and control exits the enclosing `given` (implicit break). Multiple `when` clauses are tried in order until one matches.\n\n```perl\ngiven ($val) {\n    when (/^\\d+$/)      { p \"number\" }\n    when ([\"a\",\"b\",\"c\"]) { p \"early letter\" }\n    when (42)            { p \"the answer\" }\n    default              { p \"something else\" }\n}\n```\n\nIn perlrs, the `match` keyword provides more powerful pattern matching.",
        "default" => "The fallback clause in a `given` block, executed when no `when` clause matched. Every `given` should have a `default` to handle unexpected values, similar to `else` in an `if` chain or the wildcard `_` arm in perlrs `match`. If no `default` is present and nothing matches, execution simply continues after the `given` block. In perlrs `match`, use `_ => ...` for the default arm instead.\n\n```perl\ngiven ($exit_code) {\n    when (0) { p \"success\" }\n    when (1) { p \"general error\" }\n    when (2) { p \"misuse\" }\n    default  { p \"unknown exit code: $exit_code\" }\n}\n```",

        // ── Exception handling ──
        "try" => "Structured exception handling that cleanly separates the happy path from error recovery. The `try` block runs the code; if it throws (via `die`), execution jumps to the `catch` block with the exception bound to the declared variable. An optional `finally` block runs unconditionally afterward — ideal for cleanup like closing filehandles or releasing locks. Unlike `eval`, `try`/`catch` is a first-class statement with proper scoping and no `$@` pollution. In perlrs, `try` integrates with all exception types including string messages, hashrefs, and objects.\n\n```perl\ntry {\n    my $data = fetch_json($url)\n    p $data->{name}\n} catch ($e) {\n    warn \"request failed: $e\"\n    return fallback()\n} finally {\n    log_info(\"fetch attempt complete\")\n}\n```\n\nPrefer `try`/`catch` over `eval { }` for new code — it reads better and avoids `$@` clobbering races.",
        "catch" => "The error-handling clause that follows a `try` block. When the `try` block throws an exception, execution transfers to `catch` with the exception value bound to the declared variable (e.g. `$e`). The catch variable is lexically scoped to the catch block. You can inspect the exception — it may be a string, a hashref with structured error info, or an object with methods. Multiple error types can be differentiated with `ref` or `match` inside the catch body. If the catch block itself throws, the exception propagates upward (the `finally` block still runs first, if present).\n\n```perl\ntry { die { code => 404, msg => \"not found\" } }\ncatch ($e) {\n    if (ref $e eq 'HASH') {\n        p \"error $e->{code}: $e->{msg}\"\n    } else {\n        p \"caught: $e\"\n    }\n}\n```",
        "finally" => "A cleanup block that runs after `try`/`catch` regardless of whether an exception was thrown or not. This guarantees resource cleanup even if the `try` block throws or the `catch` block re-throws. The `finally` block cannot change the exception or the return value — it is strictly for side effects like closing filehandles, releasing locks, or logging. If `finally` itself throws, that exception replaces the original one (avoid throwing in finally). `finally` is optional — you can use `try`/`catch` without it.\n\n```perl\nmy $fh\ntry {\n    open $fh, '<', $path or die $!\n    process(<$fh>)\n} catch ($e) {\n    log_error(\"failed: $e\")\n} finally {\n    close $fh if $fh;   # always cleanup\n}\n```",
        "eval" => "The classic Perl exception-catching mechanism. `eval { BLOCK }` executes the block in an exception-trapping context: if the block throws (via `die`), execution continues after the `eval` with the error stored in `$@`. `eval \"STRING\"` compiles and executes Perl code at runtime — powerful but dangerous (code injection risk). In perlrs, prefer `try`/`catch` for exception handling as it avoids the `$@` clobbering pitfalls and reads more clearly. `eval` remains useful for dynamic code evaluation and backward compatibility with Perl 5 idioms.\n\n```perl\neval { die \"oops\" }\nif ($@) { p \"caught: $@\" }\n\n# Eval string — dynamic code execution\nmy $expr = \"2 + 2\"\nmy $result = eval $expr\np $result;   # 4\n```\n\nCaveat: `$@` can be clobbered by intervening `eval`s or destructors — `try`/`catch` avoids this.",
        "die" => "Raise an exception, immediately unwinding the call stack until caught by `try`/`catch` or `eval`. The argument can be a string (most common), a reference (hashref for structured errors), or an object. If uncaught, the program terminates and the message is printed to STDERR. In perlrs, `die` works identically to Perl 5 and integrates with `try`/`catch`, `eval`, and the `$@` mechanism. Convention: end die messages with `\\n` to suppress the automatic \"at FILE line LINE\" suffix, or omit it to get location info for debugging.\n\n```perl\nfn divide($a, $b) {\n    die \"division by zero\\n\" unless $b\n    $a / $b\n}\n\n# Structured error\ndie { code => 400, msg => \"bad request\", field => \"email\" }\n\n# With automatic location\ndie \"something broke\";   # prints: something broke at script.pl line 5.\n```",
        "warn" => "Print a warning message to STDERR without terminating the program. Behaves like `die` but only emits the message instead of throwing an exception. If the message does not end with `\\n`, Perl appends the current file and line number. Warnings can be intercepted with `$SIG{__WARN__}` or suppressed with `no warnings`. In perlrs, `warn` is useful for non-fatal diagnostics; for structured logging, use `log_warn` instead.\n\n```perl\nwarn \"file not found: $path\" unless -e $path\nwarn \"deprecated: use fetch_json instead\\n\";  # no line number\n\n# Intercept warnings\nlocal $SIG{__WARN__} = fn ($msg) { log_warn($msg) }\nwarn \"redirected to logger\"\n```",
        "croak" => "Die from the caller's perspective — the error message reports the file and line of the *caller*, not the function that called `croak`. This is the right choice for library/module functions where the error is the caller's fault (bad arguments, misuse). Without `croak`, the user sees an error pointing at library internals, which is unhelpful. In perlrs, `croak` is available as a builtin without `use Carp`. For debugging deep call chains, use `confess` instead to get a full stack trace.\n\n```perl\nfn parse($s) {\n    croak \"parse: empty input\" unless length $s\n    croak \"parse: not JSON\" unless $s =~ /^[{\\[]/\n    json_decode($s)\n}\n\n# Error will point at the call site, not inside parse()\nmy $data = parse(\"\");   # dies: \"parse: empty input at caller.pl line 5\"\n```",
        "confess" => "Die with a full stack trace from the point of the error all the way up through every caller. This is invaluable for debugging deep call chains where `die` or `croak` only show one frame. Each frame includes the package, file, and line number. In perlrs, `confess` is available as a builtin without `use Carp`. Use `confess` during development for maximum diagnostic info; switch to `croak` in production-facing libraries where the trace would confuse end users.\n\n```perl\nfn validate($data) {\n    confess \"missing required field 'name'\" unless $data->{name}\n}\nfn process($input) { validate($input) }\nfn main { process({}) };   # full trace: main -> process -> validate\n```\n\nThe trace output shows: `missing required field 'name' at script.pl line 2.\\n\\tmain::validate called at line 4\\n\\t...`.",

        // ── I/O ──
        "say" => "Print operands followed by an automatic newline to `STDOUT`. In perlrs, `say` is always available without `-E` or `use feature 'say'` — it is a first-class builtin. The shorthand `p` is an alias for `say` and is preferred in most perlrs code. When given a list, `say` joins elements with `$,` (output field separator, empty by default) and appends `$\\` plus a newline. For streaming output over pipelines, combine with `e` (each) to print one element per line. Gotcha: `say` always adds a newline — if you need raw output without one, use `print` instead.\n\n```perl\np \"hello world\"\nmy @names = (\"alice\", \"bob\", \"eve\")\n@names |> e p\n1..5 |> maps { $_ * 2 } |> e p\n```",
        "print" => "Write operands to the selected output handle (default `STDOUT`) without appending a newline. The output field separator `$,` is inserted between arguments, and `$\\` is appended at the end — both default to empty string. You can direct output to a specific handle by passing it as the first argument with no comma: `print STDERR \"msg\"`. In perlrs, `print` behaves identically to Perl 5 and is useful when you need precise control over output formatting, such as building progress bars, writing binary data, or emitting partial lines. For most line-oriented output, prefer `p` (say) instead since it handles the newline automatically.\n\n```perl\nprint \"no newline\"\nprint STDERR \"error msg\\n\"\nprint $fh \"data to filehandle\\n\"\nfor my $pct (0..100) {\n    printf \"\\rprogress: %3d%%\", $pct\n}\nprint \"\\n\"\n```",
        "printf" => "Formatted print to a filehandle (default `STDOUT`), using C-style format specifiers. The first argument is the format string with `%s` (string), `%d` (integer), `%f` (float), `%x` (hex), `%o` (octal), `%e` (scientific), `%g` (general float), and `%%` (literal percent). Width and precision modifiers work as in C: `%-10s` left-aligns in a 10-char field, `%05d` zero-pads to 5 digits, `%.2f` gives 2 decimal places. In perlrs, `printf` supports all standard Perl 5 format specifiers including `%v` (version strings) and `%n` is disabled for safety. Direct output to a handle by placing it before the format: `printf STDERR \"...\", @args`. Unlike `sprintf`, `printf` outputs directly and returns a boolean indicating success.\n\n```perl\nprintf \"%-10s %5d\\n\", $name, $score\nprintf STDERR \"error %d: %s\\n\", $code, $msg\nprintf \"%08.2f\\n\", 3.14159;   # 00003.14\nprintf \"%s has %d item%s\\n\", $user, $n, $n == 1 ? \"\" : \"s\"\n```",
        "sprintf" => "Return a formatted string without printing it, using the same C-style format specifiers as `printf`. This is the go-to function for building formatted strings for later use — constructing log messages, building padded table columns, converting numbers to hex or binary representations, or assembling strings that will be passed to other functions. In perlrs, `sprintf` is often combined with the pipe operator: `$val |> t { sprintf \"0x%04x\", $_ } |> p`. The return value is always a string. All format specifiers from `printf` apply here.\n\n```perl\nmy $hex = sprintf \"0x%04x\", 255\nmy $padded = sprintf \"%08d\", $id\nmy $msg = sprintf \"%-20s: %s\", $key, $value\nmy @rows = map { sprintf \"%3d. %s\", $_, $names[$_] } 0..$;#names\n@rows |> e p\n```",
        "open" => "Open a filehandle for reading, writing, appending, or piping. The three-argument form `open my $fh, MODE, EXPR` is strongly preferred for safety — it prevents shell injection and makes the mode explicit. Modes include `<` (read), `>` (write/truncate), `>>` (append), `+<` (read-write), `|-` (pipe to command), and `-|` (pipe from command). In perlrs, always use lexical filehandles (`my $fh`) rather than bareword globals. PerlIO layers can be specified in the mode: `<:utf8`, `<:raw`, `<:encoding(UTF-16)`. Always check the return value — `open ... or die \"...: $!\"` is idiomatic. The `$!` variable contains the OS error message on failure. Forgetting to check `open` is one of the most common bugs in Perl code.\n\n```perl\nopen my $fh, '<', 'data.txt' or die \"open: $!\"\nmy @lines = <$fh>\nclose $fh\n\nopen my $out, '>>', 'log.txt' or die \"append: $!\"\nprint $out tm \"event happened\\n\"\nclose $out\n\nopen my $pipe, '-|', 'ls', '-la' or die \"pipe: $!\"\nwhile (<$pipe>) { p tm $_ }\n```",
        "close" => "Close a filehandle, flushing any buffered output and releasing the underlying OS file descriptor. Returns true on success, false on failure — and failure is more common than you might think. For write handles, `close` is where buffered data actually hits disk, so a full disk or network error may only surface at `close` time. Always check the return value when writing: `close $fh or die \"close: $!\"`. For pipe handles opened with `|-` or `-|`, `close` waits for the child process to exit and sets `$?` to the child's exit status. In perlrs, lexical filehandles are automatically closed when they go out of scope, but explicit `close` is clearer and lets you handle errors.\n\n```perl\nopen my $fh, '>', 'out.txt' or die $!\nprint $fh \"done\\n\"\nclose $fh or die \"write failed: $!\"\n\nopen my $p, '|-', 'gzip', '-c' or die $!\nprint $p $data\nclose $p\np \"gzip exited: $?\" if $?\n```",
        "read" => "Read a specified number of bytes from a filehandle into a scalar buffer. The signature is `read FH, SCALAR, LENGTH [, OFFSET]`. Returns the number of bytes actually read (which may be less than requested at EOF or on partial reads), 0 at EOF, or `undef` on error. The optional OFFSET argument lets you append to an existing buffer at a given position, which is useful for accumulating data in a loop. For text files, the bytes are decoded according to the handle's PerlIO layer — use `<:raw` for binary data to avoid encoding transforms. In perlrs, `read` works identically to Perl 5. For line-oriented input, prefer `<$fh>` or `readline` instead.\n\n```perl\nopen my $fh, '<:raw', $path or die $!\nmy $buf = ''\nwhile (read $fh, my $chunk, 4096) {\n    $buf .= $chunk\n}\np \"total: \" . length($buf) . \" bytes\"\nclose $fh\n```",
        "readline" => "Read one line (or all remaining lines in list context) from a filehandle. The angle-bracket operator `<$fh>` is syntactic sugar for `readline($fh)`. In scalar context, returns the next line including the trailing newline (or `undef` at EOF). In list context, returns all remaining lines as a list. The line ending is determined by `$/` (input record separator, default `\\n`). Set `$/ = undef` to slurp the entire file in one read. In perlrs, `readline` integrates with the pipe operator — you can pipe filehandle lines through `maps`, `greps`, and other streaming combinators. Always `chomp` after reading if you don't want trailing newlines.\n\n```perl\nwhile (my $line = <$fh>) {\n    chomp $line\n    p $line\n}\n\n# Slurp entire file\nlocal $/\nmy $content = <$fh>\np length $content\n```",
        "eof" => "Test whether a filehandle has reached end-of-file. Returns 1 if the next read on the handle would return EOF, 0 otherwise. Called without arguments, `eof()` (with parens) checks the last file in the `<>` / `ARGV` stream. Called with no parens as `eof`, it tests whether the current ARGV file is exhausted but more files may follow. In perlrs, `eof` is typically used in `until` loops or as a guard before `read` calls. Note that `eof` may trigger a blocking read on interactive handles (like STDIN from a terminal) to determine if data is available, so avoid calling it speculatively on interactive input. For most line-processing, `while (<$fh>)` is simpler and implicitly handles EOF.\n\n```perl\nuntil (eof $fh) {\n    my $line = <$fh>\n    p tm $line\n}\n\n# Process multiple files via ARGV\nwhile (<>) {\n    p \"new file: $ARGV\" if eof()\n}\n```",
        "seek" => "Reposition a filehandle to an arbitrary byte offset. The signature is `seek FH, POSITION, WHENCE` where WHENCE is 0 (absolute from start), 1 (relative to current position), or 2 (relative to end of file). Use the `Fcntl` constants `SEEK_SET`, `SEEK_CUR`, `SEEK_END` for clarity. Returns 1 on success, 0 on failure. `seek` is essential for random-access file I/O — re-reading headers, skipping to known offsets in binary formats, or rewinding a file for a second pass. In perlrs, `seek` flushes the PerlIO buffer before repositioning. Do not mix `seek`/`tell` with `sysread`/`syswrite` — they use separate buffering layers.\n\n```perl\nseek $fh, 0, 0;         # rewind to start\nmy $header = <$fh>\n\nseek $fh, -100, 2;      # last 100 bytes\nread $fh, my $tail, 100\np $tail\n```",
        "tell" => "Return the current byte offset of a filehandle's read/write position. Returns a non-negative integer on success, or -1 if the handle is invalid or not seekable (e.g., pipes, sockets). Useful for bookmarking a position before a speculative read so you can `seek` back if the data doesn't match expectations. In perlrs, `tell` reflects the PerlIO buffered position, not the raw OS file descriptor position — so it correctly accounts for encoding layers and buffered reads. Pair with `seek` for random-access patterns.\n\n```perl\nmy $pos = tell $fh\np \"at byte $pos\"\n\n# Bookmark and restore\nmy $mark = tell $fh\nmy $line = <$fh>\nunless ($line =~ /^HEADER/) {\n    seek $fh, $mark, 0;   # rewind to before the read\n}\n```",
        "binmode" => "Set the I/O layer on a filehandle, controlling how bytes are translated during reads and writes. Without a layer argument, `binmode $fh` switches the handle to raw binary mode (no CRLF translation on Windows, no encoding transforms). With a layer, `binmode $fh, ':utf8'` enables UTF-8 decoding, `binmode $fh, ':raw'` strips all layers for pure byte I/O, and `binmode $fh, ':encoding(ISO-8859-1)'` sets a specific encoding. In perlrs, encoding layers can also be specified directly in `open`: `open my $fh, '<:utf8', $path`. Call `binmode` before any I/O on the handle — calling it mid-stream may produce garbled output. For binary file processing (images, archives, network protocols), always use `:raw`.\n\n```perl\nopen my $fh, '<', $path or die $!\nbinmode $fh, ':utf8'\nmy @lines = <$fh>\n\nopen my $bin, '<', $img_path or die $!\nbinmode $bin, ':raw'\nread $bin, my $header, 8\np sprintf \"magic: %s\", unpack(\"H*\", $header)\n```",
        "fileno" => "Return the underlying OS file descriptor number for a filehandle, or `undef` if the handle is not connected to a real file descriptor (e.g., tied handles, in-memory handles opened to scalar refs). File descriptor numbers are small non-negative integers managed by the OS kernel: 0 is STDIN, 1 is STDOUT, 2 is STDERR. This function is mainly useful for interfacing with system calls that require raw fd numbers, checking whether two handles share the same underlying fd, or passing descriptors to child processes. In perlrs, `fileno` is rarely needed in everyday code but is essential for low-level I/O multiplexing and process management.\n\n```perl\nmy $fd = fileno STDOUT\np \"stdout fd: $fd\";   # 1\n\nif (defined fileno $fh) {\n    p \"handle is backed by fd \" . fileno($fh)\n} else {\n    p \"not a real file descriptor\"\n}\n```",
        "truncate" => "Truncate a file to a specified byte length. Accepts either a filehandle or a filename as the first argument, and the desired length as the second. `truncate $fh, 0` empties the file entirely — a common pattern when rewriting a file in place. `truncate $fh, $len` discards everything beyond byte `$len`. Returns true on success, false on failure (check `$!` for the error). In perlrs, truncate works on any seekable filehandle. When using truncate to rewrite a file, remember to also `seek $fh, 0, 0` to rewind the write position — truncating does not move the file pointer. Gotcha: truncating a file opened read-only will fail.\n\n```perl\nopen my $fh, '+<', 'data.txt' or die $!\ntruncate $fh, 0;       # empty the file\nseek $fh, 0, 0;        # rewind to start\nprint $fh \"fresh content\\n\"\nclose $fh\n```",
        "flock" => "Advisory file locking for coordinating access between processes. `flock $fh, OPERATION` where OPERATION is `LOCK_SH` (shared/read lock), `LOCK_EX` (exclusive/write lock), or `LOCK_UN` (unlock). Add `LOCK_NB` for non-blocking mode: `LOCK_EX | LOCK_NB` returns false immediately if the lock is held rather than waiting. Import constants from `Fcntl`. Advisory locks are cooperative — they only work if all processes accessing the file use `flock`. In perlrs, `flock` is the standard mechanism for safe concurrent file access in multi-process scripts, cron jobs, and daemons. Always unlock explicitly or let the handle close (which releases the lock). Gotcha: `flock` does not work on NFS on many systems.\n\n```perl\nuse Fcntl ':flock'\nopen my $fh, '>>', 'shared.log' or die $!\nflock $fh, LOCK_EX or die \"lock: $!\"\nprint $fh tm \"pid $$ wrote this\\n\"\nflock $fh, LOCK_UN\nclose $fh\n\nunless (flock $fh, LOCK_EX | LOCK_NB) {\n    p \"file is locked by another process\"\n}\n```",
        "getc" => "Read a single character from a filehandle (default `STDIN`). Returns `undef` at EOF. The character returned respects the handle's encoding layer — under `:utf8`, `getc` returns a full Unicode character which may be multiple bytes on disk. This function is useful for interactive single-key input, parsing binary formats one byte at a time, or implementing character-level tokenizers. In perlrs, `getc` blocks until a character is available on the handle. For terminal input, note that most terminals are line-buffered by default, so `getc STDIN` won't return until the user presses Enter unless you put the terminal into raw mode.\n\n```perl\nmy $ch = getc STDIN\np \"you pressed: $ch\"\n\nopen my $fh, '<:utf8', $path or die $!\nwhile (defined(my $c = getc $fh)) {\n    p \"char: $c (ord \" . ord($c) . \")\"\n}\n```",
        "select" => "In its one-argument form, `select HANDLE` sets the default output handle for `print`, `say`, and `write`, returning the previously selected handle. This is useful for temporarily redirecting output — for example, sending diagnostics to STDERR while a report goes to a file. In its four-argument form, `select RBITS, WBITS, EBITS, TIMEOUT` performs POSIX `select(2)` I/O multiplexing, waiting for one or more file descriptors to become ready for reading, writing, or to report exceptions. The four-argument form is low-level and rarely used directly in perlrs — prefer `IO::Select` or async I/O patterns for multiplexing. `select` with `$|` is also the classic way to enable autoflush on a handle.\n\n```perl\nmy $old = select STDERR\np \"this goes to stderr\"\nselect $old\n\n# Enable autoflush on STDOUT\nselect STDOUT; $| = 1\nprint \"immediately flushed\"\n```",
        "sysread" => "Low-level unbuffered read directly from a file descriptor, bypassing PerlIO buffering layers. The signature is `sysread FH, SCALAR, LENGTH [, OFFSET]`. Returns the number of bytes read, 0 at EOF, or `undef` on error. Unlike `read`, `sysread` issues a single `read(2)` system call and may return fewer bytes than requested (short read). This is the right choice for sockets, pipes, and non-blocking I/O where you need precise control over how many system calls occur and cannot tolerate buffering. In perlrs, never mix `sysread`/`syswrite` with buffered I/O (`print`, `read`, `<$fh>`) on the same handle — the buffered and unbuffered positions will diverge and produce corrupted reads.\n\n```perl\nopen my $fh, '<:raw', $path or die $!\nmy $buf = ''\nwhile (my $n = sysread $fh, $buf, 4096, length($buf)) {\n    p \"read $n bytes (total: \" . length($buf) . \")\"\n}\nclose $fh\n```",
        "syswrite" => "Low-level unbuffered write directly to a file descriptor, bypassing PerlIO buffering layers. The signature is `syswrite FH, SCALAR [, LENGTH [, OFFSET]]`. Returns the number of bytes actually written, which may be less than requested on non-blocking handles or when writing to pipes/sockets (short write). Returns `undef` on error. Like `sysread`, this issues a single `write(2)` system call and must not be mixed with buffered I/O on the same handle. In perlrs, `syswrite` is essential for socket programming, IPC, and performance-critical binary output where you need to avoid double-buffering. Always check the return value and handle short writes in a loop for robust code.\n\n```perl\nmy $data = \"hello, world\"\nmy $n = syswrite $fh, $data\np \"wrote $n bytes\"\n\n# Robust write loop for sockets\nmy $off = 0\nwhile ($off < length $data) {\n    my $written = syswrite $fh, $data, length($data) - $off, $off\n    die \"syswrite: $!\" unless defined $written\n    $off += $written\n}\n```",
        "sysseek" => "Low-level seek on a file descriptor, bypassing PerlIO buffering. The signature is `sysseek FH, POSITION, WHENCE` with the same WHENCE values as `seek` (0=start, 1=current, 2=end). Returns the new position as a true value, or `undef` on failure. Unlike `seek`, `sysseek` does not flush PerlIO buffers — it operates directly on the underlying OS file descriptor. Use `sysseek` when working with `sysread`/`syswrite` for consistent positioning. In perlrs, `sysseek` with `SEEK_CUR` and position 0 is an idiom for querying the current fd position without moving: `my $pos = sysseek $fh, 0, 1`.\n\n```perl\nsysseek $fh, 0, 0;   # rewind to start\nsysread $fh, my $buf, 512\n\nmy $pos = sysseek $fh, 0, 1\np \"fd position: $pos\"\n```",
        "sysopen" => "Low-level open using POSIX flags for precise control over how a file is opened. The signature is `sysopen FH, FILENAME, FLAGS [, PERMS]`. Flags are bitwise-OR combinations from `Fcntl`: `O_RDONLY`, `O_WRONLY`, `O_RDWR`, `O_CREAT`, `O_EXCL`, `O_TRUNC`, `O_APPEND`, `O_NONBLOCK`, and others. The optional PERMS argument (e.g., `0644`) sets the file mode when `O_CREAT` creates a new file, subject to the process umask. `sysopen` is the right tool when you need `O_EXCL` for atomic file creation (lock files, temp files), `O_NONBLOCK` for non-blocking I/O, or other flags that `open` cannot express. In perlrs, prefer three-argument `open` for routine file access and reserve `sysopen` for cases requiring specific POSIX semantics.\n\n```perl\nuse Fcntl\n# Atomic create — fails if file already exists\nsysopen my $lock, '/tmp/my.lock', O_WRONLY|O_CREAT|O_EXCL, 0644\n    or die \"already running: $!\"\nprint $lock $$\n\nsysopen my $log, 'app.log', O_WRONLY|O_APPEND|O_CREAT, 0644\n    or die \"open log: $!\"\n```",
        "write" => "Output a formatted record to a filehandle using a `format` declaration. `write` looks up the format associated with the current (or specified) filehandle, evaluates the format's picture lines against the current variables, and outputs the result. This is Perl's original report-generation mechanism, predating modules like `Text::Table` and template engines. In perlrs, `write` and `format` are supported for backward compatibility but are rarely used in new code — `printf`/`sprintf` or template strings are more flexible. The format name defaults to the filehandle name (e.g., format `STDOUT` is used by `write STDOUT`).\n\n```perl\nformat STDOUT =\n@<<<<<<<<<< @>>>>>>\n$name,       $score\n.\n\nmy ($name, $score) = (\"alice\", 42)\nwrite;   # outputs: alice           42\n```",
        "format" => "Declare a picture-format template for generating fixed-width text reports. The syntax is `format NAME = ... .` where each line alternates between picture lines (containing field placeholders) and argument lines (listing the variables to fill in). Placeholders include `@<<<<` (left-align), `@>>>>` (right-align), `@||||` (center), `@###.##` (numeric with decimal), and `@*` (multiline block fill). The format ends with a lone `.` on its own line. In perlrs, formats are a legacy feature preserved for compatibility with Perl 5 code — for new reports, prefer `printf`/`sprintf` for simple alignment or a templating module for complex layouts. Formats interact with the special variables `$~` (current format name), `$^` (header format), and `$=` (lines per page).\n\n```perl\nformat REPORT =\n@<<<<<<<<<<<<<<<< @>>>>> @;###.##\n$item,            $qty,  $price\n.\n\nmy ($item, $qty, $price) = (\"Widget\", 100, 9.99)\nmy $old = select REPORT; $~ = 'REPORT'\nwrite REPORT\nselect $old\n```",

        // ── Strings ──
        "chomp" => "`chomp STRING` — remove the trailing record separator (usually `\\n`) from a string in place and return the number of characters removed.\n\n`chomp` is the idiomatic way to strip newlines after reading input; unlike `chop`, it only removes the value of `$/` (the input record separator), so it is safe to call on strings that do not end with a newline — it simply does nothing. You can also `chomp` an entire array to strip every element at once. In perlrs, `chomp` operates on UTF-8 strings and respects multi-byte `$/` values. Prefer `chomp` over `chop` in virtually all input-processing code; `chop` is only for when you truly need to remove an arbitrary trailing character regardless of what it is.\n\n```perl\nmy $line = <STDIN>\nchomp $line\np $line\nchomp(my @lines = <$fh>);  # strip all at once\np scalar @lines\n```",
        "chop" => "`chop STRING` — remove and return the last character of a string, modifying the string in place.\n\nUnlike `chomp`, `chop` unconditionally removes whatever the final character is — newline, letter, digit, or even a multi-byte UTF-8 codepoint in perlrs. The return value is the removed character. This makes `chop` useful for peeling off known trailing delimiters or building parsers that consume input character-by-character, but dangerous for general newline removal because it will silently eat a real character if the string does not end with `\\n`. When called on an array, `chop` removes the last character of every element and returns the last one removed.\n\n```perl\nmy $s = \"hello!\"\nmy $last = chop $s;  # $last = \"!\", $s = \"hello\"\np $s\nmy @words = (\"foo\\n\", \"bar\\n\")\nchop @words;  # strips trailing newline from each\n@words |> e p\n```",
        "chr" => "`chr NUMBER` — return the character represented by the given ASCII or Unicode code point.\n\n`chr` is the inverse of `ord`: `chr(ord($c)) eq $c` always holds. It accepts any non-negative integer and returns a single-character string. In perlrs, values above 127 produce valid UTF-8 characters, so `chr 0x1F600` gives you a smiley emoji with no special encoding gymnastics. This is handy for building binary protocols, generating escape sequences, or constructing Unicode test data. Pass `chr` through a `|>` pipeline with `t` to transform streams of code points into readable text.\n\n```perl\np chr 65;       # A\np chr 0x1F600;  # smiley emoji\nmy @abc = map { chr($_ + 64) } 1..26\n@abc |> join \"\" |> p;  # ABCDEFGHIJKLMNOPQRSTUVWXYZ\n```",
        "hex" => "`hex STRING` — interpret a hexadecimal string and return its numeric value.\n\nThe leading `0x` prefix is optional: both `hex \"ff\"` and `hex \"0xff\"` return 255. The string is case-insensitive, so `hex \"DeAdBeEf\"` works fine. If the string contains non-hex characters, perlrs warns and converts up to the first invalid character. This is the standard way to parse hex-encoded values from config files, color codes, or protocol dumps. For the reverse operation (number to hex string), use `sprintf \"%x\"`. Note that `hex` always returns a number, never a string — arithmetic is immediate.\n\n```perl\nmy $n = hex \"deadbeef\"\nprintf \"0x%x = %d\\n\", $n, $n\np hex \"ff\";   # 255\n\"cafe\" |> t { hex } |> p;   # 51966\n```",
        "oct" => "`oct STRING` — interpret an octal, hexadecimal, or binary string and return its numeric value.\n\n`oct` is the multi-base cousin of `hex`: it auto-detects the base from the prefix. Strings starting with `0b` are binary, `0x` are hex, and bare digits or `0`-prefixed digits are octal. This makes it the go-to for parsing Unix file permissions (`oct \"0755\"` gives 493), binary literals, or any string where the radix is embedded in the value. In perlrs, `oct` handles arbitrarily large integers via the same big-number pathway as other arithmetic. A common gotcha: `oct \"8\"` warns because 8 is not a valid octal digit — use `hex` or a bare numeric literal instead.\n\n```perl\np oct \"0755\";    # 493\np oct \"0b1010\";  # 10\np oct \"0xff\";    # 255\nmy $perms = oct \"644\"\np sprintf \"%04o\", $perms;  # 0644\n```",
        "index" => "`index STRING, SUBSTRING [, POSITION]` — return the zero-based position of the first occurrence of SUBSTRING within STRING, or -1 if not found.\n\nThe optional POSITION argument lets you start the search at a given offset, which is essential for scanning forward through a string in a loop (call `index` repeatedly, advancing POSITION past each hit). In perlrs, `index` operates on UTF-8 character positions, not byte offsets, so it is safe for multi-byte strings. For finding the *last* occurrence, use `rindex` instead. A common pattern is pairing `index` with `substr` to extract fields from fixed-format data without the overhead of a regex or `split`.\n\n```perl\nmy $i = index \"hello world\", \"world\";  # 6\np $i\nmy $s = \"a.b.c.d\"\nmy $pos = 0\nwhile (($pos = index $s, \".\", $pos) != -1) {\n    p \"dot at $pos\"\n    $pos++\n}\n```",
        "rindex" => "`rindex STRING, SUBSTRING [, POSITION]` — return the zero-based position of the last occurrence of SUBSTRING within STRING, searching backward from POSITION (or the end).\n\n`rindex` mirrors `index` but searches from right to left, making it ideal for extracting file extensions, final path components, or the last delimiter in a string. The optional POSITION argument caps how far right the search begins — characters after POSITION are ignored. Returns -1 when the substring is not found. In perlrs, positions are UTF-8 character offsets. Combine with `substr` for efficient right-side extraction without regex.\n\n```perl\nmy $path = \"foo/bar/baz.tar.gz\"\nmy $i = rindex $path, \"/\";     # 7\np substr $path, $i + 1;        # baz.tar.gz\nmy $ext = rindex $path, \".\";   # 15\np substr $path, $ext + 1;      # gz\n```",
        "lc" => "`lc STRING` — return a fully lowercased copy of the string.\n\n`lc` performs Unicode-aware lowercasing in perlrs, so `lc \"\\x{C4}\"` (capital A with diaeresis) correctly returns the lowercase form. It does not modify the original string — it returns a new one. This is the standard way to normalize strings for case-insensitive comparison: `if (lc $a eq lc $b)`. In perlrs `|>` pipelines, wrap `lc` with `t` to apply it as a streaming transform. For lowercasing only the first character, use `lcfirst` instead.\n\n```perl\np lc \"HELLO\";               # hello\n\"SHOUT\" |> t lc |> t rev |> p;  # tuohs\nmy @norm = map lc, @words\n@norm |> e p\n```",
        "lcfirst" => "`lcfirst STRING` — return a copy of the string with only the first character lowercased, leaving the rest unchanged.\n\nThis is useful for converting PascalCase identifiers to camelCase, or for formatting output where only the initial letter matters. In perlrs, `lcfirst` is Unicode-aware, so it handles accented capitals and multi-byte first characters correctly. Like `lc`, it returns a new string rather than modifying in place. If you need the entire string lowercased, use `lc` instead.\n\n```perl\np lcfirst \"Hello\";    # hello\np lcfirst \"XMLParser\"; # xMLParser\nmy @camel = map lcfirst, @PascalNames\n@camel |> e p\n```",
        "uc" => "`uc STRING` — return a fully uppercased copy of the string.\n\n`uc` performs Unicode-aware uppercasing in perlrs, correctly handling multi-byte characters and locale-sensitive transformations. It returns a new string without modifying the original. Use `uc` for normalizing strings before comparison, formatting headers, or transforming pipeline output. In perlrs `|>` chains, combine with `maps` or `t` for streaming uppercase transforms. For uppercasing only the first character (e.g., sentence capitalization), use `ucfirst` instead.\n\n```perl\np uc \"hello\";  # HELLO\n@words |> maps { uc } |> e p\n\"whisper\" |> t uc |> p;   # WHISPER\n```",
        "ucfirst" => "`ucfirst STRING` — return a copy of the string with only the first character uppercased, leaving the rest unchanged.\n\nThis is the standard way to capitalize the first letter of a word for display, title-casing, or converting camelCase to PascalCase. In perlrs, `ucfirst` is Unicode-aware and correctly handles multi-byte leading characters. It returns a new string and does not modify the original. For full uppercasing, use `uc`. A common idiom is `ucfirst lc $word` to normalize a word to \"Title\" form.\n\n```perl\np ucfirst \"hello\";            # Hello\np ucfirst lc \"hELLO\";         # Hello\nmy @titled = map { ucfirst lc } @raw\n@titled |> join \" \" |> p\n```",
        "length" => "`length STRING` — return the number of characters in a string, or the number of elements when given an array.\n\nIn string context, `length` counts Unicode characters, not bytes — so `length \"\\x{1F600}\"` is 1 in perlrs even though the emoji occupies 4 bytes in UTF-8. When passed an array, perlrs returns the element count (equivalent to `scalar @arr`), which diverges slightly from Perl where `length @arr` stringifies the array first. This dual behavior is intentional in perlrs for convenience. To get byte length instead of character length, use `bytes::length` or encode first. Always use `length` rather than comparing against the empty string when checking for non-empty input.\n\n```perl\np length \"hello\";  # 5\nmy @a = (1..10)\np length @a;       # 10\np length \"\\x{1F600}\";  # 1 (single emoji codepoint)\n```",
        "substr" => "`substr STRING, OFFSET [, LENGTH [, REPLACEMENT]]` — extract or replace a substring.\n\n`substr` is perlrs's Swiss-army knife for positional string manipulation. With two arguments it extracts from OFFSET to end; with three it extracts LENGTH characters; with four it replaces that range with REPLACEMENT and returns the original extracted portion. Negative OFFSET counts from the end of the string (`substr $s, -3` gives the last three characters). In perlrs, offsets are UTF-8 character positions, making it safe for multi-byte text. `substr` as an lvalue (`substr($s, 0, 1) = \"X\"`) is also supported for in-place mutation. Prefer `substr` over regex when you know exact positions — it is faster and clearer.\n\n```perl\nmy $s = \"hello world\"\np substr $s, 0, 5;              # hello\np substr $s, -5;                # world\nsubstr $s, 6, 5, \"perlrs\";     # $s = \"hello perlrs\"\np $s\n```",
        "quotemeta" => "`quotemeta STRING` — escape all non-alphanumeric characters with backslashes, returning a string safe for interpolation into a regex.\n\nThis is essential when building dynamic regexes from user input: without `quotemeta`, characters like `.`, `*`, `(`, `)`, `[`, and `\\` would be interpreted as regex metacharacters, leading to either broken patterns or security vulnerabilities (ReDoS). The equivalent inline syntax is `\\Q..\\E` inside a regex. In perlrs, `quotemeta` handles the full Unicode range, escaping any character that is not `[A-Za-z0-9_]`. Use it liberally whenever you splice user-provided strings into patterns.\n\n```perl\nmy $input = \"file (1).txt\"\nmy $safe = quotemeta $input\np $safe;  # file\\ \\(1\\)\\.txt\nmy $found = (\"file (1).txt\" =~ /^$safe$/)\np $found;  # 1\n```",
        "ord" => "`ord STRING` — return the numeric (Unicode code point) value of the first character of the string.\n\n`ord` is the inverse of `chr`: `ord(chr($n)) == $n` always holds. When passed a multi-character string, only the first character is examined — the rest are ignored. In perlrs, `ord` returns the full Unicode code point, not just 0-255, so `ord \"\\x{1F600}\"` returns 128512. This is useful for character classification, building lookup tables, or implementing custom encodings. For ASCII checks, `ord($c) >= 32 && ord($c) <= 126` tests printability.\n\n```perl\np ord \"A\";          # 65\np ord \"\\n\";         # 10\np ord \"\\x{1F600}\";  # 128512\nmy @codes = map ord, split //, \"hello\"\n@codes |> e p;      # 104 101 108 108 111\n```",
        "join" => "`join SEPARATOR, LIST` — concatenate all elements of LIST into a single string, placing SEPARATOR between each pair of adjacent elements.\n\n`join` is the inverse of `split`. It stringifies each element before joining, so mixing numbers and strings is fine. An empty separator (`join \"\", @list`) concatenates without gaps. In perlrs, `join` works naturally with `|>` pipelines: a range or filtered list can be piped directly into `join` with a separator argument. This is the standard way to build CSV lines, path strings, or human-readable lists from arrays. `join` never adds a trailing separator — if you need one, append it yourself.\n\n```perl\nmy $csv = join \",\", @fields\n1..5 |> join \"-\" |> p;   # 1-2-3-4-5\nmy @parts = (\"usr\", \"local\", \"bin\")\np join \"/\", \"\", @parts;   # /usr/local/bin\n```",
        "split" => "`split /PATTERN/, STRING [, LIMIT]` — divide STRING into a list of substrings by splitting on each occurrence of PATTERN.\n\n`split` is one of the most-used string functions. The PATTERN is a regex, so you can split on character classes, alternations, or lookaheads. A LIMIT caps the number of returned fields; the final field contains the unsplit remainder. The special pattern `\" \"` (a single space string, not regex) mimics `awk`-style splitting: it strips leading whitespace and splits on runs of whitespace. In perlrs, `split` integrates with `|>` pipelines, accepting piped-in strings for ergonomic one-liners. Trailing empty fields are removed by default; pass -1 as LIMIT to preserve them.\n\n```perl\nmy @parts = split /,/, \"a,b,c\"\n\"one:two:three\" |> split /:/ |> e p;   # one two three\nmy ($user, $domain) = split /@/, $email, 2\np \"$user at $domain\"\n```",
        "reverse" => "`reverse LIST` — in list context, return the elements in reverse order; in scalar context, reverse the characters of a string.\n\nThe dual nature of `reverse` is context-dependent: `reverse @array` flips element order, while `scalar reverse $string` (or just `reverse $string` in scalar context) reverses character order. In perlrs, the string form is Unicode-aware, correctly reversing multi-byte characters rather than individual bytes. In `|>` pipelines, use the `t rev` shorthand for a concise streaming string reverse. `reverse` does not modify the original — it always returns a new list or string.\n\n```perl\np reverse \"hello\";       # olleh\nmy @r = reverse 1..5;   # (5,4,3,2,1)\n@r |> e p;               # 5 4 3 2 1\n\"abc\" |> t rev |> p;     # cba\n```",
        "study" => "`study STRING` — hint to the regex engine that the given string will be matched against multiple patterns, allowing it to build an internal lookup table for faster matching.\n\nIn classic Perl, `study` builds a linked list of character positions so subsequent regex matches can skip impossible starting points. In practice, modern regex engines (including perlrs's Rust-based engine) already perform these optimizations internally, so `study` is effectively a no-op in perlrs — calling it is harmless but provides no measurable speedup. It exists for compatibility with Perl code that uses it. You only need `study` when porting legacy scripts that call it; do not add it to new perlrs code expecting a performance benefit.\n\n```perl\nstudy $text;   # no-op in perlrs, kept for compatibility\nmy @hits = grep { /$pattern/ } @lines\np scalar @hits\n```",

        // ── Arrays & lists ──
        "push" => "`push @array, LIST` — appends one or more elements to the end of an array and returns the new length. This is the primary way to grow arrays in perlrs and works identically to Perl's builtin. You can push scalars, lists, or even the result of a pipeline. In perlrs, `push` is O(1) amortized thanks to the underlying Rust `Vec`.\n\n```perl\nmy @q\npush @q, 1..3\npush @q, \"four\", \"five\"\np scalar @q;   # 5\n@q |> e p;     # 1 2 3 four five\n```\n\nReturns the new element count, so `my $len = push @arr, $val;` is valid.",
        "pop" => "`pop @array` — removes and returns the last element of an array, or `undef` if the array is empty. This is the complement of `push` and together they give you classic stack (LIFO) semantics. In perlrs the operation is O(1) because the underlying Rust `Vec::pop` simply decrements the length. When called without an argument inside a subroutine, `pop` operates on `@_`; at file scope it operates on `@ARGV`, matching standard Perl behavior.\n\n```perl\nmy @stk = 1..5\nmy $top = pop @stk\np $top;          # 5\np scalar @stk;   # 4\n@stk |> e p;     # 1 2 3 4\n```",
        "shift" => "`shift @array` — removes and returns the first element of an array, sliding all remaining elements down by one index. Like `pop`, it returns `undef` on an empty array. Without an explicit argument it defaults to `@_` inside subroutines and `@ARGV` at file scope. Because every element must be moved, `shift` is O(n); if you only need LIFO access, prefer `push`/`pop`. Use `shift` when processing argument lists or implementing FIFO queues.\n\n```perl\nmy @args = @ARGV\nmy $cmd = shift @args\np $cmd\n@args |> e p;   # remaining arguments\n```",
        "unshift" => "`unshift @array, LIST` — prepends one or more elements to the beginning of an array and returns the new length. It is the counterpart of `push` for the front of the array. Like `shift`, this is O(n) because existing elements must be moved to make room. When you need to build an array in reverse order or maintain a priority queue with newest items first, `unshift` is the idiomatic choice. You can pass multiple values and they will appear in the same order they were given.\n\n```perl\nmy @log = (\"b\", \"c\")\nunshift @log, \"a\"\n@log |> e p;   # a b c\nmy $len = unshift @log, \"x\", \"y\"\np $len;        # 5\n@log |> e p;   # x y a b c\n```",
        "splice" => "`splice @array, OFFSET [, LENGTH [, LIST]]` — the Swiss-army knife for array mutation. It can insert, remove, or replace elements at any position in a single call. With just an offset it removes everything from that point to the end. With offset and length it removes that many elements. With a replacement list it inserts those elements in place of the removed ones. The return value is the list of removed elements, which is useful for saving what you cut. In perlrs this compiles down to Rust's `Vec::splice` and `Vec::drain`.\n\n```perl\nmy @a = 1..5\nmy @removed = splice @a, 1, 2, 8, 9\n@a |> e p;        # 1 8 9 4 5\n@removed |> e p;  # 2 3\nsplice @a, 2;     # remove from index 2 onward\n@a |> e p;        # 1 8\n```",
        "sort" => "`sort [BLOCK] LIST` — returns a new list sorted in ascending order. Without a block, `sort` compares elements as strings (lexicographic). Pass a comparator block using `$_0` and `$_1` (perlrs style) or `$a` and `$b` (classic Perl) to control ordering: use `<=>` for numeric and `cmp` for string comparison. The sort is stable in perlrs, meaning equal elements preserve their original relative order. For descending order, reverse the operands in the comparator. Works naturally in `|>` pipelines.\n\n```perl\nmy @nums = (3, 1, 4, 1, 5)\nmy @asc = sort { $_0 <=> $_1 } @nums\n@asc |> e p;   # 1 1 3 4 5\nmy @desc = sort { $_1 <=> $_0 } @nums\n@desc |> e p;  # 5 4 3 1 1\n@nums |> sort |> e p;   # string sort in pipeline\n```",
        "map" => "`map BLOCK LIST` — evaluates the block for each element of the list, setting `$_` to the current element, and returns a new list of all the block's return values. This is the eager version: it consumes the entire input list and produces the entire output list before returning. Use `map` when you need the full result array or when the input is small. For large or infinite sequences, prefer `maps` (the streaming variant). The block can return zero, one, or multiple values per element, making `map` useful for both transformation and flattening.\n\n```perl\nmy @sq = map { $_ ** 2 } 1..5\n@sq |> e p;   # 1 4 9 16 25\nmy @pairs = map { ($_, $_ * 10) } 1..3\n@pairs |> e p;   # 1 10 2 20 3 30\n```",
        "maps" => "`maps { BLOCK } LIST` — the lazy, streaming counterpart of `map`. Instead of materializing the entire output list, `maps` returns a pull iterator that evaluates the block on demand as downstream consumers request values. This makes it ideal for `|>` pipeline chains, especially when combined with `take`, `greps`, or `collect`. Use `maps` over `map` when working with large ranges, infinite sequences, or when you want to short-circuit processing early with `take`. Memory usage is constant regardless of input size.\n\n```perl\n1..10 |> maps { $_ * 3 } |> take 4 |> e p\n# 3 6 9 12\n1..1_000_000 |> maps { $_ ** 2 } |> greps { $_ > 100 } |> take 3 |> e p\n```",
        "flat_maps" => "`flat_maps { BLOCK } LIST` — a lazy streaming flat-map that evaluates the block for each element and flattens the resulting lists into a single iterator. Where `maps` expects the block to return one value, `flat_maps` handles blocks that return zero or more values per element and concatenates them seamlessly. This is the streaming equivalent of calling `map` with a multi-value block. Use it in `|>` chains when each input element fans out into multiple outputs and you want lazy evaluation.\n\n```perl\n1..3 |> flat_maps { ($_, $_ * 10) } |> e p\n# 1 10 2 20 3 30\n@nested |> flat_maps { @$_ } |> e p;   # flatten array-of-arrays\n```",
        "grep" => "`grep { BLOCK } LIST` — filters the list, returning only elements for which the block evaluates to true. The current element is available as `$_`. This is the eager version: it processes the entire list and returns a new list. It is Perl-compatible and works exactly like Perl's builtin `grep`. For streaming/lazy filtering in `|>` pipelines, use `greps` or `filter` instead. Remember that `grep` in Perl is a list filter, not a text-search tool; for regex matching on files, use the `grep` command-line utility or perlrs's file I/O.\n\n```perl\nmy @evens = grep { $_ % 2 == 0 } 1..10\n@evens |> e p;   # 2 4 6 8 10\nmy @long = grep { length($_) > 3 } @words\n```",
        "greps" => "`greps { BLOCK } LIST` — the lazy, streaming counterpart of `grep`. Returns a pull iterator that only evaluates the predicate as elements are requested downstream. This is the preferred filtering function in `|>` pipelines because it avoids materializing intermediate lists. Combine with `take` to short-circuit early, or with `maps` and `collect` for full lazy pipelines. The block receives `$_` just like `grep`.\n\n```perl\n1..100 |> greps { $_ % 7 == 0 } |> take 3 |> e p\n# 7 14 21\n@lines |> greps { /ERROR/ } |> maps { tm } |> e p\n```",
        "filter" => "`filter { BLOCK } LIST` — perlrs-native lazy filter that returns a pull iterator, functionally identical to `greps` but named for familiarity with Rust/Ruby/JS conventions. Use `filter` or `greps` interchangeably in `|>` chains; both are streaming and both set `$_` in the block. The result must be consumed with `collect`, `e p`, `foreach`, or another terminal. Prefer `filter` when writing perlrs-idiomatic code; prefer `grep`/`greps` when porting from Perl.\n\n```perl\nmy @big = 1..1000 |> filter { $_ > 990 } |> collect\n@big |> e p;   # 991 992 993 994 995 996 997 998 999 1000\n1..50 |> filter { $_ % 2 } |> take 5 |> e p;   # 1 3 5 7 9\n```",
        "compact" | "cpt" => "`compact` (alias `cpt`) — perlrs-native streaming operator that removes `undef` and empty-string values from a list or iterator. This is a common data-cleaning step when dealing with parsed input, optional fields, or split results that produce empty segments. It is equivalent to `greps { defined($_) && $_ ne \"\" }` but more concise and faster because the check is inlined in Rust. Numeric zero and the string `\"0\"` are preserved since they are defined and non-empty.\n\n```perl\nmy @raw = (1, undef, \"\", 2, undef, 3)\n@raw |> compact |> e p;   # 1 2 3\nsplit(/,/, \"a,,b,,,c\") |> cpt |> e p;   # a b c\n```",
        "reject" => "`reject { BLOCK } LIST` — perlrs-native streaming inverse of `filter`/`greps`. It keeps only the elements for which the block returns false. This reads more naturally than `filter { !(...) }` when the condition describes what you want to exclude rather than what you want to keep. Like `filter` and `greps`, it returns a lazy iterator suitable for `|>` chains. The block receives `$_` as the current element.\n\n```perl\n1..10 |> reject { $_ % 3 == 0 } |> e p\n# 1 2 4 5 7 8 10\n@files |> reject { /\\.bak$/ } |> e p;   # skip backups\n```",
        "concat" | "chain" | "cat" => "`concat` (aliases `chain`, `cat`) — perlrs-native streaming operator that concatenates multiple lists or iterators into a single sequential iterator. Pass array references and they will be yielded in order without copying. This is useful for merging data from multiple sources into a unified pipeline. The operation is lazy: each source is drained in turn, so memory usage stays proportional to the largest single element, not the total.\n\n```perl\nmy @a = 1..3; my @b = 7..9\nconcat(\\@a, \\@b) |> e p;   # 1 2 3 7 8 9\nmy @c = (\"x\")\nconcat(\\@a, \\@b, \\@c) |> maps { uc } |> e p\n```",
        "scalar" => "`scalar EXPR` — forces the expression into scalar context. The most common use is `scalar @array` to get the element count instead of the list of elements. In perlrs, scalar context on an array returns its length as a Rust `usize`. You can also use `scalar` on a hash to get the number of key-value pairs, or on a function call to force it to return a single value. This is essential when you want a count inside string interpolation or as a function argument where list context would be ambiguous.\n\n```perl\nmy @items = (\"a\", \"b\", \"c\")\np scalar @items;   # 3\np \"count: \" . scalar @items;   # count: 3\nmy %h = (x => 1, y => 2)\np scalar %h;       # 2\n```",
        "defined" => "`defined EXPR` — returns true if the expression has a value that is not `undef`. This is the canonical way to distinguish between \"no value\" and \"a value that happens to be false\" (such as `0`, `\"\"`, or `\"0\"`). Use `defined` before dereferencing optional values or checking return codes from functions that return `undef` on failure. In perlrs, `defined` compiles to a Rust `Option::is_some()` check internally. Note that `defined` on an aggregate (hash or array) is deprecated in Perl 5 and is a no-op in perlrs.\n\n```perl\nmy $x = undef\np defined($x) ? \"yes\" : \"no\";   # no\n$x = 0\np defined($x) ? \"yes\" : \"no\";   # yes\nmy $val = fn_that_may_fail()\np $val if defined $val\n```",
        "exists" => "`exists EXPR` — tests whether a specific key is present in a hash or an index is present in an array, regardless of whether the associated value is `undef`. This is different from `defined`: a key can exist but hold `undef`. Use `exists` to check hash membership before accessing a value to avoid autovivification side effects. In perlrs, `exists` on a hash compiles to Rust's `HashMap::contains_key`, and on an array it checks bounds. You can also use it with nested structures: `exists $h{a}{b}` only checks the final level.\n\n```perl\nmy %h = (a => 1, b => undef)\np exists $h{b} ? \"yes\" : \"no\";   # yes (key present, value undef)\np exists $h{c} ? \"yes\" : \"no\";   # no\nmy @a = (10, 20, 30)\np exists $a[5] ? \"yes\" : \"no\";   # no\n```",
        "delete" => "`delete EXPR` — removes a key-value pair from a hash or an element from an array and returns the removed value (or `undef` if it did not exist). For hashes this is the only way to truly remove a key; assigning `undef` to `$h{key}` leaves the key in place. For arrays, `delete` sets the element to `undef` but does not shift indices (use `splice` if you need to close the gap). In perlrs, hash deletion maps to Rust's `HashMap::remove`. You can delete multiple keys at once with a hash slice: `delete @h{@keys}`.\n\n```perl\nmy %h = (x => 1, y => 2, z => 3)\nmy $old = delete $h{x}\np $old;   # 1\np exists $h{x} ? \"yes\" : \"no\";   # no\ndelete @h{\"y\", \"z\"};   # delete multiple keys\n```",
        "each" => "`each %hash` — returns the next (key, value) pair from a hash as a two-element list, or an empty list when the iterator is exhausted. Each hash has its own internal iterator, which is reset when you call `keys` or `values` on the same hash. This is memory-efficient for large hashes because it does not build the full key list. Gotcha: do not add or delete keys while iterating with `each`; that can cause skipped or duplicated entries. In perlrs, the iteration order is non-deterministic (Rust `HashMap` order).\n\n```perl\nmy %h = (a => 1, b => 2, c => 3)\nwhile (my ($k, $v) = each %h) { p \"$k=$v\" }\n# output order varies: a=1 b=2 c=3\n```",
        "keys" => "`keys %hash` — returns the list of all keys in a hash in no particular order. When called on an array, it returns the list of valid indices (0 to `$#array`). In scalar context, `keys` returns the number of keys. Calling `keys` resets the `each` iterator on that hash, which is the standard way to restart iteration. In perlrs, this calls Rust's `HashMap::keys()` and collects into a `Vec`. For sorted output, chain with `sort` via the pipe operator.\n\n```perl\nmy %env = (HOME => \"/root\", USER => \"me\")\nkeys(%env) |> sort |> e p;   # HOME USER\np scalar keys %env;           # 2\nmy @a = (10, 20, 30)\nkeys(@a) |> e p;              # 0 1 2\n```",
        "values" => "`values %hash` — returns the list of all values in a hash in no particular order (matching the order of `keys` for the same hash state). When called on an array, it returns the array elements themselves. In scalar context, returns the count of values. Like `keys`, calling `values` resets the `each` iterator. In perlrs this maps to Rust's `HashMap::values()`. Combine with `sum`, `sort`, or pipeline operators for common aggregation patterns.\n\n```perl\nmy %scores = (alice => 90, bob => 85)\np sum(values %scores);   # 175\nvalues(%scores) |> sort { $_1 <=> $_0 } |> e p;   # 90 85\n```",
        "ref" => "`ref EXPR` — returns a string indicating the reference type of the value, or an empty string if it is not a reference. Common return values are `SCALAR`, `ARRAY`, `HASH`, `CODE`, `REF`, and `Regexp`. For blessed objects it returns the class name. Use `ref` to dispatch on data type or validate arguments in polymorphic functions. In perlrs, `ref` inspects the Rust enum variant of the underlying value. Note that `ref` does not recurse; it only tells you the top-level type.\n\n```perl\nmy $r = [1, 2, 3]\np ref($r);       # ARRAY\np ref(\\%ENV);    # HASH\np ref(\\&main);   # CODE\np ref(42);       # (empty string)\n```",
        "undef" => "`undef` — the undefined value, representing the absence of a value. As a function, `undef $var` explicitly undefines a variable, freeing any value it held. `undef` is falsy in boolean context and triggers \"use of uninitialized value\" warnings under `use warnings`. In perlrs, `undef` maps to Rust's `None` in an `Option` type internally. Use `undef` to reset variables, signal missing return values, or clear hash entries without deleting the key.\n\n```perl\nmy $x = 42\nundef $x\np defined($x) ? \"def\" : \"undef\";   # undef\nfn maybe { return undef if !@_; return $_[0] }\np defined(maybe()) ? \"got\" : \"nothing\";   # nothing\n```",
        "wantarray" => "`wantarray()` — returns true if the current subroutine was called in list context, false in scalar context, and `undef` in void context. This lets a function adapt its return value to the caller's expectations. A common pattern is returning a list in list context and a count or reference in scalar context. In perlrs, use `fn` to define subroutines and `wantarray()` inside them just like in Perl. Note that `wantarray` only reflects the immediate call site, not nested contexts.\n\n```perl\nfn ctx { wantarray() ? \"list\" : \"scalar\" }\nmy @r = ctx();  p $r[0];   # list\nmy $r = ctx();  p $r;      # scalar\nfn flexible { wantarray() ? (1, 2, 3) : 3 }\nmy @all = flexible();   p scalar @all;   # 3\nmy $cnt = flexible();   p $cnt;          # 3\n```",
        "caller" => "`caller [LEVEL]` — returns information about the calling subroutine's context. In list context it returns `(package, filename, line)` for the given call-stack level (default 0, the immediate caller). With higher levels you can walk up the call stack for debugging or generating stack traces. In scalar context, `caller` returns just the package name. In perlrs, `caller` works with `fn`-defined subroutines and integrates with the runtime's frame tracking. This is invaluable for building custom error reporters or trace utilities.\n\n```perl\nfn trace { my ($pkg, $f, $ln) = caller(); p \"$f:$ln\" }\ntrace();   # prints current file:line\nfn deep { my ($pkg, $f, $ln) = caller(1); p \"grandparent: $f:$ln\" }\n```",
        "pos" => "`pos SCALAR` — gets or sets the position where the next `m//g` (global match) will start searching in the given string. After a successful `m//g` match, `pos` returns the offset just past the end of the match. You can assign to `pos($s)` to manually reposition the search. If the last `m//g` failed, `pos` resets to `undef`. This is essential for writing lexers or tokenizers that consume a string incrementally with `\\G`-anchored patterns. In perlrs, `pos` tracks per-scalar state just like Perl.\n\n```perl\nmy $s = \"abcabc\"\nwhile ($s =~ /a/g) { p pos($s) }\n# 1 4\npos($s) = 0;   # reset to scan again\np pos($s);     # 0\n```",

        // ── List::Util & friends ──
        "all" => "`all { COND } @list` — returns true (1) if every element in the list satisfies the predicate, false (\"\") otherwise. The block receives each element as `$_` and should return a boolean. Short-circuits on the first failing element, so it never evaluates more than necessary. Works with `|>` pipelines and accepts bare lists or array variables.\n\n```perl\nmy @nums = 2, 4, 6, 8\np all { $_ % 2 == 0 } @nums;   # 1\n1..100 |> all { $_ > 0 } |> p;  # 1\n```",
        "any" => "`any { COND } @list` — returns true (1) if at least one element satisfies the predicate, false (\"\") if none do. The block receives each element as `$_`. Short-circuits on the first match, making it efficient even on large lists. This is the perlrs equivalent of Perl's `List::Util::any` and can be used in `|>` pipelines.\n\n```perl\nmy @vals = 1, 3, 5, 8\np any { $_ > 7 } @vals;   # 1\n1..1000 |> any { $_ == 42 } |> p;  # 1\n```",
        "none" => "`none { COND } @list` — returns true (1) if no element satisfies the predicate, false (\"\") if any element matches. Logically equivalent to `!any { COND } @list` but reads more naturally in guard clauses. Short-circuits on the first match. Useful for validation checks where you want to assert the absence of a condition.\n\n```perl\nmy @words = (\"cat\", \"dog\", \"bird\")\np none { /z/ } @words;   # 1\np \"all non-negative\" if none { $_ < 0 } @vals\n```",
        "first" => "`first { COND } @list` — returns the first element for which the block returns true, or `undef` if no element matches. The alias `fst` is also available. Short-circuits immediately upon finding a match, so only the minimum number of elements are tested. Ideal for searching sorted or unsorted lists when you need a single result.\n\n```perl\nmy $f = first { $_ > 10 } 3, 7, 12, 20\np $f;   # 12\n1..1_000_000 |> first { $_ % 9999 == 0 } |> p;  # 9999\n```",
        "min" => "`min @list` — returns the numerically smallest value from a list. Compares all elements using numeric (`<=>`) comparison, so stringy values are coerced to numbers. Returns `undef` for an empty list. In perlrs, `min` is a built-in that does not require `use List::Util` and works directly in `|>` pipelines.\n\n```perl\np min(5, 3, 9, 1);   # 1\nmy @temps = (72.1, 68.5, 74.3)\n@temps |> min |> p;  # 68.5\n```",
        "max" => "`max @list` — returns the numerically largest value from a list. Compares all elements using numeric (`<=>`) comparison. Returns `undef` for an empty list. Like `min`, this is a perlrs built-in available without imports and works in `|>` pipelines. Combine with `map` to extract max values from complex structures.\n\n```perl\np max(5, 3, 9, 1);   # 9\n1..100 |> map { $_ ** 2 } |> max |> p;  # 10000\n```",
        "sum" | "sum0" => "`sum @list` returns the numeric sum of all elements. Returns `undef` for an empty list. `sum0` is identical except it returns `0` for an empty list, which avoids the need for a fallback `// 0` guard. Both are perlrs built-ins that work in `|>` pipelines. Use `sum0` in contexts where an empty input is expected and you need a safe numeric default.\n\n```perl\np sum(1..100);    # 5050\np sum0();          # 0\n@prices |> map { $_->{amount} } |> sum0 |> p\n```",
        "product" => "`product @list` — returns the product of all numeric elements in the list. Returns `undef` for an empty list. Useful for computing factorials, combinatoric products, and compound multipliers. In perlrs this is a built-in that composes naturally with `|>` pipelines and `range`.\n\n```perl\np product(1..5);   # 120\nrange(1, 10) |> product |> p;  # 3628800\n```",
        "reduce" => "`reduce { $_0 OP $_1 } @list` — performs a sequential left fold over the list. The first two elements are passed as `$_0` and `$_1` to the block; the result becomes `$_0` for the next iteration. The traditional `$a`/`$b` names are also supported for Perl compatibility. Returns `undef` for an empty list and the single element for a one-element list. For a fold with an explicit initial value, see `fold`.\n\n```perl\nmy $fac = reduce { $_0 * $_1 } 1..6\np $fac;   # 720\nmy $longest = reduce { length($_0) > length($_1) ? $_0 : $_1 } @words\n```",
        "fold" => "`fold { $_0 OP $_1 } INIT, @list` — left fold with an explicit initial accumulator value. The initial value is passed as the first `$_0`, and each list element arrives as `$_1`. Unlike `reduce`, `fold` never returns `undef` for an empty list — it returns the initial value instead. Both `$a`/`$b` and `$_0`/`$_1` are supported in the block. Use `fold` when you need a guaranteed starting point for the accumulation.\n\n```perl\nmy $total = fold { $_0 + $_1 } 100, 1..5\np $total;   # 115\nmy $csv = fold { \"$_0,$_1\" } \"\", @fields\n```",
        "reductions" => "`reductions { $_0 OP $_1 } @list` — returns the running (cumulative) results of a left fold, also known as a scan or prefix-sum. Each element of the output is the accumulator state after processing the corresponding input element. The output list has the same length as the input. Both `$_0`/`$_1` and `$a`/`$b` naming conventions are supported.\n\n```perl\nmy @pfx = reductions { $_0 + $_1 } 1..4\n@pfx |> e p;   # 1 3 6 10\n1..5 |> reductions { $_0 * $_1 } |> e p;  # 1 2 6 24 120\n```",
        "mean" => "`mean @list` — returns the arithmetic mean (average) of a numeric list. Computed as `sum / count` in a single pass. Returns `undef` for an empty list. The result is always a floating-point value even if all inputs are integers. Combine with `map` to compute averages over extracted fields.\n\n```perl\np mean(2, 4, 6, 8);   # 5\n@students |> map { $_->{score} } |> mean |> p\n```",
        "median" => "`median @list` — returns the median value of a numeric list. For odd-length lists, this is the middle element after sorting. For even-length lists, it is the arithmetic mean of the two middle elements. The input list does not need to be pre-sorted. Returns `undef` for an empty list.\n\n```perl\np median(1, 3, 5, 7, 9);   # 5\np median(1, 3, 5, 7);      # 4\n1..100 |> median |> p;     # 50.5\n```",
        "mode" => "`mode @list` — returns the most frequently occurring value in the list. If there is a tie, the value that appears first in the list wins. Returns `undef` for an empty list. Works with both numeric and string values — comparison is done by stringification. Useful for finding the dominant category in a dataset.\n\n```perl\np mode(1, 2, 2, 3, 3, 3);   # 3\nmy @logs = qw(INFO WARN INFO ERROR INFO)\np mode(@logs);  # INFO\n```",
        "stddev" | "std" => "`stddev @list` (alias `std`) — returns the population standard deviation of a numeric list. This is the square root of the population variance, measuring how spread out values are from the mean. Uses N (not N-1) in the denominator, so it computes the population statistic rather than the sample statistic. Returns `undef` for an empty list.\n\n```perl\np stddev(2, 4, 4, 4, 5, 5, 7, 9);   # 2\n1..10 |> std |> p\n```",
        "variance" => "`variance @list` — returns the population variance of a numeric list, computed as the mean of the squared deviations from the mean. Like `stddev`, this uses N (not N-1) as the divisor. The variance is `stddev ** 2`. Returns `undef` for an empty list. Useful for statistical analysis and as a building block for higher-order stats.\n\n```perl\np variance(2, 4, 4, 4, 5, 5, 7, 9);   # 4\nmy @samples = map { rand(100) } 1..1000\np variance(@samples)\n```",
        "sample" => "`sample N, @list` — returns a random sample of N elements drawn without replacement from the list. The returned elements are in random order. If N exceeds the list length, the entire list is returned (shuffled). Uses a Fisher-Yates partial shuffle internally for efficiency. Each call produces a different result due to random selection.\n\n```perl\nmy @pick = sample 3, 1..100\n@pick |> e p;   # 3 random values\nmy @test_cases = sample 10, @all_cases\n```",
        "shuffle" => "`shuffle @list` — returns a new list with all elements in random order using a Fisher-Yates shuffle. The original list is not modified. Every permutation is equally likely. In perlrs this is a built-in; no `use List::Util` is needed. The alias `shuf` is also available. Commonly used with `|>` and `take` to draw random subsets.\n\n```perl\nmy @deck = shuffle 1..52\n@deck |> take 5 |> e p;   # 5 random cards\nmy @randomized = shuffle @questions\n```",
        "uniq" => "`uniq @list` — removes duplicate elements from a list, preserving the order of first occurrence. Comparison is done by string equality. The alias `uq` is also available. This is eager (not streaming) and returns a new list. For streaming deduplication in `|>` pipelines, use `distinct`. For type-specific comparison, see `uniqnum`, `uniqstr`, and `uniqint`.\n\n```perl\nmy @u = uniq 1, 2, 2, 3, 1, 3\n@u |> e p;   # 1 2 3\nmy @hosts = uniq @all_hosts\n```",
        "uniqint" => "`uniqint @list` — removes duplicate elements comparing values as integers. Each element is truncated to its integer part before comparison, so `1.1` and `1.9` are considered equal (both become `1`). The first occurrence is kept. This is useful when you have floating-point data but care only about the integer portion for uniqueness.\n\n```perl\nmy @u = uniqint 1, 1.1, 1.9, 2\n@u |> e p;   # 1 2\nmy @distinct_ids = uniqint @raw_ids\n```",
        "uniqnum" => "`uniqnum @list` — removes duplicate elements comparing values as floating-point numbers. Unlike `uniq` (which compares as strings), `uniqnum` treats `1.0` and `1.00` as equal because they have the same numeric value. The first occurrence of each numeric value is preserved. Use this when your data contains numbers that may have different string representations.\n\n```perl\nmy @u = uniqnum 1.0, 1.00, 2.5, 2.50\n@u |> e p;   # 1 2.5\nmy @prices = uniqnum @all_prices\n```",
        "uniqstr" => "`uniqstr @list` — removes duplicate elements comparing values strictly as strings. This is the same comparison as `uniq` but makes the intent explicit. Numeric values `1` and `1.0` are considered different because their string representations differ. Use `uniqstr` when you want to be explicit that string semantics are intended.\n\n```perl\nmy @u = uniqstr \"a\", \"b\", \"a\", \"c\"\n@u |> e p;   # a b c\nmy @tags = uniqstr @all_tags\n```",
        "zip" => "`zip(\\@a, \\@b, ...)` — combines multiple arrays element-wise into a list of arrayrefs. Each output arrayref contains one element from each input array at the corresponding index. Accepts two or more array references. The alias `zp` is also available. By default, `zip` pads shorter arrays with `undef` (equivalent to `zip_longest`). For truncating behavior, use `zip_shortest`.\n\n```perl\nmy @a = 1..3; my @b = (\"a\",\"b\",\"c\")\nzip(\\@a, \\@b) |> e p;   # [1,a] [2,b] [3,c]\nmy @matrix = zip(\\@xs, \\@ys, \\@zs)\n```",
        "zip_longest" => "`zip_longest(\\@a, \\@b, ...)` — combines arrays element-wise, padding shorter arrays with `undef` to match the longest. This is the explicit version of the default `zip` behavior. Every input array contributes exactly one element per output tuple; missing elements become `undef`. Useful when you need to process all data from unequal-length sources.\n\n```perl\nmy @a = 1..3; my @b = (\"x\")\nzip_longest(\\@a, \\@b) |> e p;   # [1,x] [2,undef] [3,undef]\n```",
        "zip_shortest" => "`zip_shortest(\\@a, \\@b, ...)` — combines arrays element-wise, stopping at the shortest input array. No `undef` padding is produced; the output length equals the minimum input length. Use this when you only want complete tuples and extra trailing elements should be discarded.\n\n```perl\nmy @a = 1..5; my @b = (\"x\",\"y\")\nzip_shortest(\\@a, \\@b) |> e p;   # [1,x] [2,y]\nmy @paired = zip_shortest(\\@keys, \\@values)\n```",
        "mesh" => "`mesh(\\@a, \\@b, ...)` — interleaves multiple arrays into a flat list rather than arrayrefs. While `zip` returns `([1,\"a\"], [2,\"b\"])`, `mesh` returns `(1, \"a\", 2, \"b\")`. This makes it ideal for constructing hashes from parallel key and value arrays. The result is a flat list suitable for direct hash assignment.\n\n```perl\nmy @k = (\"a\",\"b\"); my @v = (1,2)\nmy %h = mesh(\\@k, \\@v)\np $h{a};   # 1\nmy %lookup = mesh(\\@ids, \\@names)\n```",
        "mesh_longest" => "`mesh_longest(\\@a, \\@b, ...)` — interleaves arrays into a flat list, padding shorter arrays with `undef` to match the longest. Like `mesh`, the output is flat (not arrayrefs). Missing elements become `undef` in the output sequence. Use this when building a flat interleaved list from arrays of unequal length where you need every element represented.\n\n```perl\nmy @a = 1..3; my @b = (\"x\")\nmy @r = mesh_longest(\\@a, \\@b)\n@r |> e p;   # 1 x 2 undef 3 undef\n```",
        "mesh_shortest" => "`mesh_shortest(\\@a, \\@b, ...)` — interleaves arrays into a flat list, stopping at the shortest input array. No `undef` padding is produced; trailing elements from longer arrays are silently dropped. Use this when you only want complete interleaved groups and partial data should be discarded.\n\n```perl\nmy @a = 1..3; my @b = (\"x\",\"y\")\nmy @r = mesh_shortest(\\@a, \\@b)\n@r |> e p;   # 1 x 2 y\n```",
        "chunked" => "`chunked N, @list` — splits a list into non-overlapping chunks of N elements each. Each chunk is an arrayref. The final chunk may contain fewer than N elements if the list length is not evenly divisible. The alias `chk` is also available. In perlrs, `chunked` is eager and returns a list of arrayrefs; for streaming chunk behavior in pipelines, use `chunk`.\n\n```perl\nmy @ch = chunked 3, 1..7\n@ch |> e p;   # [1,2,3] [4,5,6] [7]\n1..12 |> chunked 4 |> e { p join \",\", @$_ }\n```",
        "windowed" => "`windowed N, @list` — returns a sliding window of N consecutive elements over the list. Each window is an arrayref. The output contains `len - N + 1` windows. Unlike `chunked`, windows overlap: each successive window advances by one element. The alias `win` is also available. Useful for computing moving averages, detecting patterns in sequences, or n-gram extraction.\n\n```perl\nmy @w = windowed 3, 1..5\n@w |> e p;   # [1,2,3] [2,3,4] [3,4,5]\nmy @deltas = windowed(2, @vals) |> map { $_->[1] - $_->[0] }\n```",
        "tail" | "tl" => "`tail N, @list` — returns the last N elements of the list. If N exceeds the list length, the entire list is returned. The alias `tl` is also available. This is the complement of `head`/`take` and mirrors Perl's `List::Util::tail`. In perlrs, it works both as a function call and in `|>` pipelines.\n\n```perl\nmy @t = tail 2, 1..5\n@t |> e p;   # 4 5\n1..100 |> tl 3 |> e p;  # 98 99 100\n```",
        "pairs" => "`pairs @list` — takes a flat list and groups consecutive elements into pairs, returning a list of two-element arrayrefs `([$k, $v], ...)`. The input list must have an even number of elements. Each pair can be accessed via array indexing (`$_->[0]`, `$_->[1]`). This is the inverse of `unpairs` and is commonly used to iterate over hash-like flat lists in a structured way.\n\n```perl\nmy @p = pairs \"a\", 1, \"b\", 2\n@p |> e { p \"$_->[0]=$_->[1]\" };   # a=1 b=2\nmy @entries = pairs %hash\n```",
        "unpairs" => "`unpairs @list_of_pairs` — flattens a list of two-element arrayrefs back into a flat key-value list. This is the inverse of `pairs`. Each arrayref `[$k, $v]` becomes two consecutive elements in the output. Useful for converting structured pair data back into a format suitable for hash assignment or flat list processing.\n\n```perl\nmy @flat = unpairs [\"a\",1], [\"b\",2]\n@flat |> e p;   # a 1 b 2\nmy %h = unpairs @filtered_pairs\n```",
        "pairkeys" => "`pairkeys @list` — extracts the keys (even-indexed elements) from a flat pairlist. Given a list like `(\"a\", 1, \"b\", 2, \"c\", 3)`, returns `(\"a\", \"b\", \"c\")`. This is equivalent to `map { $_->[0] } pairs @list` but more concise and efficient. Useful for extracting just the key side of a key-value flat list without constructing intermediate pair objects.\n\n```perl\nmy @k = pairkeys \"a\", 1, \"b\", 2, \"c\", 3\n@k |> e p;   # a b c\nmy @config_pairs = (host => \"localhost\", port => 8080)\nmy @names = pairkeys @config_pairs\n```",
        "pairvalues" => "`pairvalues @list` — extracts the values (odd-indexed elements) from a flat pairlist. Given `(\"a\", 1, \"b\", 2)`, returns `(1, 2)`. This is equivalent to `map { $_->[1] } pairs @list` but more concise. Pair it with `pairkeys` to split a flat key-value list into separate key and value arrays.\n\n```perl\nmy @v = pairvalues \"a\", 1, \"b\", 2\n@v |> e p;   # 1 2\nmy @defaults = (timeout => 30, retries => 3)\nmy @settings = pairvalues @defaults\n```",
        "pairmap" => "`pairmap BLOCK @list` — maps over consecutive pairs in a flat list, passing the key as `$_0` (or `$a`) and the value as `$_1` (or `$b`). The block can return any number of elements. This is the pair-aware equivalent of `map` and is ideal for transforming hash-like flat lists. The result is a flat list of whatever the block returns.\n\n```perl\nmy @out = pairmap { \"$_0=$_1\" } \"a\", 1, \"b\", 2\n@out |> e p;   # a=1 b=2\nmy @cfg_pairs = (host => \"x\", port => 80)\nmy @upper = pairmap { uc($_0), $_1 } @cfg_pairs\n```",
        "pairgrep" => "`pairgrep { BLOCK } @list` — filters consecutive pairs from a flat list, keeping only those where the block returns true. The key is available as `$_0` (or `$a`) and the value as `$_1` (or `$b`). Returns a flat list of the matching key-value pairs. This is the pair-aware equivalent of `grep` and is useful for filtering hash-like data by both key and value simultaneously.\n\n```perl\nmy @big = pairgrep { $_1 > 5 } \"a\", 3, \"b\", 9, \"c\", 1\n@big |> e p;   # b 9\nmy @alert_pairs = (info => 1, critical_a => 9, critical_b => 7)\nmy @important = pairgrep { $_0 =~ /^critical/ } @alert_pairs\n```",
        "pairfirst" => "`pairfirst { BLOCK } @list` — returns the first pair from a flat list where the block returns true, as a two-element list `($key, $value)`. The key is `$_0` (or `$a`) and the value is `$_1` (or `$b`). Short-circuits on the first match. Returns an empty list if no pair matches. This is the pair-aware equivalent of `first`.\n\n```perl\nmy @hit = pairfirst { $_1 > 5 } \"x\", 2, \"y\", 8\np \"@hit\";   # y 8\nmy @flags = (info => 1, debug => 7, trace => 0)\nmy ($k, $v) = pairfirst { $_0 eq \"debug\" } @flags\n```",

        // ── Functional list ops ──
        "flatten" | "fl" => "`flatten LIST` / `fl LIST` — recursively flatten nested arrayrefs into a single flat list.\n\nFlatten walks every element: scalars pass through unchanged, arrayrefs are opened and their contents are recursively flattened, so arbitrarily deep nesting is handled in one call. In pipeline mode (`|> fl`) it streams element-by-element, so you can chain it with `map`, `grep`, or `take` without materializing the entire intermediate list. The `fl` alias keeps pipeline chains concise.\n\n```perl\nmy @flat = flatten([1,[2,3]],[4]);     # (1,2,3,4)\n[1,[2,[3,4]]] |> fl |> e p;            # 1 2 3 4\n@nested |> fl |> grep { $_ > 0 } |> e p\n```",
        "distinct" => "`distinct LIST` — remove duplicate elements, preserving first-occurrence order (alias for `uniq`).\n\nEach element is compared as a string; the first time a value is seen it is emitted, and all subsequent occurrences are silently dropped. When used in a pipeline the deduplication state is maintained across streamed chunks, so `distinct` works correctly on lazy iterators and generators. This is a perlrs built-in backed by a hash set internally, so it runs in O(n) time regardless of list size.\n\n```perl\nmy @u = distinct(3,1,2,1,3);           # (3,1,2)\n1,2,2,3,3,3 |> distinct |> e p;        # 1 2 3\nstdin |> distinct |> e p;              # unique lines\n```",
        "collect" => "`collect ITERATOR` — materialize a lazy iterator or pipeline into a concrete list.\n\nPipeline stages in perlrs are lazy: chaining `|> map {...} |> grep {...}` builds up a deferred computation without consuming any elements. Calling `collect` forces evaluation and returns all results as a regular Perl list. This is the standard way to terminate a lazy chain when you need the full result in an array. Without `collect`, the iterator is consumed element-by-element by `e`, `take`, or other streaming sinks.\n\n```perl\nmy @out = range(1,5) |> map { $_ * 2 } |> collect\ngen { yield $_ for 1..3 } |> collect |> e p\nmy @data = stdin |> grep /INFO/ |> collect\n```",
        "drop" | "skip" | "drp" => "`drop N, LIST` / `skip N, LIST` / `drp N, LIST` — skip the first N elements and return the rest.\n\nThis operation is fully streaming: when used in a pipeline, the first N elements are consumed and discarded without buffering, and all subsequent elements flow through to the next stage. If the list contains fewer than N elements, the result is empty. The `skip` and `drp` aliases exist for readability in different contexts — all three compile to the same internal op.\n\n```perl\n1..10 |> drop 3 |> e p;                # 4 5 6 7 8 9 10\nmy @rest = drp 2, @data\nstdin |> skip 1 |> e p;                # skip header line\n```",
        "take" | "head" | "hd" => "`take N, LIST` / `head N, LIST` / `hd N, LIST` — return at most the first N elements.\n\nIn streaming mode, `take` pulls exactly N elements from the upstream iterator and then stops, so it short-circuits infinite or very large sources efficiently. This makes it safe to write `range(1, 1_000_000) |> take 5` without allocating a million-element list. If the source has fewer than N elements, all of them are returned. The `head` and `hd` aliases mirror Unix `head` semantics.\n\n```perl\n1..100 |> take 5 |> e p;               # 1 2 3 4 5\nmy @top = hd 3, @sorted\nstdin |> hd 10 |> e p;                 # first 10 lines\n```",
        "drop_while" => "`drop_while { COND } LIST` — skip leading elements while the predicate returns true, then emit everything after.\n\nThe block receives each element as `$_`. Once the predicate returns false for the first time, that element and all remaining elements pass through unconditionally — the predicate is never consulted again. This is streaming: elements are tested one at a time without buffering. Useful for skipping headers, preamble, or sorted prefixes in data streams.\n\n```perl\n1..10 |> drop_while { $_ < 5 } |> e p; # 5 6 7 8 9 10\n@log |> drop_while { /^;#/ } |> e p    # skip comment header\n```",
        "skip_while" => "`skip_while { COND } LIST` — skip leading elements while the predicate is true (alias for `drop_while`).\n\nBehavior is identical to `drop_while`: once the predicate returns false, that element and all subsequent elements are emitted. The `skip_while` name is provided for users coming from Rust or Kotlin where this is the conventional name. Both compile to the same streaming operation internally.\n\n```perl\n1..10 |> skip_while { $_ < 5 } |> e p; # 5 6 7 8 9 10\n@sorted |> skip_while { $_ le \"m\" } |> e p\n```",
        "take_while" => "`take_while { COND } LIST` — emit leading elements while the predicate returns true, then stop.\n\nThe block receives each element as `$_`. Elements are emitted as long as the predicate holds; the moment it returns false, the pipeline terminates immediately without consuming further input. This short-circuit behavior makes `take_while` efficient on infinite iterators and large streams. It is the complement of `drop_while`.\n\n```perl\n1..10 |> take_while { $_ < 5 } |> e p; # 1 2 3 4\nstdin |> take_while { !/^END/ } |> e p\n```",
        "first_or" => "`first_or DEFAULT, LIST` — return the first element of the list, or DEFAULT if the list is empty.\n\nThis is a streaming terminal: it pulls exactly one element from the upstream iterator and returns it, or returns the default value if the iterator is exhausted. It never buffers the entire list. This is especially useful at the end of a `grep` or `map` pipeline where you need a safe fallback instead of `undef` when no match is found.\n\n```perl\nmy $v = first_or 0, @maybe_empty\nmy $x = grep { $_ > 99 } @nums |> first_or -1\nstdin |> grep /^ERROR/ |> first_or \"(none)\" |> p\n```",
        "lines" | "ln" => "`lines STRING` / `ln STRING` — split a string on newline boundaries, returning a streaming iterator of lines.\n\nEach line is yielded without the trailing newline character. When piped from `slurp`, this gives you a lazy line-by-line view of a file without loading all lines into memory at once. Both `\\n` and `\\r\\n` line endings are handled. The `ln` alias keeps pipelines compact.\n\n```perl\nslurp(\"data.txt\") |> lines |> e p\nmy @rows = lines $multiline_str\n$body |> ln |> grep /TODO/ |> e p\n```",
        "chars" | "ch" => "`chars STRING` / `ch STRING` — split a string into individual characters, returning a streaming iterator.\n\nEach Unicode grapheme cluster is yielded as a separate string element. This is useful for character-level processing such as frequency counting, transliteration, or building character n-grams. In pipeline mode the characters stream one at a time, so you can chain with `take`, `grep`, or `with_index` without materializing the full character array.\n\n```perl\n\"hello\" |> chars |> e p;               # h e l l o\nmy @c = chars \"abc\"\n\"emoji: \\x{1F600}\" |> ch |> e p\n```",
        "stdin" => "`stdin` — return a streaming iterator over lines read from standard input.\n\nEach call to the iterator reads one line from STDIN, strips the trailing newline, and yields it. The iterator terminates at EOF. Because it is lazy, combining `stdin` with `take`, `grep`, or `first_or` processes only as many lines as needed — the rest of STDIN is left unconsumed. This is the idiomatic perlrs way to build Unix-style filters.\n\n```perl\nstdin |> grep /error/i |> e p\nstdin |> take 5 |> e p\nstdin |> en |> e { p \"$_->[0]: $_->[1]\" }\n```",
        "trim" | "tm" => "`trim STRING` or `trim LIST` — strip leading and trailing ASCII whitespace.\n\nWhen given a single string, `trim` returns the stripped result. When given a list or used in a pipeline, it operates in streaming mode, trimming each element individually as it flows through. Whitespace includes spaces, tabs, carriage returns, and newlines. The `tm` alias is convenient in pipeline chains where brevity matters.\n\n```perl\n\" hello \" |> tm |> p;                  # \"hello\"\n@raw |> tm |> e p\nslurp(\"data.csv\") |> ln |> tm |> e p\n```",
        "pluck" => "`pluck KEY, LIST_OF_HASHREFS` — extract a single key from each hashref in the list.\n\nFor each element, `pluck` dereferences it as a hashref and returns the value at the given key. Elements where the key is missing yield `undef`. This is a streaming operation: in a pipeline, each hashref is processed and the extracted value is emitted immediately. It is the perlrs equivalent of `map { $_->{KEY} }` but more readable and optimized internally.\n\n```perl\n@users |> pluck \"name\" |> e p\nmy @ids = pluck \"id\", @records\n@rows |> pluck \"email\" |> distinct |> e p\n```",
        "grep_v" => "`grep_v PATTERN, LIST` — inverse grep: reject elements that match the pattern, keep the rest.\n\nThis is the complement of `grep` — any element where the pattern matches is dropped, and non-matching elements pass through. It accepts a regex, a string, or a code block as the pattern. In streaming mode, each element is tested and either forwarded or discarded without buffering. This is a perlrs built-in that avoids the awkward `grep { !/pattern/ }` double-negation.\n\n```perl\n@words |> grep_v /^;#/ |> e p          # drop comments\nmy @clean = grep_v qr/tmp/, @files\nstdin |> grep_v /^\\s*$/ |> e p;        # drop blank lines\n```",
        "with_index" | "wi" => "`with_index LIST` / `wi LIST` — pair each element with its 0-based index as `[$item, $index]`.\n\nEach element is wrapped in a two-element arrayref where `$_->[0]` is the original value and `$_->[1]` is its position. This is useful when you need positional information during a map or grep without maintaining a manual counter. Note the order is `[item, index]`, which differs from `enumerate` which yields `[index, item]`.\n\n```perl\nqw(a b c) |> wi |> e { p \"$_->[1]: $_->[0]\" }\n# 0: a  1: b  2: c\n@data |> wi |> grep { $_->[1] % 2 == 0 } |> e { p $_->[0] }\n```",
        "enumerate" | "en" => "`enumerate ITERATOR` / `en ITERATOR` — yield `[$index, $item]` pairs from a streaming iterator.\n\nEach element is wrapped as `[$index, $item]` where the index starts at 0 and increments for each element. Unlike `with_index` which returns `[item, index]`, `enumerate` uses the Rust/Python convention of `[index, item]`. This is a streaming operation: the index counter is maintained lazily as elements flow through the pipeline.\n\n```perl\nstdin |> en |> e { p \"$_->[0]: $_->[1]\" }\n1..5 |> en |> e { p \"$_->[0]: $_->[1]\" }\n@lines |> en |> grep { $_->[0] < 10 } |> e { p $_->[1] }\n```",
        "chunk" | "chk" => "`chunk N, ITERATOR` / `chk N, ITERATOR` — group elements into N-sized arrayrefs as they stream through.\n\nElements are buffered until N are collected, then the group is emitted as a single arrayref. The final chunk may contain fewer than N elements if the source is not evenly divisible. This is fully streaming: only one chunk is held in memory at a time, making it safe for large or infinite iterators. Use `chunk` for batching work (e.g., bulk database inserts) or formatting output into rows.\n\n```perl\n1..9 |> chk 3 |> e { p join \",\", @$_ }\n# 1,2,3  4,5,6  7,8,9\nstdin |> chk 100 |> e { bulk_insert(@$_) }\n```",
        "dedup" | "dup" => "`dedup ITERATOR` / `dup ITERATOR` — drop consecutive duplicate elements from a stream.\n\nOnly adjacent duplicates are removed: if the same value appears later after an intervening different value, it is emitted again. Comparison is string-based by default. This is a streaming operation that holds only the previous element in memory, so it works on infinite iterators. For global deduplication across the entire stream, use `distinct` instead.\n\n```perl\n1,1,2,2,3,1,1 |> dedup |> e p;        # 1 2 3 1\n@sorted |> dedup |> e p;              # like uniq(1)\nstdin |> dedup |> e p;                 # collapse repeated lines\n```",
        "range" => "`range(START, END [, STEP])` — create a lazy integer iterator from START to END with an optional step.\n\nThe range is inclusive on both ends. When START > END and no step is given, perlrs automatically counts downward. An explicit STEP controls the increment and must be negative when counting down, or the range will be empty. The iterator is fully lazy: no list is allocated, and elements are generated on demand, making `range(1, 1_000_000)` as cheap to create as `range(1, 5)`. Combine with `|>` to feed into streaming pipelines.\n\n```perl\nrange(1, 5) |> e p;                    # 1 2 3 4 5\nrange(5, 1) |> e p;                    # 5 4 3 2 1\nrange(0, 10, 2) |> e p;                # 0 2 4 6 8 10\nrange(10, 0, -2) |> e p;               # 10 8 6 4 2 0\n```",
        "tap" => "`tap { BLOCK } LIST` — execute a side-effecting block for each element, then pass the element through unchanged.\n\nThe return value of the block is ignored; the original element is always forwarded to the next pipeline stage. This makes `tap` ideal for injecting logging, debugging, or metrics collection into the middle of a pipeline without altering the data flow. It is fully streaming and preserves element order.\n\n```perl\n1..5 |> tap { log_debug \"saw: $_\" } |> map { $_ * 2 } |> e p\n@files |> tap { p \"processing: $_\" } |> map slurp |> e p\n```",
        "tee" => "`tee FILE, ITERATOR` — write each element to a file as a side effect while passing it through the pipeline.\n\nEvery element that flows through `tee` is appended as a line to the specified file, and the element itself continues downstream unchanged. The file is opened once on first element and closed when the iterator is exhausted. This is the perlrs equivalent of the Unix `tee` command, useful for auditing or logging intermediate pipeline results to disk.\n\n```perl\n1..10 |> tee \"/tmp/log.txt\" |> map { $_ * 2 } |> e p\nstdin |> tee \"/tmp/raw.log\" |> grep /ERROR/ |> e p\n```",
        "nth" => "`nth N, LIST` — return the Nth element using 0-based indexing.\n\nWhen used in a pipeline, `nth` consumes and discards the first N elements, returns the next one, and stops — so it short-circuits on infinite iterators. On a plain list, it is equivalent to `$list[N]` but works as a function call for pipeline composition. Returns `undef` if the list has fewer than N+1 elements.\n\n```perl\nmy $third = nth 2, @data\n1..10 |> nth 4 |> p;                   # 5\nstdin |> nth 0 |> p;                   # first line\n```",
        "to_set" => "`to_set LIST` — collect a list or iterator into a `set` object with O(1) membership testing.\n\nThe resulting set contains only unique elements (duplicates are discarded). This is a terminal operation that materializes the full stream. The returned set supports `->contains($val)`, `->union($other)`, `->intersection($other)`, and `->difference($other)`. Use this when you need fast repeated lookups against a collection of values.\n\n```perl\nmy $s = 1..5 |> to_set\n@words |> to_set;                      # deduplicated set\nmy $allowed = to_set @whitelist\np $allowed->contains(\"foo\")\n```",
        "to_hash" => "`to_hash LIST` — collect a flat list of key-value pairs into a Perl hash.\n\nThe list is consumed two elements at a time: odd-positioned elements become keys and even-positioned elements become values. If there is an odd number of elements, the last key maps to `undef`. This is a terminal operation that materializes the full stream. Useful for converting pipeline output into a lookup table.\n\n```perl\nmy %h = qw(a 1 b 2) |> to_hash\n@pairs |> to_hash\nmy %freq = @words |> map { $_, 1 } |> to_hash\n```",
        "set" => "`set LIST` — create a set (unique unordered collection) from the given elements.\n\nDuplicate values are collapsed on construction so the set contains each value exactly once. The set object provides `->contains($val)` for O(1) membership testing, plus `->union($s)`, `->intersection($s)`, `->difference($s)`, and `->len` methods. Internally backed by a Rust `HashSet` for performance. Use `to_set` to convert an existing iterator into a set.\n\n```perl\nmy $s = set(1, 2, 3, 2, 1)\np $s->contains(2);                     # 1\np $s->len;                             # 3\nmy $both = $s->union(set(3, 4, 5))\n```",
        "deque" => "`deque LIST` — create a double-ended queue initialized with the given elements.\n\nA deque supports efficient O(1) insertion and removal at both ends via `->push_front($val)`, `->push_back($val)`, `->pop_front`, and `->pop_back`. It also supports `->len` and iteration. Internally backed by a Rust `VecDeque`, it is ideal for sliding window algorithms, BFS traversals, or any scenario where you need fast access to both ends of a sequence.\n\n```perl\nmy $dq = deque(1, 2, 3)\n$dq->push_front(0)\n$dq->push_back(4)\np $dq->pop_front;                      # 0\n```",
        "heap" => "`heap LIST` — create a min-heap (priority queue) from the given elements.\n\nElements are heapified on construction so that `->pop` always returns the smallest element in O(log n) time. `->push($val)` inserts a new element, also in O(log n). The heap supports `->peek` to inspect the minimum without removing it, and `->len` for the current size. Internally backed by a Rust `BinaryHeap` (inverted for min-heap semantics), it is the go-to structure for top-K queries, Dijkstra, and merge-K-sorted-lists problems.\n\n```perl\nmy $h = heap(5, 3, 8, 1)\np $h->pop;                             # 1 (smallest first)\np $h->peek;                            # 3 (next smallest)\n$h->push(0)\n```",
        "peek" => "`peek ITERATOR` — inspect the next element of an iterator without consuming it.\n\nThe peeked value is buffered internally so that the next call to `->next` or pipeline pull returns the same element. This is useful for lookahead parsing, conditional branching on the next value, or implementing `take_while`-style logic manually. Calling `peek` multiple times without advancing the iterator returns the same value each time. Works with any perlrs iterator including `gen`, `range`, `stdin`, and pipeline results.\n\n```perl\nmy $g = gen { yield $_ for 1..5 }\np peek $g;                             # 1 (not consumed)\np $g->next;                            # 1\np peek $g;                             # 2\n```",

        // ── Parallel extensions (perlrs) ──
        "pmap" => "Parallel `map` powered by rayon's work-stealing thread pool. Every element of the input list is processed concurrently across all available CPU cores, and the output order is guaranteed to match the input order. This is the primary workhorse for CPU-bound transforms in perlrs — use it whenever you have a pure function and a large list. Pass `progress => 1` to get a live progress bar on STDERR for long-running jobs.\n\nTwo equivalent surface syntaxes:\n  • Block form — `pmap BLOCK LIST` — element bound to `$_`\n  • Bare-fn form — `pmap FUNC, LIST` — single-arg function name as first argument\n\n```perl\n# Block form\nmy @out = pmap { $_ * 2 } 1..1_000_000\nmy @hashes = pmap sha256 @blobs, progress => 1\n1..100 |> pmap { fetch(\"https://api.example.com/item/$_\") } |> e p\n\n# Bare-fn form (works for builtins and user-defined subs)\nmy @hashes = pmap sha256, @blobs, progress => 1\nsub double { $_0 * 2 }\nmy @r = pmap double, (1..1_000_000)\n```",
        "pmap_chunked" => "Parallel map that groups input into contiguous batches of N elements before distributing to threads. This reduces per-item scheduling overhead when the per-element work is very cheap (e.g. a few arithmetic ops). Each thread receives a slice of N consecutive items, processes them sequentially within the batch, then returns the batch result. Use this instead of `pmap` when profiling shows rayon overhead dominates the actual computation.\n\n```perl\nmy @out = pmap_chunked 100, { $_ ** 2 } 1..1_000_000\nmy @parsed = pmap_chunked 50, { json_decode } @json_strings\n```",
        "pgrep" => "Parallel `grep` that evaluates the filter predicate concurrently across all CPU cores using rayon. The result preserves the original input order, so it is a drop-in replacement for `grep` on large lists. Best suited for predicates that do meaningful work per element — if the predicate is trivial (e.g. a single regex on short strings), sequential `grep` may be faster due to lower scheduling overhead.\n\nTwo equivalent surface syntaxes: `pgrep { BLOCK } LIST` or `pgrep FUNC, LIST`.\n\n```perl\n# Block form\nmy @matches = pgrep { /complex_pattern/ } @big_list\nmy @primes = pgrep { is_prime } 2..1_000_000\n@files |> pgrep { -s $_ > 1024 } |> e p\n\n# Bare-fn form\nsub even { $_0 % 2 == 0 }\nmy @e = pgrep even, 1..10;        # (2,4,6,8,10)\n```",
        "pfor" => "Parallel `foreach` that executes a side-effecting block across all CPU cores with no return value. Use this when you need to perform work for each element (writing files, sending requests, updating shared state) but don't need to collect results. The block receives each element as `$_`. Iteration order is non-deterministic, so the block must be safe to run concurrently.\n\nTwo equivalent surface syntaxes: `pfor { BLOCK } LIST` or `pfor FUNC, LIST`.\n\n```perl\n# Block form\npfor { write_report } @records\npfor { compress_file } glob(\"*.log\")\n@urls |> pfor { fetch; p \"done: $_\" }\n\n# Bare-fn form\nsub work { print \"did $_0\\n\" }\npfor work, (1, 2, 3)\n```",
        "psort" => "Parallel sort that uses rayon's parallel merge-sort algorithm. Accepts an optional comparator block using `$_0`/`$_1` (or `$a`/`$b`). For large lists (10k+ elements), this significantly outperforms the sequential `sort` by splitting the array, sorting partitions in parallel, and merging. The sort is stable — equal elements retain their relative order.\n\n```perl\nmy @sorted = psort { $_0 <=> $_1 } @big_list\nmy @by_name = psort { $_0->{name} cmp $_1->{name} } @records\n@nums |> psort { $a <=> $b } |> e p\n```",
        "pcache" => "Parallel memoized map — each element is processed concurrently, but results are cached by the stringified value of `$_` so duplicate inputs are computed only once. This is ideal when your input list contains many repeated values and the computation is expensive. The cache is a concurrent hash map shared across all threads, so there is no lock contention on reads after the first computation.\n\nTwo equivalent surface syntaxes: `pcache { BLOCK } LIST` or `pcache FUNC, LIST`.\n\n```perl\n# Block form\nmy @out = pcache { expensive_lookup } @list_with_dupes\nmy @resolved = pcache { dns_resolve } @hostnames\n\n# Bare-fn form\nmy @resolved = pcache dns_resolve, @hostnames\n```",
        "preduce" => "Parallel tree-fold using rayon's `reduce` — splits the list into chunks, reduces each chunk independently, then merges partial results. The combining operation **must be associative** (e.g. `+`, `*`, `max`); non-associative ops will produce incorrect results. Much faster than sequential `reduce` on large numeric lists because the tree structure allows O(log n) merge depth across cores.\n\n```perl\nmy $total = preduce { $_0 + $_1 } @nums\nmy $biggest = preduce { $_0 > $_1 ? $_0 : $_1 } @vals\nmy $product = preduce { $a * $b } 1..100\n```",
        "preduce_init" => "Parallel fold with identity value.\n\n```perl\nmy $total = preduce_init 0, { $_0 + $_1 } @list\n```",
        "pmap_reduce" => "Fused parallel map + tree reduce.\n\n```perl\nmy $sum = pmap_reduce { $_*2 } { $_0 + $_1 } @list\n```",
        "pany" => "`pany { COND } @list` — parallel short-circuit `any`.",
        "pfirst" => "`pfirst { COND } @list` — parallel first matching element.",
        "puniq" => "`puniq @list` — parallel unique elements.",
        "pselect" => "`pselect(@channels)` — wait on multiple `pchannel` receivers.",
        "pflat_map" => "Parallel flat-map: map + flatten results. Each element produces zero or more output values via the block/function, and the outputs are concatenated in input order.\n\nTwo equivalent surface syntaxes: `pflat_map BLOCK LIST` or `pflat_map FUNC, LIST`.\n\n```perl\n# Block form\nmy @out = pflat_map expand @list\n\n# Bare-fn form\nsub expand { ($_0, $_0 * 10) }\nmy @r = pflat_map expand, (1, 2, 3);   # (1, 10, 2, 20, 3, 30)\n```",
        "pflat_map_on" => "Distributed parallel flat-map over `cluster`.",
        "fan" => "Execute BLOCK or FUNC N times in parallel (`$_`/`$_0` = index 0..N-1). With no count, defaults to the rayon pool size (`pe -j`).\n\nTwo equivalent surface syntaxes:\n  • Block form — `fan N { BLOCK }` or `fan { BLOCK }`\n  • Bare-fn form — `fan N, FUNC` or `fan FUNC`\n\n```perl\n# Block form\nfan 8 { work($_) }\nfan { work($_) } progress => 1\n\n# Bare-fn form\nsub work { print \"tick $_0\\n\" }\nfan 8, work\nfan work, progress => 1;   # uses pool size\n```",
        "fan_cap" => "Like `fan` but captures return values in index order. Two surface syntaxes: `fan_cap N { BLOCK }` or `fan_cap N, FUNC`.\n\n```perl\n# Block form\nmy @results = fan_cap 8 { compute($_) }\n\n# Bare-fn form\nsub compute { $_0 * $_0 }\nmy @squares = fan_cap 8, compute\n```",

        // ── Cluster / distributed ──
        "cluster" => "`cluster([\"host1:N\", \"host2:M\", ...])` — build an SSH-backed worker pool for distributing perlrs workloads across multiple machines.\n\nEach entry in the list is a hostname (or `user@host`) followed by a colon and the number of worker slots to allocate on that host. Under the hood, perlrs opens persistent SSH multiplexed connections to each host, deploys lightweight `pe --remote-worker` processes, and manages a work-stealing scheduler across the entire cluster. The cluster object is then passed to distributed primitives like `pmap_on` and `pflat_map_on`. Workers must have `pe` installed and accessible on `$PATH`. If a host becomes unreachable mid-computation, its in-flight tasks are automatically re-queued to surviving hosts.\n\n```perl\nmy $cl = cluster([\"server1:8\", \"server2:4\", \"gpu-box:16\"])\nmy @results = pmap_on $cl, { heavy_compute } @jobs\n\n# Single-machine cluster for testing:\nmy $local = cluster([\"localhost:4\"])\n```",
        "pmap_on" => "`pmap_on $cluster, { BLOCK } @list` — distributed parallel map that fans work across a `cluster` of remote machines.\n\nElements from `@list` are serialized, shipped to remote `pe --remote-worker` processes over SSH, executed in parallel across every worker slot in the cluster, and the results are gathered back in input order. This is the distributed equivalent of `pmap`: same interface, same order guarantee, but the thread pool spans multiple hosts instead of local cores. Use this when a single machine's CPU count is the bottleneck. The block must be self-contained — it cannot close over local file handles or database connections, since it executes in a separate process on a remote host. Large closures are serialized once and cached on each worker for the lifetime of the cluster.\n\n```perl\nmy $cl = cluster([\"host1:8\", \"host2:8\"])\nmy @hashes = pmap_on $cl, { sha256(slurp) } @file_paths\nmy @results = pmap_on $cl, { fetch(\"https://api.example.com/$_\") } 1..10_000\n```",
        "ssh" => "`ssh($host, $command)` — execute a shell command on a remote host via SSH and return its stdout as a string.\n\nThis is a simple synchronous wrapper around an SSH invocation. The command is run in the remote user's default shell, and stdout is captured and returned. If the remote command exits non-zero, perlrs dies with the stderr output. For bulk remote work, prefer `cluster` + `pmap_on` which manages connection pooling and parallelism automatically. `ssh` is best for one-off administrative commands, health checks, or bootstrapping a remote environment before building a cluster.\n\n```perl\nmy $uptime = ssh(\"server1\", \"uptime\")\np ssh(\"deploy@prod\", \"cat /etc/hostname\")\nmy $free = ssh(\"gpu-box\", \"nvidia-smi --query-gpu=memory.free --format=csv,noheader\")\n```",

        // ── Async / concurrency ──
        "async" => "`async { BLOCK }` — schedule a block for execution on a background worker thread and return a task handle immediately.\n\nThe block begins executing as soon as a thread is available in perlrs's global rayon thread pool, while the calling code continues without blocking. To retrieve the result, pass the task handle to `await`, which blocks until the task completes and returns its value. If the block panics, the panic is captured and re-raised at the `await` call site. Use `async` for fire-and-forget background work, overlapping I/O with computation, or launching multiple independent tasks that you later join. For structured fan-out with index-based parallelism, prefer `fan` or `fan_cap` instead.\n\n```perl\nmy $task = async { long_compute() }\ndo_other_work()\nmy $val = await $task\n\n# Overlapping multiple fetches:\nmy @tasks = map { async { fetch(\"https://api.example.com/$_\") } } 1..10\nmy @results = map await @tasks\n```",
        "spawn" => "`spawn { BLOCK }` — Rust-style alias for `async`; schedules a block on a background thread and returns a joinable task handle.\n\nThis is identical to `async` in every respect — same thread pool, same semantics, same `await` for joining. The name exists for developers coming from Rust's `tokio::spawn` or `std::thread::spawn` who find `spawn` more natural. Use whichever reads better in your code; mixing `async` and `spawn` in the same program is perfectly fine since they share the same underlying pool.\n\n```perl\nmy $task = spawn { expensive_io() }\nmy $val = await $task\n\nmy @handles = map { spawn { process } } @items\nmy @out = map await @handles\n```",
        "await" => "`await $task` — block the current thread until an async/spawn task completes and return its result value.\n\nIf the background task has already finished by the time `await` is called, the result is returned immediately with no scheduling overhead. If the task panicked, `await` re-raises the panic as a die in the calling thread, preserving the original error message and backtrace. You can `await` a task exactly once; calling `await` on an already-joined handle is a fatal error. For waiting on multiple tasks, simply map over the handles — perlrs does not yet provide a `join_all` primitive, but `map await @tasks` achieves the same effect.\n\n```perl\nmy $task = async { 42 }\nmy $result = await $task;              # 42\n\n# Await with error handling:\nmy $t = spawn { die \"oops\" }\neval { await $t };                     # $@ eq \"oops\"\n```",
        "pchannel" => "`pchannel(N)` — create a bounded multi-producer multi-consumer (MPMC) channel with capacity N, returning a `($tx, $rx)` pair.\n\nThe transmitter `$tx` supports `->send($val)` which blocks if the channel is full (backpressure). The receiver `$rx` supports `->recv` which blocks until a value is available, and `->try_recv` which returns `undef` immediately if the channel is empty. Both ends can be cloned and shared across threads — clone `$tx` with `$tx->clone` to create additional producers, or clone `$rx` for additional consumers. When all transmitters are dropped, `->recv` on the receiver returns `undef` to signal completion. Use `pchannel` to build producer-consumer pipelines, rate-limited work queues, or to communicate between `async`/`spawn` tasks.\n\n```perl\nmy ($tx, $rx) = pchannel(100)\nasync { $tx->send($_) for 1..1000; undef $tx; }\nwhile (defined(my $val = $rx->recv)) {\n    p $val\n}\n\n# Multiple producers:\nmy ($tx, $rx) = pchannel(50)\nfor my $i (1..4) {\n    my $t = $tx->clone\n    spawn { $t->send(\"from worker $i: $_\") for 1..100; }\n}\nundef $tx;  # drop original so channel closes when workers finish\n```",
        "barrier" => "`barrier(N)` — create a synchronization barrier that blocks until exactly N threads have arrived at the wait point.\n\nEach thread calls `$b->wait` and is suspended until all N participants have reached the barrier, at which point all are released simultaneously. This is useful for coordinating phased parallel algorithms where all workers must complete step K before any worker begins step K+1. The barrier is reusable — after all threads are released, it resets and can be waited on again. Internally backed by a Rust `std::sync::Barrier` for zero-overhead synchronization.\n\n```perl\nmy $b = barrier(4)\nfor my $i (0..3) {\n    spawn {\n        setup_phase($i)\n        $b->wait;                      # all 4 threads sync here\n        compute_phase($i)\n    }\n}\n```",
        "ppool" => "`ppool(N, sub { ... })` — create a persistent thread pool of N worker threads, each running the provided subroutine in a loop.\n\nThe pool is typically paired with a `pchannel`: workers pull items from the channel's receiver, process them, and optionally send results to an output channel. Unlike `pmap` which is a one-shot parallel transform, `ppool` keeps threads alive for the lifetime of the pool, making it ideal for long-running server-style workloads, background drain loops, or scenarios where thread startup cost would dominate short-lived `pmap` calls. Workers exit when their input channel is closed (all transmitters dropped). The pool object supports `->join` to block until all workers have finished.\n\n```perl\nmy ($tx, $rx) = pchannel(100)\nmy $pool = ppool 4, sub {\n    while (defined(my $job = $rx->recv)) {\n        process($job)\n    }\n}\n$tx->send($_) for @work_items\nundef $tx;                             # signal completion\n$pool->join;                           # wait for drain\n```",
        "pwatch" => "`pwatch(PATH, sub { ... })` — watch a file or directory for filesystem changes and invoke the callback on each event.\n\nThe watcher runs on a background thread using OS-native notifications (FSEvents on macOS, inotify on Linux) so it consumes near-zero CPU while idle. The callback receives the event type and affected path in `$_`. Directory watches are recursive by default. The watcher continues until the returned handle is dropped or the program exits. This is useful for building live-reload dev servers, file-triggered pipelines, or audit logs. Combine with `debounce` or a `pchannel` if the callback is expensive and rapid bursts of events need to be coalesced.\n\n```perl\nmy $w = pwatch \"./src\", sub {\n    p \"changed: $_\"\n    rebuild()\n}\n\n# Watch multiple paths:\nmy $w1 = pwatch \"/var/log/app.log\", sub { p \"log updated\" }\nmy $w2 = pwatch \"./config\", sub { reload_config() }\nsleep;                                 # block forever\n```",

        // ── Pipeline / lazy iterators ──
        "pipeline" => "`pipeline(@list)` — wrap a list (or iterator) in a lazy pipeline object supporting chained `->map`, `->filter`, `->take`, `->skip`, `->flat_map`, `->tap`, and other transforms that execute zero work until a terminal method is called.\n\nNo intermediate lists are allocated between stages — each element flows through the full chain one at a time, making pipelines memory-efficient even on very large or infinite inputs. Terminal methods include `->collect` (materialize to list), `->reduce { ... }` (fold), `->for_each { ... }` (side-effect iteration), and `->count`. Pipelines compose naturally with perlrs's `|>` operator: you can feed `pipeline(...)` output into further `|>` stages or vice versa. Use `pipeline` when you want explicit method-chaining style rather than the flat `|>` pipe syntax — both compile to the same lazy evaluation engine.\n\n```perl\nmy @out = pipeline(@data)\n  ->filter { $_ > 0 }\n  ->map { $_ * 2 }\n  ->take(10)\n  ->collect\n\npipeline(1..1_000_000)\n  ->filter { $_ % 3 == 0 }\n  ->map { $_ ** 2 }\n  ->take(5)\n  ->for_each { p $_ };                # 9 36 81 144 225\n\nmy $sum = pipeline(@scores)\n  ->filter { $_ >= 60 }\n  ->reduce { $_0 + $_1 }\n```",
        "par_pipeline" => "`par_pipeline(source => \\@data, stages => [...], workers => N)` — build a multi-stage parallel pipeline where each stage's map/filter block runs concurrently across N worker threads.\n\nUnlike `pmap` which parallelizes a single transform, `par_pipeline` lets you define a sequence of named stages — each with its own block — that execute in parallel while preserving input order in the final output. Internally, perlrs partitions the source into chunks, distributes them across workers, and pipelines the stages so that stage 2 can begin processing a chunk as soon as stage 1 finishes it, overlapping computation across stages. This is ideal for multi-step ETL workloads where each step is CPU-bound and the data volume is large. The `workers` parameter defaults to the number of logical CPUs.\n\n```perl\nmy @results = par_pipeline(\n    source  => \\@raw_records,\n    stages  => [\n        { name => \"parse\",     map => sub { json_decode } },\n        { name => \"transform\", map => sub { enrich } },\n        { name => \"validate\",  filter => sub { $_->{valid} } },\n    ],\n    workers => 8,\n)\n```",
        "par_pipeline_stream" => "`par_pipeline_stream(source => ..., stages => [...], workers => N)` — streaming variant of `par_pipeline` that connects stages via bounded `pchannel` queues instead of materializing intermediate arrays.\n\nEach stage runs as an independent pool of workers, pulling from an input channel and pushing to an output channel. This gives true pipelined parallelism: stage 1 workers produce items while stage 2 workers consume them concurrently, bounded by channel capacity to prevent memory blowup. The streaming design makes this suitable for infinite or very large data sources (file streams, network feeds) where materializing the full dataset between stages is impractical. Results are emitted in arrival order by default; pass `ordered => 1` to reorder them to match input order at the cost of buffering.\n\n```perl\npar_pipeline_stream(\n    source  => sub { while (my $line = <STDIN>) { yield $line } },\n    stages  => [\n        { name => \"parse\", map => sub { json_decode } },\n        { name => \"score\", map => sub { compute_score } },\n    ],\n    workers => 4,\n    on_item => sub { p $_ },           # process results as they arrive\n)\n```",

        // ── Parallel I/O ──
        "par_lines" => "`par_lines PATH, { code }` — memory-map a file and scan its lines in parallel across all available CPU cores.\n\nThe file is `mmap`'d into memory rather than read sequentially, and line boundaries are detected in parallel chunks. Each line is passed to the callback as `$_`. Because the file is memory-mapped, there is no read-buffer overhead and the OS kernel pages data in on demand, making `par_lines` extremely efficient for multi-gigabyte log files or CSV data. Line order within the callback is not guaranteed (lines run in parallel), so the callback should be a self-contained side-effecting operation (accumulate into a shared structure via `pchannel`, write to a file, etc.) or use `par_lines` with a reducer. For ordered processing, use `read_lines` with `pmap` instead.\n\n```perl\npar_lines \"data.txt\", sub { process }\n\n# Count matching lines in a large log:\nmy $count = 0\npar_lines \"/var/log/syslog\", sub { $count++ if /ERROR/ }\np $count\n\n# Feed lines into a channel for downstream processing:\nmy ($tx, $rx) = pchannel(1000)\nasync { par_lines \"huge.csv\", sub { $tx->send($_) }; undef $tx; }\n```",
        "par_walk" => "`par_walk PATH, { code }` — recursively walk a directory tree in parallel, invoking the callback for every file path found.\n\nDirectory traversal is parallelized using a work-stealing thread pool: multiple directories are read concurrently, and the callback fires as each file is discovered. The path is passed as `$_` (absolute). This is significantly faster than a sequential `find`-style walk on SSDs and networked filesystems where directory `readdir` latency dominates. Symlinks are not followed by default to avoid cycles. The walk visits files only — directories themselves are not passed to the callback unless you pass `dirs => 1`. Combine with `pmap` for a two-phase pattern: first collect paths with `par_walk`, then process file contents in parallel.\n\n```perl\npar_walk \"./src\", sub { say $_ if /\\.rs$/ }\n\n# Collect all JSON files under a directory:\nmy @json_files\npar_walk \"/data\", sub { push @json_files, $_ if /\\.json$/ }\np scalar @json_files\n\n# Parallel content search:\npar_walk \".\", sub {\n    if (/\\.log$/) {\n        my @hits = grep /FATAL/, rl\n        p \"$_: \", scalar @hits, \" fatals\" if @hits\n    }\n}\n```",
        "par_sed" => "`par_sed PATTERN, REPLACEMENT, @files` — perform an in-place regex substitution across multiple files in parallel.\n\nEach file is processed by a separate worker thread: the file is read into memory, all matches of PATTERN are replaced with REPLACEMENT, and the result is written back atomically (via a temp file + rename, so readers never see a partially-written file). This is the perlrs equivalent of `sed -i` but parallelized across the file list — ideal for codebase-wide refactors, log scrubbing, or bulk config updates. The pattern uses perlrs regex syntax (PCRE-style). Returns the total number of substitutions made across all files.\n\n```perl\npar_sed qr/oldFunc/, \"newFunc\", glob(\"src/*.pl\")\n\n# Case-insensitive replace across a project:\npar_sed qr/TODO/i, \"DONE\", par_walk(\".\", sub { $_ if /\\.rs$/ })\n\n# Scrub sensitive data from logs:\nmy $n = par_sed qr/\\b\\d{3}-\\d{2}-\\d{4}\\b/, \"XXX-XX-XXXX\", @log_files\np \"redacted $n occurrences\"\n```",
        "par_fetch" => "`par_fetch @urls` — fetch a list of URLs in parallel using async HTTP, returning an array of response bodies in input order.\n\nUnder the hood, perlrs spawns concurrent HTTP GET requests across a connection pool with keep-alive and automatic retry on transient failures (5xx, timeouts). The degree of parallelism is bounded by the connection pool size (default 64) to avoid overwhelming the target server. For heterogeneous HTTP methods or custom headers, use `http_request` inside `pmap` instead. `par_fetch` is the right tool when you have a homogeneous list of URLs and just need the bodies — it handles connection reuse, DNS caching, and TLS session resumption automatically for maximum throughput.\n\n```perl\nmy @bodies = par_fetch @urls\n\n# Fetch and decode JSON in one shot:\nmy @data = par_fetch(@api_urls) |> map json_decode\n\n# Download pages with progress:\nmy @pages = par_fetch @urls, progress => 1\n```",
        "serve" => "Start a blocking HTTP server.\n\n```perl\nserve 8080, fn ($req) {\n    # $req = { method, path, query, headers, body, peer }\n    { status => 200, body => \"hello\" }\n}\n\nserve 3000, fn ($req) {\n    my $data = { name => \"perlrs\", version => \"0.4\" }\n    { status => 200, body => json_encode($data) }\n}, { workers => 8 }$1\n\nHandler returns: hashref `{ status, body, headers }`, string (200 OK), or undef (404).\nJSON content-type auto-detected when body starts with `{` or `[`.",
        "par_csv_read" => "`par_csv_read @files` — read multiple CSV files in parallel, returning an array of parsed datasets (one per file).\n\nEach file is read and parsed by a separate worker thread using a fast Rust CSV parser that handles quoting, escaping, and UTF-8 correctly. Headers are auto-detected from the first row of each file, and each row is returned as a hashref keyed by header names. This is dramatically faster than sequential CSV parsing when you have many files — common in data engineering pipelines where data arrives as daily/hourly CSV partitions. For a single large CSV file, prefer `par_lines` with manual splitting, since `par_csv_read` parallelizes across files, not within a single file.\n\n```perl\nmy @datasets = par_csv_read glob(\"data/*.csv\")\nfor my $ds (@datasets) {\n    p scalar @$ds, \" rows\"\n}\n\n# Merge all CSVs into one list:\nmy @all_rows = par_csv_read(@files) |> flat\np $all_rows[0]->{name};                # access by header\n\n# Filter and aggregate:\nmy @sales = par_csv_read(glob(\"sales_*.csv\")) |> flat\n  |> grep { $_->{region} eq \"US\" }\n```",

        // ── Typing (perlrs) ──
        "typed" => "`typed` adds optional runtime type annotations to lexical variables and subroutine parameters. When a `typed` declaration is in effect, perlrs inserts a lightweight check at assignment time that verifies the value matches the declared type (`Int`, `Str`, `Float`, `Bool`, `ArrayRef`, `HashRef`, or a user-defined `struct` name). This is especially useful for catching accidental type mismatches at function boundaries in larger programs. The annotation is purely a runtime guard — it has zero impact on pipeline performance because the check is only performed once at the point of assignment, not on every read.\n\n```perl\ntyped my $x : Int = 42\ntyped my $name : Str = \"hello\"\ntyped my $pi : Float = 3.14\nmy $add = fn ($a: Int, $b: Int) { $a + $b }\np $add->(3, 4);   # 7\n```\n\nNote: assigning a value of the wrong type raises a runtime exception immediately.\n\nYou can mix typed and untyped variables freely in the same scope, so adopting `typed` is incremental — annotate the variables that matter and leave the rest dynamic. Subroutine parameters declared with type annotations in `fn` are checked on every call, giving you contract-style validation at function boundaries without a separate assertion library.\n\n```perl\ntyped my @nums : Int = (1, 2, 3)\ntyped my %cfg : Str = (host => \"localhost\", port => \"8080\")\n```",
        "struct" => "`struct` declares a named record type with typed fields, giving perlrs lightweight struct semantics similar to Rust structs or Python dataclasses. Structs support multiple construction syntaxes, default values, field mutation, user-defined methods, functional updates, and structural equality.\n\n**Declaration:**\n```perl\nstruct Point { x => Float, y => Float };           # typed fields\nstruct Point { x => Float = 0.0, y => Float = 0.0 }; # with defaults\nstruct Pair { key, value };                        # untyped (Any)\n```\n\n**Construction:**\n```perl\nmy $p = Point(x => 1.5, y => 2.0);  # function-call with named args\nmy $p = Point(1.5, 2.0);            # positional (declaration order)\nmy $p = Point->new(x => 1.5, y => 2.0); # traditional OO style\nmy $p = Point();                    # uses defaults if defined\n```\n\n**Field access (getter/setter):**\n```perl\nsay $p->x;       # getter (0 args)\n$p->x(3.0);      # setter (1 arg)\n```\n\n**User-defined methods:**\n```perl\nstruct Circle {\n    radius => Float,\n    fn area { 3.14159 * $self->radius ** 2 }\n    fn scale($factor: Float) {\n        Circle(radius => $self->radius * $factor)\n    }\n}\nmy $c = Circle(radius => 5)\nsay $c->area;        # 78.53975\nsay $c->scale(2);    # Circle(radius => 10)\n```\n\n**Built-in methods:**\n```perl\nmy $q = $p->with(x => 5);  # functional update — new instance\nmy $h = $p->to_hash;       # { x => 1.5, y => 2.0 }\nmy @f = $p->fields;        # (x, y)\nmy $c = $p->clone;         # deep copy\n```\n\n**Smart stringify:**\n```perl\nsay $p;  # Point(x => 1.5, y => 2)\n```\n\n**Structural equality:**\n```perl\nmy $a = Point(1, 2)\nmy $b = Point(1, 2)\nsay $a == $b;  # 1 (compares all fields)\n```\n\nNote: field type is checked at construction and mutation; unknown field names are fatal errors.",

        // ── Data encoding / codecs ──
        "json_encode" => "`json_encode` serializes any Perl data structure — hashrefs, arrayrefs, nested combinations, numbers, strings, booleans, and undef — into a compact JSON string. It uses a fast Rust-backed serializer so it is significantly faster than `JSON::XS` for large payloads. The output is always valid UTF-8 JSON suitable for writing to files, sending over HTTP, or piping to other tools. Use `json_decode` to round-trip back.\n\n```perl\nmy %cfg = (debug => 1, paths => [\"/tmp\", \"/var\"])\nmy $j = json_encode(\\%cfg)\np $j;   # {\"debug\":1,\"paths\":[\"/tmp\",\"/var\"]}\n$j |> spurt \"/tmp/cfg.json\"$1\n\nNote: undef becomes JSON `null`; Perl booleans serialize as `true`/`false`.",
        "json_decode" => "`json_decode` parses a JSON string and returns the corresponding Perl data structure — hashrefs for objects, arrayrefs for arrays, and native scalars for strings/numbers/booleans. It is strict by default: malformed JSON raises an exception rather than returning partial data. This makes it safe to use in pipelines where corrupt input should halt processing. The Rust parser underneath handles large documents efficiently and supports full Unicode.\n\n```perl\nmy $data = json_decode('{\"name\":\"perlrs\",\"ver\":1}')\np $data->{name}   # perlrs\nslurp(\"data.json\") |> json_decode |> dd$1\n\nNote: JSON `null` becomes Perl `undef`; trailing commas and comments are not allowed.",
        "stringify" | "str" => "`stringify` (alias `str`) converts any perlrs value — scalars, array refs, hash refs, nested structures, undef — into a string representation that is a valid perlrs literal. The output is designed for round-tripping: you can `eval` the returned string to reconstruct the original data structure. This makes it ideal for serializing state to a file in a Perl-native format, generating code fragments, or building reproducible test fixtures. Unlike `dd`, which targets human readability, `str` prioritizes parseability.\n\n```perl\nmy $s = str {a => 1, b => [2, 3]}\np $s;               # {a => 1, b => [2, 3]}\nmy $copy = eval $s; # round-trip back to hashref\nmy @list = (1, \"hello\", undef)\np str \\@list;        # [1, \"hello\", undef]\n```\n\nNote: references are serialized recursively; circular references will cause infinite recursion.",
        "ddump" | "dd" => "`ddump` (alias `dd`) pretty-prints any perlrs data structure to stderr in a human-readable, indented format similar to Perl's `Data::Dumper`. It is the go-to tool for quick debugging — drop a `dd` call anywhere in a pipeline to inspect intermediate values without disrupting the data flow. The output is colorized when stderr is a terminal. Unlike `str`, the output is not meant for `eval` round-tripping; it prioritizes clarity over parseability. `dd` returns its argument unchanged, so it can be inserted into pipelines transparently.\n\n```perl\nmy %h = (name => \"Alice\", scores => [98, 87, 95])\ndd \\%h;                        # pretty-prints to stderr\nmy @result = @data |> dd |> grep { $_->{active} } |> dd\ndd [1, {a => 2}, [3, 4]];     # nested structure\n```\n\nNote: `dd` writes to stderr, not stdout, so it never contaminates pipeline output.",
        "to_json" | "tj" => "`to_json` (alias `tj`) converts a perlrs data structure into a JSON string, functioning as a convenient shorthand for `json_encode`. It accepts hashrefs, arrayrefs, scalars, and nested combinations, producing compact JSON output suitable for APIs, config files, or inter-process communication. The alias `tj` is particularly useful at the end of a pipeline to serialize the final result. The Rust-backed serializer handles large structures efficiently and always produces valid UTF-8.\n\n```perl\nmy %user = (name => \"Bob\", age => 30)\np tj \\%user;   # {\"age\":30,\"name\":\"Bob\"}\nmy @items = map { {id => $_, val => $_ * 2} } 1..3\np tj \\@items;  # [{\"id\":1,\"val\":2},{\"id\":2,\"val\":4},{\"id\":3,\"val\":6}]\n@data |> tj |> spurt \"out.json\"\n```",
        "to_csv" | "tc" => "`to_csv` (alias `tc`) serializes a list of hashrefs or arrayrefs into a CSV-formatted string, complete with a header row derived from hash keys when given hashrefs. This is the fastest way to produce spreadsheet-ready output from structured data. Fields containing commas, quotes, or newlines are automatically escaped according to RFC 4180. The alias `tc` keeps one-liners terse when piping query results or API responses straight to CSV.\n\n```perl\nmy @rows = ({name => \"Alice\", age => 30}, {name => \"Bob\", age => 25})\np tc \\@rows;   # name,age\\nAlice,30\\nBob,25\ntc(\\@rows) |> spurt \"people.csv\"\nmy @grid = ([1, 2, 3], [4, 5, 6])\np tc \\@grid;   # 1,2,3\\n4,5,6\n```",
        "to_toml" | "tt" => "`to_toml` (alias `tt`) serializes a perlrs hashref into a TOML-formatted string. TOML is a popular configuration format that maps cleanly to hash structures, making `tt` ideal for generating config files programmatically. Nested hashes become TOML sections, arrays become TOML arrays, and scalar values are serialized with their natural types. The output is always valid TOML that can be parsed back with `toml_decode`.\n\n```perl\nmy %cfg = (database => {host => \"localhost\", port => 5432}, debug => 1)\np tt \\%cfg\n# [database]\n# host = \"localhost\"\n# port = 5432\n# debug = 1\ntt(\\%cfg) |> spurt \"config.toml\"\n```",
        "to_yaml" | "ty" => "`to_yaml` (alias `ty`) serializes a perlrs data structure into a YAML-formatted string. YAML is widely used for configuration and data exchange where human readability matters. Nested structures are represented with indentation, arrays with leading dashes, and strings are quoted only when necessary. The output is valid YAML 1.2 that round-trips cleanly through `yaml_decode`. The alias `ty` is convenient for quick inspection of complex data.\n\n```perl\nmy %app = (name => \"myapp\", deps => [\"tokio\", \"serde\"], version => \"1.0\")\np ty \\%app\n# name: myapp\n# version: \"1.0\"\n# deps:\n#   - tokio\n#   - serde\nty(\\%app) |> spurt \"app.yaml\"\n```",
        "to_xml" | "tx" => "`to_xml` (alias `tx`) serializes a perlrs data structure into an XML string. Hash keys become element names, array values become repeated child elements, and scalar values become text content. This is useful for generating XML payloads for SOAP APIs, RSS feeds, or configuration files that require XML format. The output is well-formed XML that can be parsed back with `xml_decode`.\n\n```perl\nmy %doc = (root => {title => \"Hello\", items => [\"a\", \"b\", \"c\"]})\np tx \\%doc\n# <root><title>Hello</title><items>a</items><items>b</items><items>c</items></root>\ntx(\\%doc) |> spurt \"doc.xml\"\n```",
        "to_html" | "th" => "`to_html` (alias `th`) serializes a perlrs data structure into a self-contained HTML document with cyberpunk styling (dark background, neon cyan/magenta accents, monospace fonts). Arrays of hashrefs render as full tables with headers, plain arrays as bullet lists, single hashes as key-value tables, and scalars as styled text blocks. Pipe to a file and open in a browser for instant data visualization.\n\n```perl\nmy @rows = ({name => \"Alice\", age => 30}, {name => \"Bob\", age => 25})\nth(\\@rows) |> spurt \"people.html\"\nmy %cfg = (host => \"localhost\", port => 5432)\np th \\%cfg;   # full HTML page to stdout\n@data |> th |> spurt \"report.html\"\n```",
        "to_markdown" | "to_md" | "tmd" => "`to_markdown` (aliases `to_md`, `tmd`) serializes a perlrs data structure into Markdown text. Arrays of hashrefs render as GFM tables with headers and separator rows, plain arrays as bullet lists, single hashes as 2-column key-value tables, and scalars as plain text. The output is valid GitHub-Flavored Markdown suitable for README files, issue comments, or any Markdown renderer.\n\n```perl\nmy @rows = ({name => \"Alice\", age => 30}, {name => \"Bob\", age => 25})\np tmd \\@rows\n# | name | age |\n# | --- | --- |\n# | Alice | 30 |\n# | Bob | 25 |\ntmd(\\@rows) |> spurt \"table.md\"\nmy %h = (a => 1, b => 2)\np to_md \\%h\n```",
        "frequencies" | "freq" | "frq" => "`frequencies` (aliases `freq`, `frq`) counts how many times each distinct element appears in a list and returns a hashref mapping each value to its count. This is the perlrs equivalent of a histogram or counter — useful for analyzing log files, counting word occurrences, tallying categorical data, or finding duplicates. The input list is flattened, so you can pass arrays directly. The returned hashref can be fed into `dd` for inspection or `to_json` for serialization.\n\n```perl\nmy @words = qw(apple banana apple cherry banana apple)\nmy $counts = freq @words\np $counts;   # {apple => 3, banana => 2, cherry => 1}\nrl(\"access.log\") |> map { /^(\\S+)/ && $1 } |> freq |> dd\nmy @rolls = map { 1 + int(rand 6) } 1..1000\nfrq(@rolls) |> to_json |> p\n```",
        "interleave" | "il" => "`interleave` (alias `il`) merges two or more arrays by alternating their elements: first element of each array, then second element of each, and so on. If the arrays have different lengths, shorter arrays contribute `undef` for their missing positions. This is useful for building key-value pair lists from separate key and value arrays, creating round-robin schedules, or weaving parallel data streams together.\n\n```perl\nmy @keys = qw(name age city)\nmy @vals = (\"Alice\", 30, \"NYC\")\nmy @pairs = il \\@keys, \\@vals\np @pairs;   # name, Alice, age, 30, city, NYC\nmy %h = il \\@keys, \\@vals\np $h{name}; # Alice\nmy @rgb = il [255,0,0], [0,255,0], [0,0,255]\n```",
        "words" | "wd" => "`words` (alias `wd`) splits a string on whitespace boundaries and returns the resulting list of words. It handles leading, trailing, and consecutive whitespace gracefully — unlike a naive `split / /`, it never produces empty strings. This is the idiomatic way to tokenize a line of text in perlrs, and the short alias `wd` keeps pipelines compact. It is equivalent to Perl's `split ' '` behavior.\n\n```perl\nmy @w = wd \"  hello   world  \"\np @w;       # hello, world\nmy $line = \"  foo  bar  baz  \"\nmy $count = scalar wd $line\np $count;   # 3\nrl(\"data.txt\") |> map { scalar wd $_ } |> e p\n```",
        "count" | "len" | "cnt" => "`count` (aliases `len`, `cnt`) returns the number of elements in a list, the number of characters in a string, the number of key-value pairs in a hash, or the cardinality of a set. It is a universal length function that dispatches based on the type of its argument. This replaces the need to use `scalar @array` or `length $string` — `cnt` is shorter and works uniformly across types. In a pipeline, it naturally reduces a collection to a single number.\n\nNote: `size` is NOT a count alias — it returns a file's byte size (see `size`).\n\n```perl\nmy @arr = (1, 2, 3, 4, 5)\np cnt @arr;          # 5\np len \"hello\";       # 5\nmy %h = (a => 1, b => 2)\np cnt \\%h;           # 2\nrl(\"file.txt\") |> cnt |> p;  # line count\n```",
        "size" => "`size` returns the byte size of a file on disk — equivalent to Perl's `-s FILE` file test. With no arguments, it operates on `$_`; with one argument, it stats the given path; with multiple arguments (or a flattened list), it returns an array of sizes. Paths that can't be stat'd return `undef`. This is a perlrs extension that makes pipelines over filenames concise.\n\n```perl\np size \"Cargo.toml\";                   # 2013\nf |> map +{ $_ => size } |> tj |> p;   # [{name => bytes}, ...]\nf |> filter { size > 1024 } |> e p;    # files larger than 1 KiB\n```",
        "list_count" | "list_size" => "`list_count` (alias `list_size`) returns the total number of elements after flattening a nested list structure. Unlike `count` which returns the top-level element count, `list_count` recursively descends into array references and counts only leaf values. This is useful when you have a list of lists and need to know the total number of individual items rather than the number of sublists.\n\n```perl\nmy @nested = ([1, 2], [3, 4, 5], [6])\np list_count @nested;   # 6\nmy @deep = ([1, [2, 3]], [4])\np list_size @deep;      # 4\np list_count 1, 2, 3;   # 3  (flat list works too)\n```",
        "clamp" | "clp" => "`clamp` (alias `clp`) constrains each value in a list to lie within a specified minimum and maximum range. Values below the minimum are raised to it; values above the maximum are lowered to it; values already in range pass through unchanged. This is essential for sanitizing user input, bounding computed values before display, or enforcing physical constraints in simulations. When given a single scalar, it returns a single clamped value.\n\n```perl\nmy @scores = (105, -3, 42, 99, 200)\nmy @clamped = clp 0, 100, @scores\np @clamped;   # 100, 0, 42, 99, 100\nmy $val = clp 0, 255, $input;   # bound to byte range\nmy @pct = map { clp 0.0, 1.0, $_ } @raw_ratios\n```",
        "normalize" | "nrm" => "`normalize` (alias `nrm`) rescales a list of numeric values so that the minimum maps to 0 and the maximum maps to 1, using min-max normalization. This is a standard preprocessing step for machine learning features, data visualization (mapping values to color gradients or bar heights), and statistical analysis. If all values are identical, the result is a list of zeros to avoid division by zero. The output preserves the relative ordering of the input.\n\n```perl\nmy @temps = (32, 68, 100, 212)\nmy @normed = nrm @temps\np @normed;   # 0, 0.2, 0.377..., 1\nmy @pixels = nrm @raw_intensities\n@pixels |> map { int($_ * 255) } |> e p;  # scale to 0-255\n```",
        "snake_case" | "sc" => "`snake_case` (alias `sc`) converts a string from any common casing convention — camelCase, PascalCase, kebab-case, or mixed — into snake_case, where words are lowercase and separated by underscores. This is the standard naming convention for Perl and Rust variables and function names. Consecutive uppercase letters in acronyms are handled intelligently (e.g., `parseHTTPResponse` becomes `parse_http_response`).\n\n```perl\np sc \"camelCase\";          # camel_case\np sc \"PascalCase\";         # pascal_case\np sc \"kebab-case\";         # kebab_case\np sc \"parseHTTPResponse\";  # parse_http_response\nmy @methods = qw(getUserName setEmailAddr)\n@methods |> map sc |> e p\n```",
        "camel_case" | "cc" => "`camel_case` (alias `cc`) converts a string from any casing convention into camelCase, where the first word is lowercase and subsequent words are capitalized with no separators. This is the standard naming convention for JavaScript variables and Java methods. The function handles underscores, hyphens, and spaces as word boundaries and strips them during conversion.\n\n```perl\np cc \"snake_case\";         # snakeCase\np cc \"kebab-case\";         # kebabCase\np cc \"PascalCase\";         # pascalCase\np cc \"hello world\";        # helloWorld\nmy @cols = qw(first_name last_name email_addr)\n@cols |> map cc |> e p\n```",
        "kebab_case" | "kc" => "`kebab_case` (alias `kc`) converts a string from any casing convention into kebab-case, where words are lowercase and separated by hyphens. This is the standard naming convention for CSS classes, URL slugs, and CLI flag names. Like `snake_case`, it intelligently handles acronyms and mixed-case input. The function treats underscores, spaces, and case transitions as word boundaries.\n\n```perl\np kc \"camelCase\";          # camel-case\np kc \"PascalCase\";         # pascal-case\np kc \"snake_case\";         # snake-case\np kc \"parseHTTPResponse\";  # parse-http-response\nmy $slug = kc $title;      # URL-safe slug\n```",
        "json_jq" => "`json_jq` applies a jq-style query expression to a perlrs data structure and returns the matched value. This brings the power of the `jq` command-line JSON processor directly into perlrs without shelling out. Dot notation traverses hash keys, bracket notation indexes arrays, and nested paths are separated by dots. It is ideal for extracting deeply nested values from API responses or config files without chains of hash dereferences.\n\n```perl\nmy $data = json_decode(slurp \"api.json\")\np json_jq($data, \".results[0].name\")\nmy $cfg = rj \"config.json\"\np json_jq($cfg, \".database.host\");  # deep extract\nfetch_json(\"https://api.example.com/users\") |> json_jq(\".data[0].email\") |> p\n```",
        "toml_decode" => "`toml_decode` (alias `td`) parses a TOML-formatted string and returns the corresponding perlrs hash structure. TOML sections become nested hashrefs, arrays map to arrayrefs, and typed scalars (integers, floats, booleans, datetime strings) are preserved. This is useful for reading configuration files, parsing Cargo.toml manifests, or processing any TOML input. Malformed TOML raises an exception.\n\n```perl\nmy $cfg = toml_decode(slurp \"config.toml\")\np $cfg->{database}{host};   # localhost\nmy $cargo = toml_decode(slurp \"Cargo.toml\")\np $cargo->{package}{version}\nslurp(\"settings.toml\") |> toml_decode |> dd\n```",
        "toml_encode" => "`toml_encode` (alias `te`) serializes a perlrs hashref into a valid TOML string. Nested hashes become TOML sections with `[section]` headers, arrays become TOML arrays, and scalars are serialized with appropriate quoting. This is the inverse of `toml_decode` and is useful for generating or updating configuration files programmatically. The output is human-readable and can be written directly to a `.toml` file.\n\n```perl\nmy %cfg = (server => {host => \"0.0.0.0\", port => 8080}, debug => 0)\ntoml_encode(\\%cfg) |> spurt \"server.toml\"\nmy $round = toml_decode(toml_encode(\\%cfg))\np $round->{server}{port};  # 8080\n```",
        "xml_decode" => "`xml_decode` (alias `xd`) parses an XML string and returns a perlrs data structure. Elements become hash keys, text content becomes scalar values, repeated child elements become arrayrefs, and attributes are accessible through a conventions-based mapping. This is useful for consuming SOAP responses, RSS feeds, SVG files, or any XML-based API. Malformed XML raises an exception rather than returning partial data.\n\n```perl\nmy $doc = xml_decode('<root><name>perlrs</name><ver>1</ver></root>')\np $doc->{root}{name};   # perlrs\nslurp(\"feed.xml\") |> xml_decode |> dd\nmy $svg = xml_decode(fetch(\"https://example.com/image.svg\"))\np $svg->{svg}\n```",
        "xml_encode" => "`xml_encode` (alias `xe`) serializes a perlrs data structure into a well-formed XML string. Hash keys become element names, scalar values become text content, and arrayrefs become repeated sibling elements. This is useful for generating XML payloads for SOAP APIs, creating RSS or Atom feeds, or producing configuration files in XML format. The output is the inverse of `xml_decode` and round-trips cleanly.\n\n```perl\nmy %data = (root => {title => \"Test\", items => [{id => 1}, {id => 2}]})\np xml_encode \\%data\nxml_encode(\\%data) |> spurt \"output.xml\"\nmy $payload = xml_encode {request => {action => \"query\", id => 42}}\nhttp_request(method => \"POST\", url => $endpoint, body => $payload)\n```",
        "yaml_decode" => "`yaml_decode` (alias `yd`) parses a YAML string and returns the corresponding perlrs data structure. Mappings become hashrefs, sequences become arrayrefs, and scalars are coerced to their natural Perl types. This handles YAML 1.2 including multi-document streams, anchors/aliases, and flow notation. It is the go-to function for reading YAML configuration files, Kubernetes manifests, or CI pipeline definitions. Invalid YAML raises an exception.\n\n```perl\nmy $cfg = yaml_decode(slurp \"docker-compose.yml\")\np $cfg->{services}{web}{image}\nmy $ci = yaml_decode(slurp \".github/workflows/ci.yml\")\ndd $ci->{jobs}\nslurp(\"values.yaml\") |> yaml_decode |> json_encode |> p\n```",
        "yaml_encode" => "`yaml_encode` (alias `ye`) serializes a perlrs data structure into a valid YAML string. Hashes become YAML mappings, arrays become sequences with dash prefixes, and scalars are quoted only when necessary for disambiguation. The output is human-readable and suitable for writing config files, generating Kubernetes resources, or producing YAML-based API payloads. It is the inverse of `yaml_decode`.\n\n```perl\nmy %svc = (name => \"api\", replicas => 3, ports => [80, 443])\nyaml_encode(\\%svc) |> spurt \"service.yaml\"\nmy $round = yaml_decode(yaml_encode(\\%svc))\np $round->{replicas};   # 3\nrj(\"config.json\") |> yaml_encode |> p;  # JSON to YAML\n```",
        "csv_read" => "`csv_read` (alias `cr`) reads CSV data from a file path or string and returns an array of arrayrefs, where each inner arrayref represents one row. The parser handles RFC 4180 CSV correctly — quoted fields with embedded commas, newlines inside quotes, and escaped double quotes are all supported. The first row is treated as data (not a header) unless you process it separately. This is the fastest way to ingest tabular data in perlrs.\n\n```perl\nmy @rows = csv_read \"data.csv\"\np $rows[0];             # first row as arrayref\n@rows |> e { p $_->[0] };  # print first column\nmy @inline = csv_read \"a,b,c\\n1,2,3\\n4,5,6\"\np scalar @inline;       # 3 rows (including header)\n```",
        "csv_write" => "`csv_write` (alias `cw`) serializes an array of arrayrefs into a CSV-formatted string. Each inner arrayref becomes one row, and fields are automatically quoted when they contain commas, quotes, or newlines. This is the complement of `csv_read` and produces output conforming to RFC 4180. Use it to generate CSV files from computed data, export database query results, or prepare data for spreadsheet import.\n\n```perl\nmy @data = ([\"name\", \"age\"], [\"Alice\", 30], [\"Bob\", 25])\ncsv_write(\\@data) |> spurt \"people.csv\"\nmy @report = map { [$_->{id}, $_->{score}] } @results\np csv_write \\@report\n```",
        "dataframe" => "`dataframe` (alias `df`) creates a columnar dataframe from tabular data, providing a structured way to work with rows and columns. You can construct a dataframe from an array of hashrefs, an array of arrayrefs with a header row, or from a CSV file. The dataframe supports column selection, filtering, sorting, and aggregation operations. This is perlrs's answer to Python's pandas DataFrame — lightweight but sufficient for common data manipulation tasks.\n\n```perl\nmy $df = df [{name => \"Alice\", age => 30}, {name => \"Bob\", age => 25}]\np $df\nmy $df2 = df csv_read \"data.csv\"\nmy @ages = $df->{age}\np @ages;   # 30, 25\n```",
        "sqlite" => "`sqlite` (alias `sql`) executes a SQL statement against an SQLite database file and returns the results. For SELECT queries, it returns an array of hashrefs where each hashref represents one row with column names as keys. For INSERT, UPDATE, and DELETE statements, it returns the number of affected rows. Bind parameters prevent SQL injection and handle quoting automatically. The database file is created if it does not exist, making `sqlite` a zero-setup embedded database.\n\n```perl\nsqlite(\"app.db\", \"CREATE TABLE users (name TEXT, age INT)\")\nsqlite(\"app.db\", \"INSERT INTO users VALUES (?, ?)\", \"Alice\", 30)\nmy @rows = sqlite(\"app.db\", \"SELECT * FROM users WHERE age > ?\", 20)\n@rows |> e { p $_->{name} }\nsqlite(\"app.db\", \"SELECT count(*) as n FROM users\") |> dd\n```",

        // ── HTTP / networking ──
        "fetch" => "`fetch` (alias `ft`) performs a blocking HTTP GET request to the given URL and returns the response body as a string. It follows redirects automatically and raises an exception on network errors or non-2xx status codes. This is the simplest way to retrieve content from the web in perlrs — no need to configure a client or parse response objects. For JSON APIs, prefer `fetch_json` which additionally decodes the response.\n\n```perl\nmy $html = fetch \"https://example.com\"\np $html\nmy $ip = fetch \"https://api.ipify.org\"\np \"My IP: $ip\"\nfetch(\"https://example.com/data.txt\") |> spurt \"local.txt\"$1\n\nNote: for POST, PUT, DELETE, or custom headers, use `http_request` instead.",
        "fetch_json" => "`fetch_json` (alias `ftj`) performs a blocking HTTP GET request and automatically decodes the JSON response body into a perlrs data structure. This combines `fetch` and `json_decode` into a single call, which is the common case when consuming REST APIs. It raises an exception if the response is not valid JSON or the request fails. The returned value is typically a hashref or arrayref ready for immediate use.\n\n```perl\nmy $data = fetch_json \"https://api.github.com/repos/perlrs/perlrs\"\np $data->{stargazers_count}\nfetch_json(\"https://jsonplaceholder.typicode.com/todos\") |> e { p $_->{title} }\nmy $weather = fetch_json \"https://wttr.in/?format=j1\"\np $weather->{current_condition}[0]{temp_C}\n```",
        "fetch_async" => "`fetch_async` (alias `fta`) initiates a non-blocking HTTP GET request and returns a task handle that can be awaited later. This allows you to fire off multiple HTTP requests concurrently and wait for them all to complete, dramatically reducing total latency when fetching from several endpoints. The task resolves to the response body as a string, just like `fetch`. Use this when you need to parallelize network I/O without threads.\n\n```perl\nmy $t1 = fetch_async \"https://api.example.com/users\"\nmy $t2 = fetch_async \"https://api.example.com/posts\"\nmy $users = await $t1\nmy $posts = await $t2\np \"Got users and posts concurrently\"\n```",
        "fetch_async_json" => "`fetch_async_json` (alias `ftaj`) initiates a non-blocking HTTP GET request that automatically decodes the JSON response when awaited. This combines `fetch_async` with `json_decode`, making it the ideal choice for concurrent API calls where every response is JSON. The task resolves to the decoded perlrs data structure — a hashref, arrayref, or scalar depending on the JSON content.\n\n```perl\nmy @urls = map { \"https://api.example.com/item/$_\" } 1..10\nmy @tasks = map fetch_async_json @urls\nmy @results = map await @tasks\n@results |> e { p $_->{name} }\n```",
        "http_request" => "`http_request` (alias `hr`) performs a fully configurable HTTP request with control over method, headers, body, and timeout. Unlike `fetch` which is limited to GET, `http_request` supports POST, PUT, PATCH, DELETE, and any other HTTP method. Pass named parameters for configuration. The response is returned as a hashref containing `status`, `headers`, and `body` fields, giving you full access to the HTTP response. This is the right tool when you need to send data, set authentication headers, or inspect status codes.\n\n```perl\nmy $res = http_request(method => \"POST\", url => \"https://api.example.com/users\",\n    headers => {\"Content-Type\" => \"application/json\", Authorization => \"Bearer $token\"},\n    body => tj {name => \"Alice\"})\np $res->{status};   # 201\nmy $data = json_decode $res->{body}\nmy $del = http_request(method => \"DELETE\", url => \"https://api.example.com/users/42\")\n```",

        // ── Crypto / hashing ──
        "sha256" => "`sha256` (alias `s256`) computes the SHA-256 cryptographic hash of the input data and returns it as a 64-character lowercase hexadecimal string. SHA-256 is the most widely used hash function for data integrity verification, content addressing, and digital signatures. The Rust implementation is significantly faster than pure-Perl alternatives. Accepts strings or byte buffers.\n\n```perl\np sha256 \"hello world\";   # b94d27b9934d3e08...\nmy $checksum = sha256 slurp \"release.tar.gz\"\np $checksum\nrl(\"passwords.txt\") |> map sha256 |> e p\n```",
        "sha224" => "`sha224` (alias `s224`) computes the SHA-224 cryptographic hash and returns a 56-character hex string. SHA-224 is a truncated variant of SHA-256 that produces a shorter digest while maintaining strong collision resistance. It is sometimes preferred when storage or bandwidth for the hash value is constrained, such as in compact data structures or short identifiers.\n\n```perl\np sha224 \"hello world\";   # 2f05477fc24bb4fa...\nmy $h = sha224(tj {key => \"value\"})\np $h\n```",
        "sha384" => "`sha384` (alias `s384`) computes the SHA-384 cryptographic hash and returns a 96-character hex string. SHA-384 is a truncated variant of SHA-512 that offers a middle ground between SHA-256 and SHA-512 in both digest length and security margin. It is commonly used in TLS certificate fingerprints and government security standards that require larger-than-256-bit digests.\n\n```perl\np sha384 \"hello world\";   # fdbd8e75a67f29f7...\nmy $sig = sha384(slurp \"document.pdf\")\nspurt \"document.pdf.sha384\", $sig\n```",
        "sha512" => "`sha512` (alias `s512`) computes the SHA-512 cryptographic hash and returns a 128-character hex string. SHA-512 provides the largest digest size in the SHA-2 family and is the strongest option when maximum collision resistance is needed. On 64-bit systems, SHA-512 is often faster than SHA-256 because it operates on 64-bit words natively. Use this for high-security applications or when you need a longer hash.\n\n```perl\np sha512 \"hello world\";   # 309ecc489c12d6eb...\nmy $hash = sha512(slurp \"firmware.bin\")\np $hash\nmy @hashes = map sha512 @files\n```",
        "sha1" => "`sha1` (alias `s1`) computes the SHA-1 hash and returns a 40-character hex string. SHA-1 is considered cryptographically broken for collision resistance and should not be used for security-sensitive applications. However, it remains widely used for non-security purposes such as Git object IDs, cache keys, and deduplication checksums where collision attacks are not a concern.\n\n```perl\np sha1 \"hello world\"   # 2aae6c35c94fcfb4...\nmy $git_id = sha1(\"blob \" . length($content) . \"\\0\" . $content)\np $git_id$1\n\nNote: prefer SHA-256 for any security-related use case.",
        "crc32" => "`crc32` computes the CRC-32 checksum of the input data and returns it as an unsigned 32-bit integer. CRC-32 is not a cryptographic hash — it is a fast error-detection code used in network protocols (Ethernet, ZIP, PNG), file integrity checks, and hash table bucketing. It is extremely fast compared to SHA functions, making it suitable for high-throughput deduplication or quick change detection where collision resistance is not required.\n\n```perl\np crc32 \"hello world\";   # 222957957\nmy $chk = crc32(slurp \"archive.zip\")\np sprintf \"0x%08x\", $chk;  # hex representation\nmy @checksums = map crc32 @chunks\n```",
        "hmac_sha256" | "hmac" => "`hmac_sha256` (alias `hmac`) computes an HMAC-SHA256 message authentication code using the given data and secret key, returning a hex string. HMAC combines a cryptographic hash with a secret key to produce a signature that verifies both data integrity and authenticity. This is the standard mechanism for signing API requests (AWS, Stripe, GitHub webhooks), generating secure tokens, and verifying message authenticity.\n\n```perl\nmy $sig = hmac_sha256 \"request body\", \"my-secret-key\"\np $sig\nmy $webhook_sig = hmac(\"POST /hook\\n$body\", $secret)\np $webhook_sig eq $expected ? \"valid\" : \"tampered\"\n```",
        "base64_encode" => "`base64_encode` (alias `b64e`) encodes a string or byte buffer as a Base64 string using the standard alphabet (A-Z, a-z, 0-9, +, /). Base64 is the standard way to embed binary data in text-based formats like JSON, XML, email (MIME), and data URIs. The output length is always a multiple of 4, padded with `=` as needed. Use `base64_decode` to reverse the encoding.\n\n```perl\nmy $encoded = base64_encode \"hello world\"\np $encoded;   # aGVsbG8gd29ybGQ=\nmy $img_data = slurp \"photo.png\"\nmy $data_uri = \"data:image/png;base64,\" . base64_encode($img_data)\np base64_decode(base64_encode(\"round trip\"));  # round trip\n```",
        "base64_decode" => "`base64_decode` (alias `b64d`) decodes a Base64-encoded string back to its original bytes. It accepts standard Base64 with padding and is tolerant of line breaks within the input. This is essential for processing email attachments, decoding JWT payloads, extracting embedded images from data URIs, or reading any Base64-encoded field from an API response. Raises an exception on invalid Base64 input.\n\n```perl\nmy $decoded = base64_decode \"aGVsbG8gd29ybGQ=\"\np $decoded;   # hello world\nmy $img = base64_decode($api_response->{avatar_b64})\nspurt \"avatar.png\", $img\nmy $json = base64_decode($jwt_parts[1])\ndd json_decode $json\n```",
        "hex_encode" => "`hex_encode` (alias `hxe`) converts a string or byte buffer into its lowercase hexadecimal representation, with two hex characters per input byte. This is useful for displaying binary data in a human-readable format, generating hex-encoded keys or IDs, logging raw bytes, or preparing data for protocols that use hex encoding. The output is always an even number of characters.\n\n```perl\np hex_encode \"hello\";   # 68656c6c6f\nmy $raw = slurp \"key.bin\"\np hex_encode $raw\nmy $color = hex_encode chr(255) . chr(128) . chr(0)\np \"#$color\";   # #ff8000\n```",
        "hex_decode" => "`hex_decode` (alias `hxd`) converts a hexadecimal string back to its original bytes, interpreting every two hex characters as one byte. This is the inverse of `hex_encode` and is useful for parsing hex-encoded binary data from config files, network protocols, or cryptographic outputs. The input must have an even number of valid hex characters (0-9, a-f, A-F) or an exception is raised.\n\n```perl\nmy $bytes = hex_decode \"68656c6c6f\"\np $bytes;   # hello\nmy $key = hex_decode $env_hex_key\nmy $mac = hmac_sha256($data, hex_decode($secret_hex))\np hex_decode(hex_encode(\"round trip\"));  # round trip\n```",
        "uuid" => "`uuid` (alias `uid`) generates a cryptographically random UUID version 4 string in the standard 8-4-4-4-12 hyphenated format. Each call produces a unique identifier suitable for database primary keys, correlation IDs, session tokens, temporary file names, or any situation requiring a globally unique identifier without coordination. The randomness comes from the OS CSPRNG.\n\n```perl\nmy $id = uuid()\np $id;   # e.g., 550e8400-e29b-41d4-a716-446655440000\nmy %record = (id => uuid(), name => \"Alice\", created => time)\nmy @ids = map { uuid() } 1..10\n```",
        "jwt_encode" => "`jwt_encode` creates a signed JSON Web Token from a payload hashref and a secret key. The default algorithm is HS256 (HMAC-SHA256), but you can specify an alternative as the third argument. JWTs are the standard for stateless authentication tokens, API authorization, and secure inter-service communication. The returned string contains the base64url-encoded header, payload, and signature separated by dots.\n\n```perl\nmy $token = jwt_encode({sub => \"user123\", exp => time + 3600}, \"my-secret\")\np $token\nmy $admin = jwt_encode({role => \"admin\", iat => time}, $secret, \"HS512\")\n# send as Authorization header\nhttp_request(method => \"GET\", url => $api_url,\n    headers => {Authorization => \"Bearer $token\"})\n```",
        "jwt_decode" => "`jwt_decode` verifies the signature of a JSON Web Token using the provided secret key and returns the decoded payload as a hashref. If the signature is invalid, the token has been tampered with, or it has expired (when an `exp` claim is present), the function raises an exception. This is the secure way to validate incoming JWTs from clients or other services — always use this over `jwt_decode_unsafe` in production.\n\n```perl\nmy $payload = jwt_decode($token, \"my-secret\")\np $payload->{sub};   # user123\nmy $claims = jwt_decode($bearer_token, $secret)\nif ($claims->{role} eq \"admin\") {\n    p \"admin access granted\"\n}\n```\n\nNote: raises an exception on expired tokens, invalid signatures, or malformed input.",
        "jwt_decode_unsafe" => "`jwt_decode_unsafe` decodes a JSON Web Token and returns the payload as a hashref without verifying the signature. This is intentionally insecure and should only be used for debugging, logging, or inspecting token contents in development environments. Never use this to make authorization decisions in production — an attacker can forge arbitrary payloads. The function still parses the JWT structure and base64-decodes the payload, but skips all cryptographic checks.\n\n```perl\n# debugging only — never use for auth\nmy $claims = jwt_decode_unsafe($token)\ndd $claims\np $claims->{sub}   # inspect without needing the secret\nmy $exp = $claims->{exp}\np \"Expires: \" . datetime_from_epoch($exp)$1\n\nNote: this function exists for debugging. Use `jwt_decode` with a secret for any security-relevant validation.",

        // ── File I/O helpers ──
        "read_lines" | "rl" => "Read a file and return its contents as a list of lines with trailing newlines stripped. This is the idiomatic way to slurp a file line-by-line in perlrs without manually opening a filehandle. The short alias `rl` keeps one-liners concise. If the file does not exist, the program dies with an error message.\n\n```perl\nmy @lines = rl(\"data.txt\")\np scalar @lines               # line count\n@lines |> grep /ERROR/ |> e p # print error lines\nmy $first = (rl \"config.ini\")[0]$1\n\nNote: returns an empty list for an empty file.",
        "append_file" | "af" => "Append a string to the end of a file, creating it if it does not exist. This is the safe way to add content without overwriting — useful for log files, CSV accumulation, or incremental output. The short alias `af` is convenient in pipelines. The file is opened, written, and closed atomically per call.\n\n```perl\naf(\"log.txt\", \"started at \" . datetime_utc() . \"\\n\")\n1..5 |> e { af(\"nums.txt\", \"$_\\n\") }\nmy @data = (\"a\",\"b\",\"c\")\n@data |> e { af \"out.txt\", \"$_\\n\" }\n```",
        "to_file" => "Write a string to a file, truncating any existing content. Unlike `append_file`, this replaces the file entirely. Returns the written content so it can be used in a pipeline — write to disk and continue processing in one expression. Creates the file if it does not exist.\n\n```perl\nmy $csv = \"name,age\\nAlice,30\\nBob,25\"\n$csv |> to_file(\"people.csv\") |> p\nto_file(\"empty.txt\", \"\");  # truncate a file\n```\n\nNote: the return-value-for-piping behavior distinguishes this from a plain write.",
        "tempfile" | "tf" => "Create a temporary file in the system temp directory and return its absolute path as a string. The file is created with a unique name and exists on disk immediately. Use `tf` as a short alias for quick scratch files in one-liners. The caller is responsible for cleanup, though OS temp-directory reaping will eventually reclaim it.\n\n```perl\nmy $tmp = tf()\nto_file($tmp, \"scratch data\\n\")\np rl($tmp);           # scratch data\nmy @all = map { tf() } 1..3;  # three temp files\n```",
        "tempdir" | "tdr" => "Create a temporary directory in the system temp directory and return its absolute path. The directory is created with a unique name and is ready for use immediately. The short alias `tdr` mirrors `tf` for files. Useful for isolating multi-file operations like test fixtures, build artifacts, or staged output.\n\n```perl\nmy $dir = tdr()\nto_file(\"$dir/a.txt\", \"hello\")\nto_file(\"$dir/b.txt\", \"world\")\nmy @files = glob(\"$dir/*.txt\")\np scalar @files;   # 2\n```",
        "read_json" | "rj" => "Read a JSON file from disk and parse it into a perlrs data structure (hash ref or array ref). The short alias `rj` keeps JSON-config one-liners terse. Dies if the file does not exist or contains malformed JSON. This is the complement of `write_json`/`wj`.\n\n```perl\nmy $cfg = rj(\"config.json\")\np $cfg->{database}{host}\nmy @items = @{ rj(\"list.json\") }\n@items |> e { p $_->{name} }$1\n\nNote: numeric strings remain strings; use `+0` to coerce if needed.",
        "write_json" | "wj" => "Serialize a perlrs data structure (hash ref or array ref) as pretty-printed JSON and write it to a file. Creates or overwrites the target file. The short alias `wj` pairs with `rj` for round-trip JSON workflows. Useful for persisting configuration, caching API responses, or generating fixture data.\n\n```perl\nmy %data = (name => \"Alice\", scores => [98, 87, 95])\nwj(\"out.json\", \\%data)\nmy $back = rj(\"out.json\")\np $back->{name};   # Alice\n```",

        // ── Compression ──
        "gzip" => "Compress a string or byte buffer using the gzip (RFC 1952) format and return the compressed bytes. Useful for shrinking data before writing to disk or sending over the network. Pairs with `gunzip` for decompression. The compression level is chosen automatically for a good speed/size tradeoff.\n\n```perl\nmy $raw = \"hello world\" x 1000\nmy $gz = gzip($raw)\nto_file(\"data.gz\", $gz)\np length($gz);       # much smaller than original\np gunzip($gz) eq $raw;  # 1\n```",
        "gunzip" => "Decompress gzip-compressed data (RFC 1952) and return the original bytes. Dies if the input is not valid gzip. Use this to read `.gz` files or decompress data received from HTTP responses with `Content-Encoding: gzip`. Always the inverse of `gzip`.\n\n```perl\nmy $compressed = rl(\"archive.gz\")\nmy $text = gunzip($compressed)\np $text\n# round-trip in a pipeline\n\"payload\" |> gzip |> gunzip |> p;  # payload\n```",
        "zstd" => "Compress a string or byte buffer using the Zstandard algorithm and return the compressed bytes. Zstandard offers significantly better compression ratios and speed compared to gzip, making it ideal for large datasets, IPC buffers, and caching. Pairs with `zstd_decode` for decompression.\n\n```perl\nmy $big = \"x]\" x 100_000\nmy $compressed = zstd($big)\np length($compressed);  # fraction of original\nto_file(\"data.zst\", $compressed)\np zstd_decode($compressed) eq $big;  # 1\n```",
        "zstd_decode" => "Decompress Zstandard-compressed data and return the original bytes. Dies if the input is not valid Zstandard. This is the inverse of `zstd`. Use it to read `.zst` files or decompress cached buffers that were compressed with `zstd`.\n\n```perl\nmy $packed = zstd(\"important data\\n\" x 500)\nmy $original = zstd_decode($packed)\np $original\n# file round-trip\nto_file(\"cache.zst\", zstd($payload))\np zstd_decode(rl(\"cache.zst\"))\n```",

        // ── URL encoding ──
        "url_encode" | "uri_escape" => "Percent-encode a string so it is safe to embed in a URL query parameter or path segment. Unreserved characters (alphanumeric, `-`, `_`, `.`, `~`) are left as-is; everything else becomes `%XX`. The alias `uri_escape` matches the classic `URI::Escape` name for Perl muscle-memory.\n\n```perl\nmy $q = \"hello world & friends\"\nmy $safe = url_encode($q)\np $safe   # hello%20world%20%26%20friends\nmy $url = \"https://example.com/search?q=\" . url_encode($q)\np $url$1\n\nNote: does not encode the full URL structure — encode individual components, not the whole URL.",
        "url_decode" | "uri_unescape" => "Decode a percent-encoded string back to its original form, converting `%XX` sequences to the corresponding bytes and `+` to space. The alias `uri_unescape` matches `URI::Escape` conventions. Use this when parsing query strings from incoming URLs or reading URL-encoded form data.\n\n```perl\nmy $encoded = \"hello%20world%20%26%20friends\"\np url_decode($encoded);   # hello world & friends\n# round-trip\nmy $orig = \"café ☕\"\np url_decode(url_encode($orig)) eq $orig;  # 1\n```",

        // ── Logging ──
        "log_info" => "Log a message at INFO level to stderr with a timestamp prefix. INFO is the default visible level and is appropriate for normal operational messages — startup notices, progress milestones, summary statistics. Messages are suppressed if the current log level is set higher than INFO.\n\n```perl\nlog_info(\"server started on port $port\")\nmy @rows = rl(\"data.csv\")\nlog_info(\"loaded \" . scalar(@rows) . \" rows\")\n1..5 |> e { log_info(\"processing item $_\") }\n```",
        "log_warn" => "Log a message at WARN level to stderr. Warnings indicate unexpected but recoverable situations — missing optional config, deprecated usage, slow operations. WARN messages appear at the default log level and are visually distinct from INFO in structured log output.\n\n```perl\nlog_warn(\"config file not found, using defaults\")\nlog_warn(\"query took ${elapsed}s, exceeds threshold\")\nunless (-e $path) {\n    log_warn(\"$path missing, skipping\")\n}\n```",
        "log_error" => "Log a message at ERROR level to stderr. Use this for failures that prevent an operation from completing but do not necessarily terminate the program — failed network requests, invalid input, permission errors. ERROR is always visible regardless of log level.\n\n```perl\nlog_error(\"failed to connect to $host: $!\")\neval { rj(\"bad.json\") }\nlog_error(\"parse failed: $@\") if $@\nlog_error(\"missing required field 'name'\")\n```",
        "log_debug" => "Log a message at DEBUG level to stderr. Debug messages are hidden by default and only appear when the log level is lowered to DEBUG or TRACE via `log_level`. Use for detailed internal state that helps during development — variable dumps, branch decisions, intermediate values.\n\n```perl\nlog_level(\"debug\")\nlog_debug(\"cache key: $key\")\nmy $result = compute($x)\nlog_debug(\"compute($x) => $result\")\n@items |> e { log_debug(\"item: $_\") }\n```",
        "log_trace" => "Log a message at TRACE level to stderr. This is the most verbose level, producing very fine-grained output — loop iterations, function entry/exit, raw payloads. Only visible when `log_level(\"trace\")` is set. Use sparingly in production code; primarily for deep debugging sessions.\n\n```perl\nlog_level(\"trace\")\nfn process($x) {\n    log_trace(\"entering process($x)\")\n    my $r = $x * 2\n    log_trace(\"leaving process => $r\")\n    $r\n}\n1..3 |> map process |> e p\n```",
        "log_json" => "Emit a structured JSON log line to stderr containing the message plus any additional key-value metadata. This is designed for machine-parseable logging pipelines — centralized log collectors, JSON-based monitoring, or `jq`-friendly output. Each call emits exactly one JSON object per line.\n\n```perl\nlog_json(\"request\", method => \"GET\", path => \"/api\")\nlog_json(\"metric\", name => \"latency_ms\", value => 42)\nlog_json(\"error\", msg => $@, file => $0)$1\n\nNote: all values are serialized as JSON strings.",
        "log_level" => "Get or set the current minimum log level. When called with no arguments, returns the current level as a string. When called with a level name, sets it for all subsequent log calls. Valid levels from most to least verbose: `trace`, `debug`, `info`, `warn`, `error`. The default level is `info`.\n\n```perl\np log_level();         # info\nlog_level(\"debug\");    # enable debug output\nlog_debug(\"now visible\")\nlog_level(\"error\");    # suppress everything below error\nlog_info(\"hidden\");    # not printed\n```",

        // ── Datetime ──
        "datetime_utc" => "Return the current UTC date and time as an ISO 8601 string (e.g. `2026-04-15T12:30:00Z`). This is the simplest way to get a portable, unambiguous timestamp for logging, file naming, or serialization. The returned string always ends with `Z` indicating UTC, so there is no timezone ambiguity.\n\n```perl\nmy $now = datetime_utc()\np $now;                          # 2026-04-15T12:30:00Z\naf(\"audit.log\", \"$now: started\\n\")\nmy %event = (ts => datetime_utc(), action => \"deploy\")\nwj(\"event.json\", \\%event)\n```",
        "datetime_from_epoch" => "Convert a Unix epoch timestamp (seconds since 1970-01-01 00:00:00 UTC) into an ISO 8601 datetime string. This is useful when you have raw epoch values from `time()`, file modification times, or external APIs and need a human-readable representation. Fractional seconds are truncated.\n\n```perl\nmy $ts = 1700000000\np datetime_from_epoch($ts);       # 2023-11-14T22:13:20Z\nmy $born = datetime_from_epoch(0)\np $born;                          # 1970-01-01T00:00:00Z\nmy @epochs = (1e9, 1.5e9, 2e9)\n@epochs |> e { p datetime_from_epoch }\n```",
        "datetime_strftime" => "Format an epoch timestamp using a `strftime`-style format string, giving full control over the output representation. The first argument is the format pattern and the second is the epoch value. Supports all standard specifiers: `%Y` (4-digit year), `%m` (month), `%d` (day), `%H` (hour), `%M` (minute), `%S` (second), `%A` (weekday name), and more.\n\n```perl\nmy $t = time()\np datetime_strftime(\"%Y-%m-%d\", $t);        # 2026-04-15\np datetime_strftime(\"%H:%M:%S\", $t);        # 14:23:07\np datetime_strftime(\"%A, %B %d\", $t);       # Wednesday, April 15\nmy $log_ts = datetime_strftime(\"%Y%m%d_%H%M%S\", $t)\nto_file(\"backup_$log_ts.sql\", $data)\n```",
        "datetime_now_tz" => "Return the current date and time in a specified IANA timezone as a formatted string. Pass a timezone name like `America/New_York`, `Europe/London`, or `Asia/Tokyo`. This avoids manual UTC-offset arithmetic and handles daylight saving transitions correctly. Dies if the timezone name is not recognized.\n\n```perl\np datetime_now_tz(\"America/New_York\");    # 2026-04-15 08:30:00 EDT\np datetime_now_tz(\"Asia/Tokyo\");           # 2026-04-15 21:30:00 JST\np datetime_now_tz(\"UTC\");                  # same as datetime_utc\nmy @offices = (\"US/Pacific\", \"Europe/Berlin\", \"Asia/Kolkata\")\n@offices |> e { p \"$_: \" . datetime_now_tz }\n```",
        "datetime_format_tz" => "Format an epoch timestamp in a specific IANA timezone, combining the capabilities of `datetime_strftime` and `datetime_now_tz`. This lets you render a historical or future timestamp as it would appear on the wall clock in any timezone. Handles DST transitions automatically.\n\n```perl\nmy $epoch = 1700000000\np datetime_format_tz($epoch, \"America/Chicago\")\n# 2023-11-14 16:13:20 CST\np datetime_format_tz($epoch, \"Europe/London\")\n# 2023-11-14 22:13:20 GMT\np datetime_format_tz(time(), \"Australia/Sydney\")\n```",
        "datetime_parse_local" => "Parse a local datetime string (without timezone info) into a Unix epoch timestamp, interpreting it in the system's local timezone. Accepts common formats like `2026-04-15 14:30:00` or `2026-04-15T14:30:00`. Dies if the string cannot be parsed. This is the inverse of formatting with `localtime`.\n\n```perl\nmy $epoch = datetime_parse_local(\"2026-04-15 14:30:00\")\np $epoch;                          # Unix timestamp\np datetime_from_epoch($epoch);     # back to ISO 8601\nmy $midnight = datetime_parse_local(\"2026-01-01 00:00:00\")\np time() - $midnight;              # seconds since New Year\n```",
        "datetime_parse_rfc3339" => "Parse an RFC 3339 / ISO 8601 datetime string (with timezone offset or `Z` suffix) into a Unix epoch timestamp. This is the standard format used by JSON APIs, RSS feeds, and git timestamps. Accepts strings like `2026-04-15T14:30:00Z` or `2026-04-15T14:30:00+05:30`. Dies on malformed input.\n\n```perl\nmy $epoch = datetime_parse_rfc3339(\"2026-04-15T12:00:00Z\")\np $epoch\nmy $with_tz = datetime_parse_rfc3339(\"2026-04-15T08:00:00-04:00\")\np $epoch == $with_tz;              # 1 (same instant)\n# parse API response timestamps\nmy $created = $response->{created_at}\nmy $age = time() - datetime_parse_rfc3339($created)\np \"created ${age}s ago\"\n```",
        "datetime_add_seconds" => "Add (or subtract) a number of seconds to an ISO 8601 datetime string and return the resulting ISO 8601 string. This performs calendar-aware arithmetic, correctly crossing day, month, and year boundaries. Pass a negative number to subtract time. Useful for computing deadlines, expiration times, or time windows.\n\n```perl\nmy $now = datetime_utc()\nmy $later = datetime_add_seconds($now, 3600);     # +1 hour\np $later\nmy $yesterday = datetime_add_seconds($now, -86400); # -1 day\np $yesterday\nmy $deadline = datetime_add_seconds($now, 7 * 86400); # +1 week\np \"due by $deadline\"\n```",
        "elapsed" | "el" => "Return the number of seconds elapsed since the perlrs process started, using a monotonic clock that is immune to system clock adjustments. The short alias `el` keeps benchmarking one-liners terse. Returns a floating-point value with sub-millisecond precision. Useful for timing operations, profiling hot loops, or adding relative timestamps to log output.\n\n```perl\nmy $t0 = el()\nmy @sorted = sort @big_array\nmy $dur = el() - $t0\np \"sort took ${dur}s\"\n# progress logging\n1..100 |> e { do_work; log_info(\"step $_ at \" . el() . \"s\") }\n```",
        "time" => "Return the current Unix epoch as an integer — the number of seconds since 1970-01-01 00:00:00 UTC. This is the standard wall-clock timestamp used for file times, database records, and interop with external systems. For monotonic timing of code sections, prefer `elapsed`/`el` instead since `time` can jump if the system clock is adjusted.\n\n```perl\nmy $start = time()\nsleep(2)\np time() - $start;   # ~2\nmy $ts = time()\nwj(\"stamp.json\", { created => $ts })\np datetime_from_epoch($ts);  # human-readable form\n```",
        "times" => "Return the accumulated CPU times for the process as a four-element list: `($user, $system, $child_user, $child_system)`. User time is CPU spent executing your code; system time is CPU spent in kernel calls on your behalf. Child times cover subprocesses. Values are in seconds (floating point). Useful for profiling whether a script is CPU-bound or I/O-bound.\n\n```perl\nmy ($u, $s, $cu, $cs) = times()\np \"user=${u}s sys=${s}s\"\n# after heavy computation\nmy ($u2, $s2) = times()\np \"used \" . ($u2 - $u) . \"s of CPU\"\np \"total CPU: \" . ($u2 + $s2) . \"s\"\n```",
        "localtime" => "Convert a Unix epoch timestamp to a nine-element list of broken-down local time components: `($sec, $min, $hour, $mday, $mon, $year, $wday, $yday, $isdst)`. Follows the Perl convention where `$mon` is 0-based (January=0) and `$year` is years since 1900. When called without arguments, uses the current time. Use `gmtime` for the UTC equivalent.\n\n```perl\nmy @t = localtime(time())\np \"$t[2]:$t[1]:$t[0]\";                 # HH:MM:SS\nmy $year = $t[5] + 1900\nmy $mon  = $t[4] + 1\np \"$year-$mon-$t[3]\";                   # YYYY-M-D\nmy @days = qw(Sun Mon Tue Wed Thu Fri Sat)\np $days[$t[6]];                          # day of week\n```",
        "gmtime" => "Convert a Unix epoch timestamp to a nine-element list of broken-down UTC time components, identical in structure to `localtime` but always in the UTC timezone. The fields are `($sec, $min, $hour, $mday, $mon, $year, $wday, $yday, $isdst)` where `$isdst` is always 0. When called without arguments, uses the current time.\n\n```perl\nmy @utc = gmtime(time())\nmy $year = $utc[5] + 1900\nmy $mon  = $utc[4] + 1\np sprintf(\"%04d-%02d-%02dT%02d:%02d:%02dZ\",\n    $year, $mon, @utc[3,2,1,0])\n# compare local vs UTC\nmy @loc = localtime(time())\np \"UTC hour=$utc[2] local hour=$loc[2]\"\n```",
        "sleep" => "Pause execution for the specified number of seconds. Accepts both integer and fractional values for sub-second sleeps (e.g. `sleep 0.1` for 100ms). The process yields the CPU during the sleep, so it is safe to use in polling loops without burning cycles. Returns the unslept time (always 0 unless interrupted by a signal).\n\n```perl\np \"waiting...\"; sleep(2); p \"done\"\n# polling loop\nwhile (!-e \"done.flag\") {\n    sleep(0.5)\n}\n# rate limiting\nmy @urls = @targets\n@urls |> e { fetch; sleep(0.1) }\n```",
        "alarm" => "Schedule a `SIGALRM` signal to be delivered to the process after the specified number of seconds. Calling `alarm(0)` cancels any pending alarm. Only one alarm can be active at a time — setting a new alarm replaces the previous one. Returns the number of seconds remaining on the previous alarm (or 0 if none was set). Combine with `eval` and `$SIG{ALRM}` to implement timeouts around potentially hanging operations.\n\n```perl\neval {\n    local $SIG{ALRM} = fn { die \"timeout\\n\" }\n    alarm(5);           # 5 second deadline\n    my $data = slow_network_call()\n    alarm(0);           # cancel on success\n}\nif ($@ =~ /timeout/) {\n    log_error(\"operation timed out\")\n}\n```",

        // ── File / path utilities ──
        "basename" | "bn" => "Extract the filename component from a path, stripping all leading directory segments. The short alias `bn` keeps one-liner pipelines terse. If an optional suffix argument is provided, that suffix is also stripped from the result, which is handy for removing extensions.\n\n```perl\np basename(\"/usr/local/bin/pe\");        # pe\np bn(\"/tmp/data.csv\", \".csv\");           # data\n\"/etc/nginx/nginx.conf\" |> bn |> p;      # nginx.conf\n```",
        "dirname" | "dn" => "Return the directory portion of a path, stripping the final filename component. The short alias `dn` mirrors `bn`. This is a pure string operation — it does not touch the filesystem, so it works on paths that do not exist yet. Useful for deriving output directories from input file paths.\n\n```perl\np dirname(\"/usr/local/bin/pe\");          # /usr/local/bin\np dn(\"/tmp/data.csv\");                   # /tmp\nmy $dir = dn($0);                        # directory of current script\n```",
        "fileparse" => "Split a path into its three logical components: the filename, the directory prefix, and a suffix that matches one of the supplied patterns. This mirrors Perl's `File::Basename::fileparse` and is the most flexible path decomposition available. When no suffix patterns are given the suffix is empty.\n\n```perl\nmy ($name, $dir, $sfx) = fileparse(\"/home/user/report.txt\", qr/\\.txt/)\np \"$dir | $name | $sfx\";  # /home/user/ | report | .txt\nmy ($n, $d) = fileparse(\"./lib/Foo/Bar.pm\")\np \"$d$n\";                 # ./lib/Foo/Bar.pm\n```",
        "canonpath" => "Clean up a file path by collapsing redundant separators, resolving `.` and `..` segments, and normalizing trailing slashes — all without touching the filesystem. Unlike `realpath`, this is a purely lexical operation so it works on paths that do not exist. Use it to normalize user-supplied paths before comparison or storage.\n\n```perl\np canonpath(\"/usr/./local/../local/bin/\");   # /usr/local/bin\np canonpath(\"a/b/../c\");                     # a/c\nmy $clean = canonpath($ENV{HOME} . \"/./docs/../docs\")\np $clean\n```",
        "realpath" | "rp" => "Resolve a path to its absolute canonical form by following all symbolic links and eliminating `.` and `..` segments. Unlike `canonpath`, this hits the filesystem and will die if any component does not exist. The short alias `rp` is convenient in pipelines. Use this when you need a guaranteed unique path for deduplication or comparison.\n\n```perl\np realpath(\".\");                      # /home/user/project\np rp(\"../sibling\");                   # /home/user/sibling\nmy $canon = rp($0);                  # absolute path of current script\n\".\" |> rp |> p\n```",
        "getcwd" | "pwd" => "Return the current working directory as an absolute path string. This calls the underlying OS `getcwd` function, so it always reflects the real directory even if the process changed directories via `chdir`. The alias `pwd` matches the familiar shell command. Often used to save and restore directory context around `chdir` calls.\n\n```perl\nmy $orig = pwd()\nchdir(\"/tmp\")\np pwd();      # /tmp\nchdir($orig)\np getcwd();   # back to original\n```",
        "gethostname" | "hn" => "Return the hostname of the current machine as a string. This calls the POSIX `gethostname` system call. The short alias `hn` is useful in log prefixes, temp-file naming, or distributed-system identifiers where you need to tag output by machine.\n\n```perl\np gethostname();                          # myhost.local\nmy $log_prefix = hn() . \":\" . $$;        # myhost.local:12345\nlog_info(\"running on \" . hn())\n```",
        "which" => "Search the `PATH` environment variable for the first executable matching the given command name and return its absolute path, or `undef` if not found. This is the programmatic equivalent of the shell `which` command. Useful for checking tool availability before calling `system` or `exec`.\n\n```perl\nmy $gcc = which(\"gcc\") // die \"gcc not found\"\np $gcc;                       # /usr/bin/gcc\nif (which(\"rg\")) {\n    system(\"rg pattern file\")\n} else {\n    system(\"grep pattern file\")\n}\n```",
        "which_all" | "wha" => "Return a list of all absolute paths matching the given command name across every directory in `PATH`, not just the first match. The short alias `wha` keeps things concise. This is useful for detecting shadowed executables or auditing which versions of a tool are installed.\n\n```perl\nmy @all = which_all(\"python3\")\n@all |> e p;                # /usr/local/bin/python3\n                             # /usr/bin/python3\np scalar wha(\"perl\");        # number of perls on PATH\n```",
        "glob_match" => "Test whether a filename or path matches a shell-style glob pattern. Returns true (1) on match, false (empty string) otherwise. Supports `*`, `?`, `[abc]`, and `{a,b}` patterns. This is a pure string match — it does not read the filesystem, so it works for filtering lists of paths you already have.\n\n```perl\np glob_match(\"*.pl\", \"script.pl\");        # 1\np glob_match(\"*.pl\", \"script.py\");        # (empty)\nmy @perl = grep { glob_match(\"*.{pl,pm}\", $_) } @files\n@perl |> e p\n```",
        "copy" => "Copy a file from a source path to a destination path. The destination can be a file path or a directory (in which case the source filename is preserved). Dies on failure. This is the programmatic equivalent of `cp` and avoids shelling out. Metadata such as permissions is preserved where possible.\n\n```perl\ncopy(\"src/config.yaml\", \"/tmp/config.yaml\")\ncopy(\"report.pdf\", \"/backup/\")\nmy $tmp = tf()\ncopy($0, $tmp);  # back up the current script\np slurp($tmp)\n```",
        "move" | "mv" => "Move or rename a file from source to destination. If the source and destination are on the same filesystem this is an atomic rename; otherwise it falls back to copy-then-delete. Dies on failure. The short alias `mv` mirrors the shell command.\n\n```perl\nmove(\"draft.txt\", \"final.txt\");         # rename in place\nmv(\"output.csv\", \"/archive/output.csv\"); # move across dirs\nmy $tmp = tf()\nspurt($tmp, \"data\")\nmv($tmp, \"data.txt\")\n```",
        "read_bytes" | "slurp_raw" => "Read an entire file into memory as raw bytes without any encoding interpretation. Unlike `slurp`, which returns a decoded UTF-8 string, `read_bytes` preserves the exact byte content — useful for binary files like images, compressed archives, or protocol buffers. The alias `slurp_raw` emphasizes the raw nature.\n\n```perl\nmy $png = read_bytes(\"logo.png\")\np length($png);                        # byte count\nmy $gz = slurp_raw(\"data.gz\")\nmy $text = gunzip($gz)\np $text\n```",
        "spurt" | "write_file" | "wf" => "Write a string to a file, creating it if it does not exist or truncating it if it does. This is the complement of `slurp` — together they form a read/write pair for whole-file operations. The short alias `wf` is convenient for one-liners. The file is opened, written, and closed in a single call.\n\n```perl\nspurt(\"hello.txt\", \"Hello, world!\\n\")\nwf(\"nums.txt\", join(\"\\n\", 1..10))\n\"generated content\" |> wf(\"out.txt\")\nmy $data = slurp(\"in.txt\")\nwf(\"copy.txt\", $data)\n```",
        "mkdir" => "Create a directory at the given path. An optional second argument specifies the permission mode as an octal number (default `0777`, modified by the current `umask`). Dies if the directory cannot be created. Only creates one level — use `make_path` or shell out for recursive creation.\n\n```perl\nmkdir(\"output\")\nmkdir(\"/tmp/secure\", 0700)\nmy $dir = tdr() . \"/sub\"\nmkdir($dir)\np -d $dir;    # 1\n```",
        "rmdir" => "Remove an empty directory. Dies if the directory does not exist, is not empty, or cannot be removed due to permissions. This only removes a single directory — it will not recursively delete contents. Remove files with `unlink` first, then call `rmdir`.\n\n```perl\nmkdir(\"scratch\")\nrmdir(\"scratch\")\np -d \"scratch\";   # (empty, dir is gone)\nunlink(\"tmp/file.txt\")\nrmdir(\"tmp\")\n```",
        "unlink" => "Delete one or more files from the filesystem. Returns the number of files successfully removed. Does not remove directories — use `rmdir` for those. Dies on permission errors. Accepts a list of paths, making it convenient for batch cleanup.\n\n```perl\nunlink(\"temp.log\")\nmy $n = unlink(\"a.tmp\", \"b.tmp\", \"c.tmp\")\np \"removed $n files\"\nmy @old = glob(\"*.bak\")\nunlink(@old)\n```",
        "rename" => "Rename a file or directory from an old name to a new name. This is an atomic operation on the same filesystem. If the destination already exists it is silently replaced. Dies on failure. Unlike `move`/`mv`, this does not fall back to copy-then-delete across filesystems.\n\n```perl\nrename(\"draft.md\", \"final.md\")\nrename(\"output\", \"output_v2\");          # works on dirs too\nmy $bak = \"config.yaml.bak\"\nrename(\"config.yaml\", $bak)\nspurt(\"config.yaml\", $new_config)\n```",
        "link" => "Create a hard link — a new directory entry pointing to the same underlying inode as the original file. Both names are indistinguishable and share the same data; removing one does not affect the other. Hard links cannot cross filesystem boundaries and typically cannot link directories.\n\n```perl\nlink(\"data.csv\", \"data_backup.csv\")\nmy @st1 = stat(\"data.csv\")\nmy @st2 = stat(\"data_backup.csv\")\np $st1[1] == $st2[1];   # 1 — same inode\n```",
        "symlink" => "Create a symbolic (soft) link that points to a target path. Unlike hard links, symlinks can cross filesystems and can point to directories. The link stores the target as a string, so it can dangle if the target is later removed. Use `readlink` to inspect where a symlink points.\n\n```perl\nsymlink(\"/usr/local/bin/pe\", \"pe_link\")\np readlink(\"pe_link\");       # /usr/local/bin/pe\np -l \"pe_link\";              # 1 (is a symlink)\nsymlink(\"../lib\", \"lib_link\");  # relative target\n```",
        "readlink" => "Return the target path that a symbolic link points to, without following the link further. Returns `undef` if the path is not a symlink. This is useful for inspecting symlink chains, verifying link targets, or resolving one level of indirection at a time.\n\n```perl\nsymlink(\"real.conf\", \"link.conf\")\nmy $target = readlink(\"link.conf\")\np $target;                    # real.conf\nif (defined readlink($path)) {\n    p \"$path is a symlink\"\n}\n```",
        "stat" => "Return a 13-element list of file status information for a path or filehandle: `($dev, $ino, $mode, $nlink, $uid, $gid, $rdev, $size, $atime, $mtime, $ctime, $blksize, $blocks)`. This calls the POSIX `stat` system call. Use it to check file size, modification time, permissions, and other metadata without reading the file.\n\n```perl\nmy @st = stat(\"data.bin\")\np \"size: $st[7] bytes\"\np \"modified: \" . datetime_from_epoch($st[9])\nmy ($mode) = (stat($0))[2]\np sprintf(\"perms: %04o\", $mode & 07777)\n```",
        "chmod" => "Change the permission bits of one or more files. The mode is specified as an octal number. Returns the number of files successfully changed. Does not follow symlinks on some platforms. Use `stat` to read the current mode before modifying.\n\n```perl\nchmod(0755, \"script.pl\")\nchmod(0644, \"config.yaml\", \"data.json\")\nmy $n = chmod(0600, glob(\"*.key\"))\np \"secured $n key files\"\n```",
        "chown" => "Change the owner and group of one or more files, specified as numeric UID and GID. Pass `-1` for either to leave it unchanged. Returns the number of files successfully changed. Typically requires root privileges.\n\n```perl\nchown(1000, 1000, \"app.log\")\nchown(-1, 100, \"shared.txt\");     # change group only\nmy $uid = (getpwnam(\"deploy\"))[2]\nchown($uid, -1, \"release.tar\")\n```",
        "chdir" => "Change the current working directory of the process. Dies if the directory does not exist or is not accessible. This affects all subsequent relative path operations. Pair with `getcwd`/`pwd` to save and restore the original directory.\n\n```perl\nmy $orig = pwd()\nchdir(\"/tmp\")\nspurt(\"scratch.txt\", \"hello\")\nchdir($orig);                  # return to original\n```",
        "glob" => "Expand a shell-style glob pattern against the filesystem and return a list of matching paths. Supports `*`, `?`, `[abc]`, `{a,b}` patterns, and `**` for recursive matching. This actually reads the filesystem, unlike `glob_match` which is a pure string test.\n\n```perl\nmy @scripts = glob(\"*.pl\")\n@scripts |> e p\nmy @all_rs = glob(\"src/**/*.rs\")\np scalar @all_rs;              # count of Rust files\nmy @cfg = glob(\"/etc/{nginx,apache2}/*.conf\")\n```",
        "opendir" => "Open a directory handle for reading its entries. Returns a directory handle that can be passed to `readdir`, `seekdir`, `telldir`, `rewinddir`, and `closedir`. Dies if the directory does not exist or cannot be opened. For most use cases `glob` or `readdir` with a path is simpler.\n\n```perl\nopendir(my $dh, \"/tmp\") or die \"cannot open: $!\"\nmy @entries = readdir($dh)\nclosedir($dh)\n@entries |> grep { $_ !~ /^\\./ } |> e p;  # skip dotfiles\n```",
        "readdir" => "Read entries from a directory handle opened with `opendir`. In list context, returns all remaining entries. In scalar context, returns the next single entry or `undef` when exhausted. Entries include `.` and `..` so you typically filter them out.\n\n```perl\nopendir(my $dh, \".\") or die $!\nwhile (my $entry = readdir($dh)) {\n    next if $entry =~ /^\\./\n    p $entry\n}\nclosedir($dh)\n```",
        "closedir" => "Close a directory handle previously opened with `opendir`, releasing the underlying OS resource. While directory handles are closed automatically when they go out of scope, explicit `closedir` is good practice in long-running programs or loops that open many directories.\n\n```perl\nopendir(my $dh, \"/var/log\") or die $!\nmy @logs = readdir($dh)\nclosedir($dh)\n@logs |> grep { /\\.log$/ } |> e p\n```",
        "seekdir" => "Set the current position in a directory handle to a location previously obtained from `telldir`. This allows you to revisit directory entries without closing and reopening the handle. Rarely needed in practice, but useful for multi-pass directory scanning.\n\n```perl\nopendir(my $dh, \".\") or die $!\nmy $pos = telldir($dh)\nmy @first_pass = readdir($dh)\nseekdir($dh, $pos);              # rewind to saved position\nmy @second_pass = readdir($dh)\nclosedir($dh)\n```",
        "telldir" => "Return the current read position within a directory handle as an opaque integer. The returned value can be passed to `seekdir` to return to that position later. This is the directory-handle equivalent of `tell` for file handles.\n\n```perl\nopendir(my $dh, \"/tmp\") or die $!\nreaddir($dh);                  # skip .\nreaddir($dh);                  # skip ..\nmy $pos = telldir($dh);        # save position after . and ..\nmy @real = readdir($dh)\nseekdir($dh, $pos);            # go back\n```",
        "rewinddir" => "Reset a directory handle back to the beginning so that the next `readdir` returns the first entry again. This is equivalent to closing and reopening the directory but more efficient. Useful when you need to iterate a directory multiple times.\n\n```perl\nopendir(my $dh, \"src\") or die $!\nmy $count = scalar readdir($dh)\nrewinddir($dh)\nmy @entries = readdir($dh)\nclosedir($dh)\np \"$count entries\"\n```",
        "utime" => "Set the access time and modification time of one or more files. Times are specified as epoch seconds. Pass `undef` for either time to set it to the current time. Returns the number of files successfully updated. Useful for cache invalidation, build systems, or preserving timestamps after transformations.\n\n```perl\nutime(time(), time(), \"output.txt\");     # touch to now\nmy $epoch = 1700000000\nutime($epoch, $epoch, @files);           # backdate files\nutime(undef, undef, \"marker.flag\");      # equivalent of touch\n```",
        "umask" => "Get or set the file creation mask, which controls the default permissions for newly created files and directories. When called with an argument, sets the new mask and returns the previous one. When called without arguments, returns the current mask. The mask is subtracted from the requested permissions in `mkdir`, `open`, etc.\n\n```perl\nmy $old = umask(0077);         # restrict: owner-only\nmkdir(\"private\");               # created with 0700\numask($old);                    # restore previous mask\np sprintf(\"%04o\", umask());     # print current mask\n```",
        "uname" => "Return system identification as a five-element list: `($sysname, $nodename, $release, $version, $machine)`. This calls the POSIX `uname` system call and is useful for platform-specific logic, logging system info, or generating diagnostic reports without shelling out.\n\n```perl\nmy ($sys, $node, $rel, $ver, $arch) = uname()\np \"$sys $rel ($arch)\";           # Linux 6.1.0 (x86_64)\nif ($sys eq \"Darwin\") {\n    p \"running on macOS\"\n}\nlog_info(\"host: $node, kernel: $rel\")\n```",

        // ── Networking / sockets ──
        "socket" => "Create a network socket with the specified domain, type, and protocol. The socket handle is stored in the first argument and can then be used with `bind`, `connect`, `send`, `recv`, and other socket operations. Domain constants include `AF_INET` (IPv4) and `AF_INET6` (IPv6); type constants include `SOCK_STREAM` (TCP) and `SOCK_DGRAM` (UDP).\n\n```perl\nsocket(my $sock, AF_INET, SOCK_STREAM, 0)\nmy $addr = sockaddr_in(8080, inet_aton(\"127.0.0.1\"))\nconnect($sock, $addr)\nsend($sock, \"GET / HTTP/1.0\\r\\n\\r\\n\", 0)\n```",
        "bind" => "Bind a socket to a local address so it can accept connections or receive datagrams on that address. The address is a packed `sockaddr_in` or `sockaddr_in6` structure. Binding is required before calling `listen` on a server socket. Dies if the address is already in use unless `SO_REUSEADDR` is set.\n\n```perl\nsocket(my $srv, AF_INET, SOCK_STREAM, 0)\nsetsockopt($srv, SOL_SOCKET, SO_REUSEADDR, 1)\nbind($srv, sockaddr_in(8080, INADDR_ANY)) or die \"bind: $!\"\nlisten($srv, 5)\n```",
        "listen" => "Mark a bound socket as passive, ready to accept incoming connections. The backlog argument specifies the maximum number of pending connections the OS will queue before refusing new ones. This is only meaningful for stream (TCP) sockets. Call `accept` in a loop after `listen` to handle clients.\n\n```perl\nsocket(my $srv, AF_INET, SOCK_STREAM, 0)\nbind($srv, sockaddr_in(9000, INADDR_ANY))\nlisten($srv, 128) or die \"listen: $!\"\nwhile (accept(my $client, $srv)) {\n    send($client, \"hello\\n\", 0)\n}\n```",
        "accept" => "Accept a pending connection on a listening socket and return a new connected socket handle. The new handle is used for communication with that specific client while the original listening socket continues accepting others. Returns the packed remote address on success, false on failure.\n\n```perl\nlisten($srv, 5)\nwhile (my $remote = accept(my $client, $srv)) {\n    my ($port, $ip) = sockaddr_in($remote)\n    p \"connection from \" . inet_ntoa($ip) . \":$port\"\n    send($client, \"welcome\\n\", 0)\n}\n```",
        "connect" => "Initiate a connection from a socket to a remote address. For TCP sockets this performs the three-way handshake; for UDP it sets the default destination so subsequent `send` calls do not need an address. Dies or returns false if the connection is refused or times out.\n\n```perl\nsocket(my $sock, AF_INET, SOCK_STREAM, 0)\nmy $addr = sockaddr_in(80, inet_aton(\"example.com\"))\nconnect($sock, $addr) or die \"connect: $!\"\nsend($sock, \"GET / HTTP/1.0\\r\\n\\r\\n\", 0)\nrecv($sock, my $buf, 4096, 0)\np $buf\n```",
        "send" => "Send data through a connected socket. The flags argument controls behavior — use `0` for normal sends. For UDP sockets you can supply a destination address as a fourth argument to send to a specific peer without calling `connect` first. Returns the number of bytes sent, or `undef` on error.\n\n```perl\nsend($sock, \"hello world\\n\", 0)\nmy $n = send($sock, $payload, 0)\np \"sent $n bytes\"\n# UDP to specific peer\nsend($udp, $msg, 0, sockaddr_in(5000, inet_aton(\"10.0.0.1\")))\n```",
        "recv" => "Receive data from a socket into a buffer. The length argument specifies the maximum number of bytes to read. For stream sockets a short read is normal — loop until you have all expected data. For datagram sockets each call returns exactly one datagram. Returns the sender address for UDP, or empty string for TCP.\n\n```perl\nrecv($sock, my $buf, 4096, 0) or die \"recv: $!\"\np $buf\nmy $data = \"\"\nwhile (recv($sock, my $chunk, 8192, 0) && length($chunk)) {\n    $data .= $chunk\n}\n```",
        "shutdown" => "Shut down part or all of a socket connection without closing the file descriptor. The `how` argument controls direction: `0` stops reading, `1` stops writing (sends FIN to peer), `2` stops both. This is useful for signaling end-of-data to the remote side while still reading its response.\n\n```perl\nsend($sock, $request, 0)\nshutdown($sock, 1);           # done writing\nrecv($sock, my $resp, 65536, 0);  # still read response\nshutdown($sock, 2);           # fully close\n```",
        "setsockopt" => "Set an option on a socket at the specified protocol level. Common uses include enabling `SO_REUSEADDR` to allow immediate rebinding after a server restart, setting `TCP_NODELAY` to disable Nagle's algorithm, or adjusting buffer sizes. The value is typically a packed integer.\n\n```perl\nsetsockopt($srv, SOL_SOCKET, SO_REUSEADDR, 1)\nsetsockopt($sock, IPPROTO_TCP, TCP_NODELAY, 1)\nsetsockopt($sock, SOL_SOCKET, SO_RCVBUF, pack(\"I\", 262144))\n```",
        "getsockopt" => "Retrieve the current value of a socket option at the specified protocol level. Returns the option value as a packed binary string — use `unpack` to interpret it. Useful for inspecting buffer sizes, checking whether `SO_REUSEADDR` is set, or reading OS-assigned values.\n\n```perl\nmy $val = getsockopt($sock, SOL_SOCKET, SO_RCVBUF)\np unpack(\"I\", $val);                 # e.g. 131072\nmy $reuse = getsockopt($srv, SOL_SOCKET, SO_REUSEADDR)\np unpack(\"I\", $reuse);               # 1 or 0\n```",
        "getpeername" => "Return the packed socket address of the remote end of a connected socket. Use `sockaddr_in` or `sockaddr_in6` to unpack it into a port and IP address. This is how a server discovers which client it is talking to after `accept`, or how a client confirms the peer address after `connect`.\n\n```perl\nmy $packed = getpeername($client)\nmy ($port, $ip) = sockaddr_in($packed)\np \"peer: \" . inet_ntoa($ip) . \":$port\"\n```",
        "getsockname" => "Return the packed socket address of the local end of a socket. This is useful when the socket was bound to `INADDR_ANY` or port `0` (OS-assigned) and you need to discover the actual address and port the OS chose. Unpack the result with `sockaddr_in`.\n\n```perl\nbind($sock, sockaddr_in(0, INADDR_ANY));  # OS picks port\nmy $local = getsockname($sock)\nmy ($port, $ip) = sockaddr_in($local)\np \"listening on port $port\"\n```",
        "gethostbyname" => "Resolve a hostname to its network addresses using the system resolver. Returns `($name, $aliases, $addrtype, $length, @addrs)`. The addresses are packed binary — pass them through `inet_ntoa` to get dotted-quad strings. This is the classic DNS forward-lookup function.\n\n```perl\nmy @info = gethostbyname(\"example.com\")\nmy @addrs = @info[4..$;#info]\n@addrs |> map inet_ntoa |> e p\nmy $ip = inet_ntoa((gethostbyname(\"localhost\"))[4])\np $ip;   # 127.0.0.1\n```",
        "gethostbyaddr" => "Perform a reverse DNS lookup — given a packed binary IP address and address family, return the hostname associated with that address. Returns `($name, $aliases, $addrtype, $length, @addrs)` on success, or empty list if no PTR record exists.\n\n```perl\nmy $packed = inet_aton(\"8.8.8.8\")\nmy $name = (gethostbyaddr($packed, AF_INET))[0]\np $name;   # dns.google\n```",
        "getpwent" => "Read the next entry from the system password database, iterating through all user accounts. Returns `($name, $passwd, $uid, $gid, $quota, $comment, $gcos, $dir, $shell)` for each user, or empty list when exhausted. Call `setpwent` to rewind and `endpwent` to close.\n\n```perl\nwhile (my @pw = getpwent()) {\n    p \"$pw[0]: uid=$pw[2] home=$pw[7]\"\n}\nendpwent()\n```",
        "getgrent" => "Read the next entry from the system group database, iterating through all groups. Returns `($name, $passwd, $gid, $members)` for each group, or empty list when exhausted. The members field is a space-separated string of usernames. Call `endgrent` when done.\n\n```perl\nwhile (my @gr = getgrent()) {\n    p \"$gr[0]: gid=$gr[2] members=$gr[3]\"\n}\nendgrent()\n```",
        "getprotobyname" => "Look up a network protocol by its name and return protocol information. Returns `($name, $aliases, $proto_number)`. The protocol number is what you pass to `socket` as the protocol argument. Common names include `tcp`, `udp`, and `icmp`.\n\n```perl\nmy ($name, $aliases, $proto) = getprotobyname(\"tcp\")\np \"$name = protocol $proto\";   # tcp = protocol 6\nsocket(my $raw, AF_INET, SOCK_RAW, (getprotobyname(\"icmp\"))[2])\n```",
        "getservbyname" => "Look up a network service by its name and protocol, returning the port number and related information. Returns `($name, $aliases, $port, $proto)`. The port is in host byte order. This resolves well-known service names like `http`, `ssh`, or `smtp` to their port numbers portably.\n\n```perl\nmy ($name, $aliases, $port) = getservbyname(\"http\", \"tcp\")\np \"$name => port $port\";         # http => port 80\nmy $ssh_port = (getservbyname(\"ssh\", \"tcp\"))[2]\np $ssh_port;                     # 22\n```",

        // ── Process ──
        "fork" => "Fork the current process, creating a child that is an exact copy of the parent. Returns the child's PID to the parent process and `0` to the child, allowing each side to branch. Returns `undef` on failure. Always pair with `wait`/`waitpid` to reap the child and avoid zombies.\n\n```perl\nmy $pid = fork()\nif ($pid == 0) {\n    p \"child $$\"\n    exit(0)\n}\np \"parent $$, child is $pid\"\nwaitpid($pid, 0)\n```",
        "exec" => "Replace the current process image entirely with a new command. This never returns on success — the new program takes over. If `exec` fails (command not found, permission denied) it returns false and execution continues. Use `system` instead if you want to run a command and keep the current process alive.\n\n```perl\nexec(\"ls\", \"-la\", \"/tmp\") or die \"exec failed: $!\"\n# code here only runs if exec fails\n\n# common fork+exec pattern\nif (fork() == 0) {\n    exec(\"worker\", \"--daemon\")\n}\n```",
        "system" => "Execute a command in a subshell and wait for it to complete, returning the exit status. A return value of `0` means success; non-zero indicates failure. The exit status is encoded as `$? >> 8` for the actual exit code. Use backticks or `capture` if you need the command's output.\n\n```perl\nmy $rc = system(\"make\", \"test\")\np \"exit code: \" . ($rc >> 8)\nsystem(\"cp data.csv /backup/\") == 0 or die \"copy failed\"\nif (system(\"which rg >/dev/null 2>&1\") == 0) {\n    p \"ripgrep is installed\"\n}\n```",
        "wait" => "Wait for any child process to terminate and return its PID. The exit status is stored in `$?`. If there are no child processes, returns `-1`. This is the simplest reaping function — use `waitpid` when you need to wait for a specific child or use non-blocking flags.\n\n```perl\nfor (1..3) {\n    fork() or do { sleep(1); exit(0) }\n}\nwhile ((my $pid = wait()) != -1) {\n    p \"child $pid exited with \" . ($? >> 8)\n}\n```",
        "waitpid" => "Wait for a specific child process identified by PID to change state. The flags argument controls behavior — use `0` for blocking wait, or `WNOHANG` for non-blocking (returns `0` if child is still running). Returns the PID on success, `-1` if the child does not exist. Exit status is in `$?`.\n\n```perl\nmy $pid = fork() // die \"fork: $!\"\nif ($pid == 0) { sleep(2); exit(42) }\nwaitpid($pid, 0)\np \"child exited: \" . ($? >> 8);   # 42\n\n# non-blocking poll\nwhile (waitpid($pid, WNOHANG) == 0) {\n    p \"still running...\"\n    sleep(1)\n}\n```",
        "kill" => "Send a signal to one or more processes by PID. The signal can be specified as a number (`9`) or a name (`\"TERM\"`). Sending signal `0` tests whether the process exists without actually sending anything. Returns the number of processes successfully signaled.\n\n```perl\nkill(\"TERM\", $child_pid)\nkill(9, @worker_pids);                # SIGKILL\nif (kill(0, $pid)) {\n    p \"process $pid is alive\"\n}\nmy $n = kill(\"HUP\", @daemons)\np \"signaled $n processes\"\n```",
        "exit" => "Terminate the program immediately with the given exit status code. An exit code of `0` conventionally means success; any non-zero value indicates an error. `END` blocks and object destructors are still run. Use `POSIX::_exit` to skip cleanup entirely.\n\n```perl\nexit(0);                 # success\nexit(1) if $error       # failure\n\n# conditional exit in a pipeline\nmy $ok = system(\"make test\")\nexit($ok >> 8) if $ok\n```",
        "getlogin" => "Return the login name of the user who owns the current terminal session. This reads from the system's utmp/utmpx database and may return `undef` for processes without a controlling terminal (cron jobs, daemons). For a more reliable alternative, use `getpwuid($<)` which looks up the effective UID.\n\n```perl\nmy $user = getlogin() // (getpwuid($<))[0]\np \"running as $user\"\nlog_info(\"session started by \" . getlogin())\n```",
        "getpwuid" => "Look up user account information by numeric UID. Returns `($name, $passwd, $uid, $gid, $quota, $comment, $gcos, $dir, $shell)` on success, or empty list if the UID does not exist. This is the reliable way to map a UID to a username and home directory.\n\n```perl\nmy ($name, undef, undef, undef, undef, undef, undef, $home) = getpwuid($<)\np \"user: $name, home: $home\"\nmy $root_shell = (getpwuid(0))[8]\np $root_shell;   # /bin/bash or /bin/zsh\n```",
        "getpwnam" => "Look up user account information by username string. Returns the same 9-element list as `getpwuid`: `($name, $passwd, $uid, $gid, $quota, $comment, $gcos, $dir, $shell)`. Returns empty list if the user does not exist. Useful for resolving a username to a UID before calling `chown`.\n\n```perl\nmy @info = getpwnam(\"deploy\")\np \"uid=$info[2] home=$info[7]\"\nmy $uid = (getpwnam(\"www-data\"))[2]\nchown($uid, -1, \"public/index.html\")\n```",
        "getgrgid" => "Look up group information by numeric GID. Returns `($name, $passwd, $gid, $members)` where members is a space-separated string of usernames belonging to the group. Returns empty list if the GID does not exist.\n\n```perl\nmy ($name, undef, undef, $members) = getgrgid(0)\np \"group $name: $members\"\nmy $gname = (getgrgid((stat(\"file.txt\"))[5]))[0]\np \"file group: $gname\"\n```",
        "getgrnam" => "Look up group information by group name string. Returns `($name, $passwd, $gid, $members)`. Useful for resolving a group name to a GID before calling `chown`, or for checking group membership.\n\n```perl\nmy ($name, undef, $gid, $members) = getgrnam(\"staff\")\np \"gid=$gid members=$members\"\nchown(-1, $gid, \"shared_dir\")\nif ((getgrnam(\"admin\"))[3] =~ /\\b$user\\b/) {\n    p \"$user is an admin\"\n}\n```",
        "getppid" => "Return the process ID of the parent process. This is useful for detecting whether the process has been orphaned (parent PID becomes 1 on Unix when the original parent exits), or for logging the process hierarchy. Always returns a valid PID.\n\n```perl\np \"my pid: $$, parent: \" . getppid()\nif (getppid() == 1) {\n    log_warn(\"parent process has exited, we are orphaned\")\n}\n```",
        "getpgrp" => "Return the process group ID of the current process (or of the specified PID). Processes in the same group receive signals together — for example, Ctrl-C sends SIGINT to the entire foreground process group. Use `setpgrp` to move a process into a different group.\n\n```perl\np \"process group: \" . getpgrp()\nmy $pg = getpgrp($$)\np $pg == $$ ? \"group leader\" : \"group member\"\n```",
        "setpgrp" => "Set the process group ID of a process. Call `setpgrp(0, 0)` to make the current process a new group leader, which is useful for daemonization or isolating a subprocess from the terminal's signal group. Takes `(PID, PGID)` — use `0` for the current process.\n\n```perl\nsetpgrp(0, 0);   # become process group leader\nif (fork() == 0) {\n    setpgrp(0, 0);  # child gets its own group\n    exec(\"worker\")\n}\n```",
        "getpriority" => "Get the scheduling priority (nice value) of a process, process group, or user. The `which` argument selects the target type: `PRIO_PROCESS`, `PRIO_PGRP`, or `PRIO_USER`. Lower values mean higher priority. The default nice value is `0`; range is typically `-20` to `19`.\n\n```perl\nmy $nice = getpriority(0, $$);    # PRIO_PROCESS, current PID\np \"nice value: $nice\"\nmy $user_prio = getpriority(2, $<);  # PRIO_USER, current user\np \"user priority: $user_prio\"\n```",
        "setpriority" => "Set the scheduling priority (nice value) of a process, process group, or user. Lowering the nice value (higher priority) typically requires root privileges. Raising it (lower priority) is always allowed. Use this to deprioritize background batch work or boost latency-sensitive tasks.\n\n```perl\nsetpriority(0, $$, 10);   # lower priority for batch work\nif (fork() == 0) {\n    setpriority(0, 0, 19);  # lowest priority for child\n    exec(\"batch-job\")\n}\n```",

        // ── Misc builtins ──
        "pack" => "Convert a list of values into a binary string according to a template. Each template character specifies how one value is encoded: `N` for 32-bit big-endian unsigned, `n` for 16-bit, `a` for raw bytes, `Z` for null-terminated string, etc. This is essential for constructing binary protocols, file formats, and `sockaddr` structures.\n\n```perl\nmy $bin = pack(\"NnA4\", 0xDEADBEEF, 8080, \"test\")\np length($bin);                # 10 bytes\nmy $header = pack(\"A8 N N\", \"MAGIC01\\0\", 1, 42)\nspurt(\"data.bin\", $header)\n```",
        "unpack" => "Decode a binary string into a list of values according to a template, performing the inverse of `pack`. The template characters must match how the data was packed. Use this for parsing binary file formats, network protocol headers, or any structured binary data.\n\n```perl\nmy ($magic, $version, $count) = unpack(\"A8 N N\", $header)\np \"v$version, $count records\"\nmy ($port, $addr) = unpack(\"n a4\", $sockaddr)\np inet_ntoa($addr) . \":$port\"\n```",
        "vec" => "Treat a string as a bit vector and get or set individual elements at a specified bit width. The first argument is the string, the second is the element offset, and the third is the bit width (1, 2, 4, 8, 16, or 32). As an lvalue, `vec` modifies the string in place. Useful for compact boolean arrays and bitmap manipulation.\n\n```perl\nmy $bits = \"\"\nvec($bits, 0, 1) = 1;   # set bit 0\nvec($bits, 7, 1) = 1;   # set bit 7\np vec($bits, 0, 1);     # 1\np vec($bits, 3, 1);     # 0\np unpack(\"B8\", $bits);  # 10000001\n```",
        "tie" => "Bind a variable to an implementing class so that all accesses (read, write, delete, etc.) are intercepted by methods on that class. This is Perl's mechanism for transparent object-backed variables — tied hashes can be backed by a database, tied scalars can validate on assignment, etc. Use `untie` to remove the binding.\n\n```perl\ntie my %db, 'DB_File', 'cache.db'\n$db{key} = \"value\";         # writes to disk\np $db{key};                  # reads from disk\nuntie %db\n```",
        "prototype" => "Return the prototype string of a named function, or `undef` if the function has no prototype. Prototypes control how arguments are parsed at compile time — they influence context and reference-passing behavior. Useful for introspection and metaprogramming.\n\n```perl\np prototype(\"CORE::push\");     # \\@@\np prototype(\"CORE::map\");      # &@\nfn greet($name) { p \"hi $name\" }\np prototype(\\&greet);          # undef (signatures, no proto)\n```",
        "bless" => "Associate a reference with a package name, turning it into an object of that class. The blessed reference can then have methods called on it via `->`. The second argument defaults to the current package. This is the foundation of Perl's object system.\n\n```perl\nfn new($class, %args) {\n    bless { %args }, $class\n}\nmy $obj = new(\"Dog\", name => \"Rex\", breed => \"Lab\")\np ref($obj);          # Dog\np $obj->{name};       # Rex\n```",
        "rand" => "Return a pseudo-random floating-point number in the range `[0, N)`. If N is omitted it defaults to `1`. The result is never exactly equal to N. For integer results, combine with `int`. Seed the generator with `srand` for reproducible sequences.\n\n```perl\np rand();                # e.g. 0.7342...\np int(rand(100));        # random int 0..99\nmy @deck = 1..52\nmy @shuffled = sort { rand() <=> rand() } @deck;  # poor shuffle\nmy $coin = rand() < 0.5 ? \"heads\" : \"tails\"\np $coin\n```",
        "srand" => "Seed the pseudo-random number generator used by `rand`. Calling `srand` with a specific value produces a reproducible sequence, which is useful for testing. Without arguments, Perl seeds from a platform-specific entropy source. You rarely need to call this explicitly — Perl auto-seeds on first use of `rand`.\n\n```perl\nsrand(42);                   # reproducible sequence\np int(rand(100));            # always the same value\nsrand(42)\np int(rand(100));            # same value again\nsrand();                     # re-seed from entropy\n```",
        "int" => "Truncate a floating-point number toward zero, discarding the fractional part. This is not rounding — `int(1.9)` is `1` and `int(-1.9)` is `-1`. Use `sprintf(\"%.0f\", $n)` or `POSIX::round` for proper rounding. Commonly paired with `rand` to generate random integers.\n\n```perl\np int(3.7);       # 3\np int(-3.7);      # -3\np int(rand(6)) + 1;  # dice roll 1..6\n1..10 |> map { $_ / 3 } |> map int |> e p\n```",
        "abs" => "Return the absolute value of a number, stripping any negative sign. Returns the argument unchanged if it is already non-negative. Works on both integers and floating-point numbers.\n\n```perl\np abs(-42);       # 42\np abs(3.14);      # 3.14\nmy $diff = abs($a - $b)\np \"distance: $diff\"\n1..5 |> map { $_ - 3 } |> map abs |> e p;  # 2 1 0 1 2\n```",
        "sqrt" => "Return the square root of a non-negative number. Dies if the argument is negative — use `abs` first or check the sign. For the inverse operation, use `squared`/`sq` or the `**` operator.\n\n```perl\np sqrt(144);         # 12\np sqrt(2);           # 1.41421356...\nmy $hyp = sqrt($a**2 + $b**2);  # Pythagorean theorem\n1..5 |> map sqrt |> e { p sprintf(\"%.3f\", $_) }\n```",
        "squared" | "sq" | "square" => "Return the square of a number (`N * N`). This is a perlrs convenience function — clearer than writing `$n ** 2` or `$n * $n` in pipelines. The aliases `sq` and `square` are interchangeable.\n\n```perl\np squared(5);        # 25\np sq(12);            # 144\n1..5 |> map sq |> e p;    # 1 4 9 16 25\nmy $hyp = sqrt(sq($a) + sq($b));  # Pythagorean theorem\n```",
        "cubed" | "cb" | "cube" => "Return the cube of a number (`N * N * N`). This is a perlrs convenience function for the common `$n ** 3` operation, useful in math-heavy pipelines. The aliases `cb` and `cube` are interchangeable.\n\n```perl\np cubed(3);          # 27\np cb(10);            # 1000\n1..4 |> map cb |> e p;  # 1 8 27 64\nmy $vol = cb($side);             # volume of a cube\n```",
        "expt" | "pow" | "pw" => "Raise a base to an arbitrary exponent and return the result. This is the function form of the `**` operator. Accepts integer and floating-point exponents, including negative values for reciprocals and fractional values for roots.\n\n```perl\np expt(2, 10);       # 1024\np expt(27, 1/3);     # 3.0 (cube root)\np expt(10, -2);      # 0.01\n1..8 |> map { expt(2, $_) } |> e p;  # 2 4 8 16 32 64 128 256\n```",
        "exp" => "Return Euler's number *e* raised to the given power. `exp(0)` is `1`, `exp(1)` is approximately `2.71828`. This is the inverse of `log`. Useful for exponential growth/decay calculations, probability distributions, and converting between logarithmic and linear scales.\n\n```perl\np exp(1);            # 2.71828182845905\np exp(0);            # 1\nmy $growth = $initial * exp($rate * $time)\n1..5 |> map exp |> e { p sprintf(\"%.4f\", $_) }\n```",
        "log" => "Return the natural (base-*e*) logarithm of a positive number. Dies if the argument is zero or negative. For base-10 logarithms, divide by `log(10)`. For base-2, divide by `log(2)`. This is the inverse of `exp`.\n\n```perl\np log(exp(1));       # 1.0\np log(100) / log(10);  # 2.0 (log base 10)\nmy $bits = log($n) / log(2);  # log base 2\n1..5 |> map log |> e { p sprintf(\"%.3f\", $_) }\n```",
        "sin" => "Return the sine of an angle given in radians. The result ranges from `-1` to `1`. For degrees, convert first: `sin($deg * 3.14159265 / 180)`. Use `atan2` to go in the reverse direction.\n\n```perl\np sin(0);               # 0\np sin(3.14159265 / 2);  # 1.0\nmy @wave = map { sin($_ * 0.1) } 0..62\n@wave |> e { p sprintf(\"%6.3f\", $_) }\n```",
        "cos" => "Return the cosine of an angle given in radians. The result ranges from `-1` to `1`. `cos(0)` is `1`. Like `sin`, convert degrees to radians before calling.\n\n```perl\np cos(0);                 # 1\np cos(3.14159265);        # -1.0\nmy $x = $radius * cos($theta)\nmy $y = $radius * sin($theta)\np \"($x, $y)\"\n```",
        "atan2" => "Return the arctangent of `Y/X` in radians, using the signs of both arguments to determine the correct quadrant. The result ranges from `-pi` to `pi`. This is the standard way to compute angles from Cartesian coordinates and is more robust than `atan(Y/X)` because it handles `X=0` correctly.\n\n```perl\nmy $pi = atan2(0, -1);       # 3.14159265...\np atan2(1, 1);               # 0.785... (pi/4)\nmy $angle = atan2($dy, $dx)\np sprintf(\"%.1f degrees\", $angle * 180 / $pi)\n```",
        "formline" => "Format a line of output according to a picture template, appending the result to the `$^A` (format accumulator) variable. This is the low-level engine behind Perl's `format`/`write` report-generation system. Template characters like `@<<<` (left-justify), `@>>>` (right-justify), and `@###.##` (numeric) control field placement.\n\n```perl\n$^A = \"\"\nformline(\"@<<<< @>>>>>\\n\", \"Name\", \"Score\")\nformline(\"@<<<< @>>>>>\\n\", \"Alice\", 98)\nformline(\"@<<<< @>>>>>\\n\", \"Bob\", 85)\np $^A\n```",
        "not" => "Low-precedence logical negation — returns true if the expression is false, and false if it is true. Functionally identical to `!` but binds looser than almost everything, so `not $a == $b` is `not($a == $b)` rather than `(!$a) == $b`. Useful for readable boolean conditions.\n\n```perl\nif (not defined $val) {\n    p \"val is undef\"\n}\nmy @missing = grep { not -e $_ } @files\n@missing |> e p\np not 0;    # 1\np not 1;    # (empty string)\n```",
        "syscall" => "Invoke a raw system call by its numeric identifier, passing arguments directly to the kernel. This is an escape hatch for system calls that have no Perl wrapper. The call number is platform-specific and the arguments must be correctly typed (integers or string buffers). Use with caution — incorrect arguments can crash the process.\n\n```perl\n# SYS_getpid on Linux x86_64 is 39\nmy $pid = syscall(39)\np $pid;                     # same as $$\n# SYS_sync on Linux is 162\nsyscall(162);               # flush filesystem caches\n```",

        // ── perlrs extensions (syntax / macros) ──
        "thread" | "t" => "Clojure-inspired threading macro — chain stages without repeating `|>`.\n\n```perl\nthread @data grep { $_ > 5 } map { $_ * 2 } sort { $_0 <=> $_1 } |> join \",\" |> p\nt \" hello \" tm uc rv lc ufc sc cc kc tj p;  # short aliases\nsub add2 { $_0 + $_1 }\nt 10 add2($_, 5) p;                          # add2(10, 5) = 15\nt 10 add2(5, $_) p;                          # add2(5, 10) = 15  (any position)\nt 10 add2($_, 5) add2($_, 100) p;            # 115 (chained)\n```\n\nStages: bare function (`uc`, `tm`, …), function with block (`map { … }`, `grep { … }`), `name(args)` call where `$_` is the threaded-value placeholder (must appear at least once in args), or `>{}` anonymous block.\n`|>` terminates the thread macro.",
        "fn" => "Alias for `sub` — define a function.\n\n```perl\nfn double($x) { $x * 2 }\nmy $f = fn { $_ * 2 }\nmy $add = fn ($a: Int, $b: Int) { $a + $b }\n```",
        "mysync" => "Declare shared variables for parallel blocks (`Arc<Mutex>`).\n\n```perl\nmysync $counter = 0\nfan 10000 { $counter++ }   # always exactly 10000\nmysync @results\nmysync %histogram$1\n\nCompound ops (`++`, `+=`, `.=`, `|=`, `&=`) are fully atomic.",
        "frozen" | "const" => "Declare an immutable lexical variable. `const my` and `frozen my` are interchangeable spellings; `const` reads more naturally for engineers coming from other languages.\n\n```perl\nconst my $pi = 3.14159\n# $pi = 3  # ERROR: cannot assign to frozen variable\n\nfrozen my @primes = (2, 3, 5, 7, 11)\n```",
        "match" => "Algebraic pattern matching (perlrs extension).\n\n```perl\nmatch ($val) {\n    /^\\d+$/ => p \"number: $val\",\n    [1, 2, _] => p \"array starting with 1,2\",\n    { name => $n } => p \"name is $n\",\n    _ => p \"default\",\n}\n```\n\nPatterns: regex, array, hash, literal, wildcard `_`. Optional `if` guard per arm.",
        "|>" => "Pipe-forward operator — threads LHS as first argument of RHS call.\n\n```perl\n\"hello\" |> uc |> rev |> p;              # OLLEH\n1..10 |> grep $_ > 5 |> map $_ * 2 |> e p\n$url |> fetch_json |> json_jq '.name' |> p\n\"hello world\" |> s/world/perl/ |> p;     # hello perl\n```\n\nZero runtime cost (parse-time desugaring). Binds looser than `||`, tighter than `?:`.",
        "pipe" | "CORE::pipe" => "Create a unidirectional pipe, returning a pair of connected filehandles: one for reading and one for writing. Data written to the write end can be read from the read end, making pipes the fundamental building block for inter-process communication. Commonly used with `fork` so the parent and child can exchange data.\n\n```perl\npipe(my $rd, my $wr) or die \"pipe: $!\"\nif (fork() == 0) {\n    close($rd)\n    print $wr \"hello from child\\n\"\n    exit(0)\n}\nclose($wr)\nmy $msg = <$rd>\np $msg;   # hello from child\n```",
        "gen" => "Create a generator — lazy `yield` values on demand.\n\n```perl\nmy $g = gen { yield $_ for 1..5 }\nmy ($val, $more) = @{$g->next}\n```",
        "yield" => "Yield a value from inside a `gen { }` generator block, suspending the generator until the consumer calls `->next` again. Each `yield` produces one element in the lazy sequence. When the block finishes without yielding, the generator signals exhaustion. This is the perlrs equivalent of Python's `yield` or Rust's `Iterator::next`.\n\n```perl\nmy $fib = gen {\n    my ($a, $b) = (0, 1)\n    while (1) {\n        yield $a\n        ($a, $b) = ($b, $a + $b)\n    }\n}\nfor (1..10) {\n    my ($val) = @{$fib->next}\n    p $val\n}\n```",
        "trace" => "Trace `mysync` mutations to stderr (tagged with worker index under `fan`).\n\n```perl\ntrace { fan 10 { $counter++ } }\n```",
        "timer" => "Measure wall-clock milliseconds for a block.\n\n```perl\nmy $ms = timer { heavy_work() }\n```",
        "bench" => "Benchmark a block N times; returns `\"min/mean/p99\"`.\n\n```perl\nmy $report = bench { work() } 1000\n```",
        "eval_timeout" => "Run a block with a wall-clock timeout (seconds).\n\n```perl\neval_timeout 5 { slow_operation() }\n```",
        "retry" => "Retry a block on failure.\n\n```perl\nretry { http_call() } times => 3, backoff => 'exponential'\n```",
        "rate_limit" => "Limit invocations per time window.\n\n```perl\nrate_limit(10, \"1s\") { hit_api() }\n```",
        "every" => "Run a block at a fixed interval.\n\n```perl\nevery \"500ms\" { tick() }\n```",
        "fore" | "e" => "Side-effect-only list iterator (like `map` but void, returns item count).\n\n```perl\nqw(a b c) |> e p;           # prints a, b, c; returns 3\n1..5 |> map $_ * 2 |> e p;  # prints 2,4,6,8,10\n```",
        "ep" => "`ep` — shorthand for `e { p }` (foreach + print). Iterates the list and prints each element.\n\n```perl\nqw(a b c) |> ep;            # prints a, b, c (one per line)\nfilef |> sorted |> ep;      # print sorted file list\n1..5 |> map $_ * 2 |> ep;   # prints 2,4,6,8,10\n```",
        "p" => "`p` — alias for `say` (print with newline).\n\n```perl\np \"hello\";       # hello\\n\np 42;            # 42\\n\n1..5 |> e p;     # prints each on its own line\n```",
        "watch" => "Watch a single file for changes (non-parallel).\n\n```perl\nwatch \"/tmp/x\", sub { process }\n```",
        "glob_par" => "Perform a parallel recursive file-system glob, using multiple threads to walk directory trees concurrently. This is significantly faster than `glob` for deep directory hierarchies with thousands of files. Accepts the same glob syntax (`*`, `**`, `{a,b}`) but returns results as they are discovered across threads. Ideal for large codebases or log directories.\n\n```perl\nmy @logs = glob_par(\"**/*.log\")\np scalar @logs;               # count of log files\n\"**/*.rs\" |> glob_par |> e p;  # print all Rust files\nmy @imgs = glob_par(\"assets/**/*.{png,jpg,webp}\")\n@imgs |> e { p bn($_) }\n```",
        "par_find_files" => "Recursively search a directory tree in parallel for files matching a glob pattern. Unlike `glob_par` which takes a single pattern string, `par_find_files` separates the root directory from the pattern, making it convenient when the search root is a variable. Returns a list of absolute paths to matching files.\n\n```perl\nmy @src = par_find_files(\"src\", \"*.rs\")\np scalar @src;                    # count of Rust files under src/\nmy @tests = par_find_files(\".\", \"*_test.pl\")\n@tests |> e p\nmy @configs = par_find_files(\"/etc\", \"*.conf\")\n```",
        "par_line_count" => "Count lines across multiple files in parallel, returning the total line count. Each file is read and counted by a separate thread, making this dramatically faster than sequential `wc -l` for large file sets. Useful for codebase metrics, log analysis, or validating data pipeline output.\n\n```perl\nmy @files = glob(\"src/**/*.rs\")\nmy $total = par_line_count(@files)\np \"$total lines of Rust\"\nmy $logs = par_line_count(glob(\"/var/log/*.log\"))\np \"$logs log lines\"\n```",
        "capture" => "Run a command and capture structured output.\n\n```perl\nmy $r = capture(\"ls -la\")\np $r->stdout, $r->stderr, $r->exit\n```",

        "pager" | "pg" | "less" => "`LIST |> pager` / `pager LIST` — pipe each element (one per line) into the user's `$PAGER` (default `less -R`; falls back to `more`, then plain stdout). Bypasses the pager when stdout isn't a TTY so pipelines like `pe -e '... |> pager' | grep` still compose.\n\nBlocks until the user quits the pager; returns `undef`.\n\nAliases: `pager`, `pg`, `less`.\n\n```perl\n# browse every callable spelling interactively\nkeys %all |> sort |> pager\n\n# filter the reference for parallel ops\nkeys %b |> grep { $b{$_} eq \"parallel\" } |> pager\n\n# whole file, one screen at a time\nslurp(\"README.md\") |> pager\n```",
        "input" => "Slurp all of stdin (or a filehandle) as one string.\n\n```perl\nmy $all = input;          # slurp stdin\nmy $fh_data = input($fh); # slurp filehandle\n```",
        "slurp" | "sl" => "Read an entire file into memory as a single UTF-8 string. The short alias `sl` is convenient in pipelines. Dies if the file does not exist or cannot be read. This is the complement of `spurt`/`wf` — together they form a simple read/write pair for whole-file operations. For binary data, use `read_bytes`/`slurp_raw` instead.\n\n```perl\nmy $text = slurp(\"config.yaml\")\np $text\nmy $json = sl(\"data.json\")\nmy $data = decode_json($json)\n\"README.md\" |> sl |> length |> p;  # character count\n```",

        // ── Language reflection (populated at interpreter init from `build.rs` tables) ──
        "perlrs::builtins" => "`%perlrs::builtins` (short: `%b`) — every **primary** callable name → its category. Primaries-only, so `scalar keys %b` is a clean unique-operation count. For the \"everything you can type\" view (primaries + aliases), use `%perlrs::all` / `%all`.\n\n```perl\np $b{pmap};               # \"parallel\"\np $b{to_json};            # \"serialization\"\np scalar keys %b;         # unique-op count\n```",

        "perlrs::all" => "`%perlrs::all` (short: `%all`) — every callable *spelling* (primaries **and** aliases) → category. Aliases inherit their primary's category.\n\nUse `%all` when you want \"how many names can I type?\" or want to look up an alias's category without hopping through `%aliases`. Use `%builtins` when you want unique operations.\n\n```perl\np scalar keys %all;   # total callable-spellings count\np $all{tj};           # \"serialization\"  (alias resolves via inheritance)\np $all{to_json};      # \"serialization\"\nkeys %all |> pager;   # browse every spelling\n```",

        "perlrs::perl_compats" => "`%perlrs::perl_compats` (short: `%pc`) — Perl 5 core names only, name → category.\n\nSubset of `%builtins` restricted to names from `is_perl5_core`. Direct O(1) access for the \"show me just Perl core\" query.\n\n```perl\np $pc{map};                    # \"array / list\"\np scalar keys %pc;             # core-only count\nkeys %pc |> sort |> p;         # enumerate every Perl core name\n```",

        "perlrs::extensions" => "`%perlrs::extensions` (short: `%e`) — perlrs-only names, name → category.\n\nDisjoint from `%perl_compats`. Everything `--compat` mode rejects at parse time, plus dispatch primaries like `basename`/`ddump` that are extensions at runtime even without a parser entry.\n\n```perl\np $e{pmap};                                # \"parallel\"\nkeys %e |> grep /^p/ |> sort |> p;         # every p* parallel op\n```",

        "perlrs::aliases" => "`%perlrs::aliases` (short: `%a`) — alias spelling → canonical primary.\n\nKeys are the 2nd-and-later names in each `try_builtin` match arm. For O(1) *reverse* lookup (primary → all its aliases), use `%perlrs::primaries` / `%p`.\n\n```perl\np $a{tj};                                  # \"to_json\"\np $a{bn};                                  # \"basename\"\np scalar keys %a;                          # total alias count\n```",

        "perlrs::descriptions" => "`%perlrs::descriptions` (short: `%d`) — name → one-line summary.\n\nFirst sentence of each LSP hover doc (`doc_for_label_text`), harvested at build time. Sparse — only names that have a hover doc appear, so `exists $d{$name}` doubles as \"is this documented?\".\n\n```perl\np $d{pmap};                                # one-line summary\np $d{to_json};                             # \"Serialize a PerlValue to a JSON string.\"\np scalar keys %d;                          # count of documented ops\nkeys %d |> grep { $d{$_} =~ /parallel/i } |> sort |> p\n```",

        "perlrs::categories" => "`%perlrs::categories` (short: `%c`) — category string → arrayref of names in that category.\n\nInverted index on `%builtins`. Gives O(1) reverse-lookup for \"list every op of kind X\" queries that would otherwise be O(n) `grep`s. Name lists are alphabetized.\n\n```perl\n$c{parallel} |> e p;                  # every parallel op\np scalar @{ $c{parallel} };           # how many?\np join \", \", @{ $c{\"array / list\"} }; # joined roster\nkeys %c |> sort |> p;                 # all category names\n```",

        "perlrs::primaries" => "`%perlrs::primaries` (short: `%p`) — primary dispatcher name → arrayref of its aliases.\n\nInverted `%aliases`. Primaries with no aliases still have an entry (empty arrayref), so `exists $p{foo}` reliably answers \"is foo a dispatch primary?\" O(1).\n\n```perl\n$p{to_json} |> e p;              # [\"tj\"]\np scalar @{ $p{basename} };      # how many aliases does basename have?\n# find every primary that has at least one alias:\nkeys %p |> grep { scalar @{$p{$_}} } |> sort |> p\n```",

        // ── Higher-order function wrappers ──
        "compose" | "comp" => "`compose` (alias `comp`) creates a right-to-left function composition. Given `compose(\\&f, \\&g)`, calling the result with `x` computes `f(g(x))`. Chain any number of functions — they apply from right to left (last argument first). This is the standard mathematical function composition found in Haskell, Clojure, and Ramda. The returned code ref can be stored, passed around, or used in pipelines.\n\n```perl\nmy $double = sub { $_[0] * 2 }\nmy $inc    = sub { $_[0] + 1 }\nmy $f = compose($inc, $double)\np $f->(5);   # 11  (double 5 → 10, inc 10 → 11)\n\nmy $pipeline = compose(\n    sub { join \",\", @{$_[0]} },\n    sub { [sort @{$_[0]}] },\n    sub { [grep { $_ > 2 } @{$_[0]}] },\n)\np $pipeline->([3,1,4,1,5]);  # 3,4,5\n```",
        "partial" => "`partial` returns a partially applied function — the bound arguments are prepended to any arguments supplied at call time. `partial(\\&f, @bound)->(x)` is equivalent to `f(@bound, x)`. This is the standard partial application from functional programming, useful for creating specialized versions of general functions without closures.\n\n```perl\nmy $add = sub { $_[0] + $_[1] }\nmy $add5 = partial($add, 5)\np $add5->(3);   # 8\n\nmy $log = sub { say \"[$_[0]] $_[1]\" }\nmy $warn_log = partial($log, \"WARN\")\n$warn_log->(\"disk full\");   # [WARN] disk full\n```",
        "curry" => "`curry` auto-curries a function with a given arity. The curried function accumulates arguments across calls and invokes the original only when enough have been collected. `curry(\\&f, N)->(a)->(b)` calls `f(a, b)` when N=2. If all arguments are supplied at once, it calls immediately.\n\n```perl\nmy $add = curry(sub { $_[0] + $_[1] }, 2)\nmy $add5 = $add->(5)\np $add5->(3);       # 8\np $add->(10, 20);   # 30  (enough args — calls immediately)\n```",
        "memoize" | "memo" => "`memoize` (alias `memo`) wraps a function so that repeated calls with the same arguments return a cached result instead of re-executing the function. Arguments are stringified and joined as the cache key. This is essential for expensive pure functions like recursive algorithms, API lookups with stable results, or any computation where the same inputs always produce the same output.\n\n```perl\nmy $fib = memoize(sub {\n    my $n = $_[0]\n    $n < 2 ? $n : $fib->($n-1) + $fib->($n-2)\n})\np $fib->(30);   # instant (without memoize: ~1B calls)\n\nmy $fetch_user = memo(sub { fetch_json(\"https://api/users/$_[0]\") })\n$fetch_user->(42);   # hits API\n$fetch_user->(42);   # returns cached\n```",
        "once" => "`once` wraps a function so it is called at most once. The first invocation executes the function and caches the result; all subsequent calls return the cached value without re-executing. This is ideal for lazy initialization, one-time setup, or singleton patterns.\n\n```perl\nmy $init = once(sub { say \"initializing...\"; 42 })\np $init->();   # prints \"initializing...\" → 42\np $init->();   # 42 (no print — cached)\np $init->();   # 42 (still cached)\n```",
        "constantly" => "`constantly` (alias `const`) returns a function that ignores all arguments and always returns the given value. Useful as a default callback, a stub in higher-order function pipelines, or anywhere a function is required but a fixed value suffices.\n\n```perl\nmy $zero = constantly(0)\np $zero->(\"anything\");   # 0\nmy @defaults = map { constantly(0)->() } 1..5;   # [0,0,0,0,0]\n```",
        "complement" | "compl" => "`complement` (alias `compl`) wraps a predicate function and returns a new function that negates its boolean result. `complement(\\&even?)->(3)` returns true. This is the functional equivalent of `!f(x)` without creating a closure.\n\n```perl\nmy $even = sub { $_[0] % 2 == 0 }\nmy $odd = complement($even)\np $odd->(3);   # 1\np $odd->(4);   # 0\n1..10 |> grep { complement($even)->($_) } |> e p;  # 1 3 5 7 9\n```",
        "juxt" => "`juxt` (juxtapose) takes multiple functions and returns a new function that calls each one with the same arguments and collects the results into an array. This is useful for computing multiple derived values from the same input in a single pass.\n\n```perl\nmy $stats = juxt(sub { min @_ }, sub { max @_ }, sub { avg @_ })\nmy @r = $stats->(3, 1, 4, 1, 5)\np \"@r\";   # 1 5 2.8\n```",
        "fnil" => "`fnil` wraps a function so that any `undef` arguments are replaced with the given defaults before the function is called. This eliminates repetitive `// $default` patterns inside function bodies.\n\n```perl\nmy $greet = fnil(sub { \"Hello, $_[0]!\" }, \"World\")\np $greet->(undef);    # Hello, World!\np $greet->(\"Alice\");  # Hello, Alice!\n```",

        // ── Deep structure utilities ──
        "deep_clone" | "dclone" => "`deep_clone` (alias `dclone`) performs a recursive deep copy of a nested data structure. Array refs, hash refs, and scalar refs are cloned recursively so that the result shares no references with the original. Modifications to the clone never affect the source. This is the perlrs equivalent of JavaScript's `structuredClone` or Perl's `Storable::dclone`.\n\n```perl\nmy $orig = {users => [{name => \"Alice\"}], meta => {v => 1}}\nmy $copy = deep_clone($orig)\n$copy->{users}[0]{name} = \"Bob\"\np $orig->{users}[0]{name};   # Alice (unchanged)\n```",
        "deep_merge" | "dmerge" => "`deep_merge` (alias `dmerge`) recursively merges two hash references. When both sides have a hash ref for the same key, they are merged recursively; otherwise the right-hand value wins. This is the standard deep merge from Lodash, Ruby's `deep_merge`, and config-file overlay patterns. Returns a new hash ref — neither input is modified.\n\n```perl\nmy $defaults = {db => {host => \"localhost\", port => 5432}, debug => 0}\nmy $overrides = {db => {port => 3306}, debug => 1}\nmy $cfg = deep_merge($defaults, $overrides)\np $cfg->{db}{host};   # localhost (from defaults)\np $cfg->{db}{port};   # 3306 (overridden)\np $cfg->{debug};      # 1 (overridden)\n```",
        "deep_equal" | "deq" => "`deep_equal` (alias `deq`) performs structural equality comparison of two values, recursively descending into array refs, hash refs, and scalar refs. Returns 1 if the structures are identical, 0 otherwise. This is the perlrs equivalent of Node's `assert.deepStrictEqual`, Lodash `isEqual`, or Python's `==` on nested dicts/lists.\n\n```perl\np deep_equal([1, {a => 2}], [1, {a => 2}]);   # 1\np deep_equal([1, {a => 2}], [1, {a => 3}]);   # 0\np deq({x => [1,2]}, {x => [1,2]});            # 1\n```",
        "tally" => "`tally` counts how many times each distinct element appears in a list and returns a hash ref mapping element → count. This is the same as Ruby's `Enumerable#tally` or Python's `Counter`. Similar to `frequencies` but follows the Ruby naming convention.\n\n```perl\nmy $t = tally(\"a\", \"b\", \"a\", \"c\", \"a\", \"b\")\np $t->{a};   # 3\np $t->{b};   # 2\nqw(red blue red green blue red) |> tally |> dd\n```",

        _ => return None,
    };
    Some(md)
}

/// Auto-generate a stub doc from the reflection `CATEGORY_MAP` for any
/// builtin that doesn't have a hand-written entry in `doc_for_label_text`.
/// Returns `"name — category builtin."` so every name has hover text.
fn doc_stub_for(label: &str) -> Option<String> {
    // Check if alias → resolve to primary for better stub
    let primary = crate::builtins::BUILTIN_ARMS.iter().find_map(|arm| {
        if arm.contains(&label) {
            arm.first().copied()
        } else {
            None
        }
    });
    let canonical = primary.unwrap_or(label);

    for &(name, category) in crate::builtins::CATEGORY_MAP {
        if name == canonical {
            let alias_note = if canonical != label {
                format!(" Alias for `{}`.", canonical)
            } else {
                String::new()
            };
            return Some(format!(
                "`{}` — {} builtin.{}\n\n```perl\n{}\n```",
                label,
                category,
                alias_note,
                if label.contains("_to_") || label.contains("to_") {
                    format!("my $result = {} $input ", label)
                } else {
                    format!(
                        "my $result = {0} $x \n# or in a pipeline:\n@list |> map {0} |> p",
                        label
                    )
                }
            ));
        }
    }
    None
}

// Thread-local cache for auto-generated stub docs so they have 'static lifetime.
thread_local! {
    static STUB_CACHE: std::cell::RefCell<std::collections::HashMap<String, &'static str>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Public entry point for `pe docs TOPIC` — returns raw markdown doc text.
/// Checks hand-written docs first, then falls back to auto-generated stubs
/// from the reflection `CATEGORY_MAP` so every builtin has at least a
/// one-liner hover.
pub fn doc_text_for(label: &str) -> Option<&'static str> {
    // Hand-written entry takes priority.
    if let Some(md) = doc_for_label_text(label) {
        return Some(md);
    }
    // Auto-generated stub, cached with 'static lifetime via leak.
    STUB_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if let Some(&cached) = map.get(label) {
            return Some(cached);
        }
        let stub = doc_stub_for(label)?;
        let leaked: &'static str = Box::leak(stub.into_boxed_str());
        map.insert(label.to_string(), leaked);
        Some(leaked)
    })
}

/// List all documented topic names (sorted, deduplicated).
/// Now includes auto-stubbed names from `CATEGORY_MAP` in addition to
/// the hand-written completion words.
pub fn doc_topics() -> Vec<&'static str> {
    let mut topics: Vec<&'static str> = include_str!("lsp_completion_words.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|l| doc_text_for(l).is_some())
        .collect();
    // Add every CATEGORY_MAP name that has a stub but isn't in the
    // completion-words file (the bulk of the ~1700 builtins).
    let existing: std::collections::HashSet<&str> = topics.iter().copied().collect();
    for &(name, _) in crate::builtins::CATEGORY_MAP {
        if !existing.contains(name) && doc_text_for(name).is_some() {
            topics.push(name);
        }
    }
    topics.sort();
    topics.dedup();
    topics
}

/// Grouped category list for the `pe docs` book view and the static-site
/// `docs/reference.html` generator (`cargo run --bin gen-docs`). Each tuple
/// is (chapter name, topic names); topics must match `doc_for_label_text`
/// keys or the generator will skip them.
pub const DOC_CATEGORIES: &[(&str, &[&str])] = &[
    (
        "Parallel Primitives",
        &[
            "pmap",
            "pmap_chunked",
            "pgrep",
            "pfor",
            "psort",
            "pcache",
            "preduce",
            "preduce_init",
            "pmap_reduce",
            "pany",
            "pfirst",
            "puniq",
            "pflat_map",
            "fan",
            "fan_cap",
        ],
    ),
    (
        "Shared State & Concurrency",
        &[
            "mysync", "async", "spawn", "await", "pchannel", "pselect", "barrier", "ppool",
            "deque", "heap", "set",
        ],
    ),
    (
        "Pipeline & Pipe-Forward",
        &[
            "|>",
            "thread",
            "t",
            "pipeline",
            "par_pipeline",
            "par_pipeline_stream",
            "collect",
        ],
    ),
    (
        "Streaming Iterators",
        &[
            "maps",
            "greps",
            "filter",
            "tap",
            "peek",
            "tee",
            "take",
            "head",
            "tail",
            "drop",
            "take_while",
            "drop_while",
            "reject",
            "compact",
            "concat",
            "enumerate",
            "chunk",
            "dedup",
            "distinct",
            "flatten",
            "with_index",
            "first_or",
            "range",
            "stdin",
            "nth",
        ],
    ),
    (
        "List Operations",
        &[
            "map",
            "grep",
            "sort",
            "reverse",
            "reduce",
            "fold",
            "reductions",
            "all",
            "any",
            "none",
            "first",
            "min",
            "max",
            "sum",
            "sum0",
            "product",
            "mean",
            "median",
            "mode",
            "stddev",
            "variance",
            "sample",
            "shuffle",
            "uniq",
            "uniqint",
            "uniqnum",
            "uniqstr",
            "zip",
            "zip_longest",
            "zip_shortest",
            "chunked",
            "windowed",
            "pairs",
            "unpairs",
            "pairkeys",
            "pairvalues",
            "pairmap",
            "pairgrep",
            "pairfirst",
            "mesh",
            "mesh_longest",
            "mesh_shortest",
            "partition",
            "frequencies",
            "tally",
            "interleave",
            "pluck",
            "grep_v",
            "select_keys",
            "clamp",
            "normalize",
            "compose",
            "partial",
            "curry",
            "memoize",
            "once",
            "constantly",
            "complement",
            "juxt",
            "fnil",
            "deep_clone",
            "deep_merge",
            "deep_equal",
        ],
    ),
    (
        "perlrs Extensions",
        &[
            "fn",
            "struct",
            "typed",
            "match",
            "frozen",
            "fore",
            "e",
            "ep",
            "p",
            "gen",
            "yield",
            "trace",
            "timer",
            "bench",
            "eval_timeout",
            "retry",
            "rate_limit",
            "every",
            "watch",
            "capture",
        ],
    ),
    (
        "Data & Serialization",
        &[
            "json_encode",
            "json_decode",
            "json_jq",
            "to_json",
            "to_csv",
            "to_toml",
            "to_yaml",
            "to_xml",
            "to_html",
            "to_markdown",
            "csv_read",
            "csv_write",
            "dataframe",
            "sqlite",
            "stringify",
            "ddump",
            "toml_decode",
            "toml_encode",
            "yaml_decode",
            "yaml_encode",
            "xml_decode",
            "xml_encode",
        ],
    ),
    (
        "HTTP & Networking",
        &[
            "fetch",
            "fetch_json",
            "fetch_async",
            "fetch_async_json",
            "http_request",
            "par_fetch",
            "serve",
            "socket",
            "bind",
            "listen",
            "accept",
            "connect",
            "send",
            "recv",
            "shutdown",
            "setsockopt",
            "getsockopt",
            "getsockname",
            "getpeername",
            "gethostbyname",
            "gethostbyaddr",
            "getprotobyname",
            "getservbyname",
        ],
    ),
    (
        "Crypto & Encoding",
        &[
            "sha256",
            "sha224",
            "sha384",
            "sha512",
            "sha1",
            "crc32",
            "hmac_sha256",
            "hmac",
            "base64_encode",
            "base64_decode",
            "hex_encode",
            "hex_decode",
            "uuid",
            "jwt_encode",
            "jwt_decode",
            "jwt_decode_unsafe",
            "url_encode",
            "url_decode",
            "uri_escape",
            "uri_unescape",
            "gzip",
            "gunzip",
            "zstd",
            "zstd_decode",
        ],
    ),
    (
        "Parallel I/O",
        &[
            "par_lines",
            "par_walk",
            "par_sed",
            "par_find_files",
            "par_line_count",
            "par_csv_read",
            "glob_par",
            "pwatch",
        ],
    ),
    (
        "File I/O",
        &[
            "open",
            "close",
            "read",
            "readline",
            "eof",
            "seek",
            "tell",
            "print",
            "say",
            "printf",
            "sprintf",
            "slurp",
            "slurp_raw",
            "read_bytes",
            "input",
            "read_lines",
            "append_file",
            "to_file",
            "write",
            "write_file",
            "spurt",
            "write_json",
            "read_json",
            "tempfile",
            "tempdir",
            "binmode",
            "fileno",
            "flock",
            "getc",
            "select",
            "truncate",
            "sysopen",
            "sysread",
            "syswrite",
            "sysseek",
            "format",
            "formline",
        ],
    ),
    (
        "Strings",
        &[
            "chomp",
            "chop",
            "length",
            "substr",
            "index",
            "rindex",
            "split",
            "join",
            "uc",
            "lc",
            "ucfirst",
            "lcfirst",
            "chr",
            "ord",
            "hex",
            "oct",
            "quotemeta",
            "reverse",
            "trim",
            "lines",
            "words",
            "chars",
            "snake_case",
            "camel_case",
            "kebab_case",
            "study",
            "pos",
        ],
    ),
    (
        "Arrays & Hashes",
        &[
            "push",
            "pop",
            "shift",
            "unshift",
            "splice",
            "keys",
            "values",
            "each",
            "delete",
            "exists",
            "scalar",
            "defined",
            "undef",
            "ref",
            "bless",
            "tie",
            "prototype",
            "wantarray",
            "caller",
        ],
    ),
    (
        "Control Flow",
        &[
            "if", "elsif", "else", "unless", "for", "foreach", "while", "until", "do", "last",
            "next", "redo", "continue", "given", "when", "default", "return", "not",
        ],
    ),
    (
        "Error Handling",
        &[
            "try", "catch", "finally", "eval", "die", "warn", "croak", "confess",
        ],
    ),
    (
        "Declarations",
        &[
            "my", "our", "local", "state", "sub", "package", "use", "no", "require", "BEGIN", "END",
        ],
    ),
    (
        "Cluster / Distributed",
        &["cluster", "pmap_on", "pflat_map_on", "ssh"],
    ),
    (
        "Datetime",
        &[
            "datetime_utc",
            "datetime_from_epoch",
            "datetime_strftime",
            "datetime_now_tz",
            "datetime_format_tz",
            "datetime_parse_local",
            "datetime_parse_rfc3339",
            "datetime_add_seconds",
            "elapsed",
            "time",
            "times",
            "localtime",
            "gmtime",
            "sleep",
            "alarm",
        ],
    ),
    (
        "Math",
        &[
            "abs", "int", "sqrt", "squared", "cubed", "expt", "exp", "log", "sin", "cos", "atan2",
            "rand", "srand",
        ],
    ),
    (
        "File System",
        &[
            "basename",
            "dirname",
            "fileparse",
            "realpath",
            "canonpath",
            "getcwd",
            "which",
            "glob",
            "glob_match",
            "copy",
            "move",
            "mv",
            "rename",
            "unlink",
            "mkdir",
            "rmdir",
            "chmod",
            "chown",
            "chdir",
            "stat",
            "link",
            "symlink",
            "readlink",
            "utime",
            "umask",
            "uname",
            "gethostname",
            "opendir",
            "readdir",
            "closedir",
            "seekdir",
            "telldir",
            "rewinddir",
        ],
    ),
    (
        "Process",
        &[
            "system",
            "exec",
            "fork",
            "wait",
            "waitpid",
            "kill",
            "exit",
            "getlogin",
            "getpwnam",
            "getpwuid",
            "getpwent",
            "getgrgid",
            "getgrnam",
            "getgrent",
            "getppid",
            "getpgrp",
            "setpgrp",
            "getpriority",
            "setpriority",
            "syscall",
        ],
    ),
    ("Pack / Binary", &["pack", "unpack", "vec"]),
    (
        "Logging",
        &[
            "log_info",
            "log_warn",
            "log_error",
            "log_debug",
            "log_trace",
            "log_json",
            "log_level",
        ],
    ),
];

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
            "my \\$${1:task} = async {\n\t${0}\n}\nmy \\$${2:result} = await \\$${1:task};",
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
            "struct ${1:Name} {\n\t${2:field} => ${3|Int,Str,Float,Bool,Any|},\n}\n",
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

    #[test]
    fn new_functional_utility_docs_exist() {
        use super::doc_text_for;
        let ops = [
            "compose",
            "partial",
            "curry",
            "memoize",
            "once",
            "constantly",
            "complement",
            "juxt",
            "fnil",
            "deep_clone",
            "deep_merge",
            "deep_equal",
            "tally",
        ];
        for op in ops {
            assert!(doc_text_for(op).is_some(), "Doc for '{}' should exist", op);
        }
    }
}
