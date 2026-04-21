//! Language server protocol (stdio) for editors — `stryke --lsp`.

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
            "name": "stryke",
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
        format!("stryke LSP: unimplemented request {}", req.method),
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
        source: Some("stryke".to_string()),
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
        StmtKind::ClassDecl { def } => {
            symbols.push(sym(
                format!("class {}", def.name),
                SymbolKind::CLASS,
                uri,
                source,
                stmt.line,
                container,
            ));
        }
        StmtKind::TraitDecl { def } => {
            symbols.push(sym(
                format!("trait {}", def.name),
                SymbolKind::INTERFACE,
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
        | StmtKind::ClassDecl { .. }
        | StmtKind::TraitDecl { .. }
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
        "and", "async", "await", "catch", "class", "continue", "default", "do", "else", "elsif",
        "eval", "extends", "finally", "for", "foreach", "given", "if", "impl", "last", "local",
        "my", "next", "no", "not", "or", "our", "package", "priv", "pub", "redo", "return",
        "spawn", "state", "struct", "sub", "trait", "try", "typed", "unless", "until", "use",
        "when", "while",
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

/// Raw doc text lookup — single source of truth for both LSP hover and `stryke docs`.
fn doc_for_label_text(label: &str) -> Option<&'static str> {
    let key = label.strip_suffix(" …").unwrap_or(label);
    let md: &'static str = match key {
        // ── Declarations & keywords ──
        "my" => "Declare a lexically scoped variable visible only within the enclosing block or file. `my` is the workhorse of Perl variable declarations — use it for scalars (`$`), arrays (`@`), and hashes (`%`). Variables declared with `my` are invisible outside their scope, which prevents accidental cross-scope mutation. In stryke, `my` variables participate in pipe chains and can be destructured in list context. Uninitialized `my` variables default to `undef`.\n\n```perl\nmy $name = \"world\"\nmy @nums = 1..5\nmy %cfg = (debug => 1, verbose => 0)\np \"hello $name\"\n@nums |> grep $_ > 2 |> e p$1\n\nUse `my` everywhere unless you specifically need `our` (package global), `local` (dynamic scope), or `state` (persistent).",
        "our" => "Declare a package-global variable that is accessible as a lexical alias in the current scope. Unlike `my`, `our` variables are visible across the entire package and can be accessed from other packages via their fully qualified name (e.g. `$Counter::total`). This is useful for package-level configuration, shared counters, or variables that need to survive across file boundaries. In stryke, `our` variables are not mutex-protected — use `mysync` instead for parallel-safe globals.\n\n```perl\npackage Counter\nour $total = 0\nfn bump { $total++ }\npackage main\nCounter::bump() for 1..5\np $Counter::total   # 5\n```\n\nPrefer `my` for local state; reach for `our` only when other packages need access.",
        "local" => "Temporarily override a global variable's value for the duration of the current dynamic scope. When the scope exits, the original value is automatically restored. This is essential for modifying Perl's special variables (like `$/`, `$\\`, `$,`) without permanently altering global state. Unlike `my` which creates a new variable, `local` saves and restores an existing global. In stryke, `local` works with all special variables and respects the same restoration semantics during exception unwinding.\n\n```perl\nlocal $/ = undef       # slurp mode\nopen my $fh, '<', 'data.txt' or die $!\nmy $body = <$fh>       # reads entire file\nclose $fh\n# $/ restored to \"\\n\" when scope exits\n```\n\nCommon patterns: `local $/ = undef` (slurp), `local $, = \",\"` (join print args), `local %ENV` (temporary env).",
        "state" => "Declare a persistent lexical variable that retains its value across calls to the enclosing subroutine. Unlike `my`, which reinitializes on each call, `state` initializes only once — the first time execution reaches the declaration — and preserves the value for all subsequent calls. This is perfect for counters, caches, and memoization without resorting to globals or closures over external variables. In stryke, `state` variables are per-thread when used inside `~> { }` or `fan` blocks; they are not shared across workers.\n\n```perl\nfn counter {\n    state $n = 0\n    ++$n\n}\np counter() for 1..5   # 1 2 3 4 5\nfn memo($x) {\n    state %cache\n    $cache{$x} //= expensive($x)\n}\n```\n\nRequires `use feature 'state'` in standard Perl, but is always available in stryke.",
        "sub" => "Define a named or anonymous subroutine. In stryke, the preferred shorthand is `fn`, which behaves identically but is shorter and supports optional typed parameters. Named subs are installed into the current package; anonymous subs (closures) capture their enclosing lexical scope. Subroutines are first-class values — assign them to scalars, store in arrays, pass as callbacks. The last expression evaluated is the implicit return value unless an explicit `return` is used.\n\n```perl\nfn greet($who) { p \"hello $who\" }\nmy $sq = fn ($x) { $x ** 2 }\n1..5 |> map $sq->($_) |> e p\nfn apply($f, @args) { $f->(@args) }\napply($sq, 7) |> p   # 49\n```\n\nUse `fn` for new stryke code; `sub` is fully supported for Perl compatibility.",
        "package" => "Set the current package namespace for all subsequent declarations. Package names are conventionally `CamelCase` with `::` separators (e.g. `Math::Utils`). All unqualified sub and `our` variable names are installed into the current package. In stryke, packages work identically to standard Perl — they provide namespace isolation but are not classes by themselves (use `struct` or `bless` for OOP). Switching packages mid-file is allowed but discouraged; prefer one package per file.\n\n```perl\npackage Math::Utils\nfn factorial($n) { $n <= 1 ? 1 : $n * factorial($n - 1) }\nfn fib($n) { $n < 2 ? $n : fib($n - 1) + fib($n - 2) }\npackage main\np Math::Utils::factorial(10)   # 3628800\n```",
        "use" => "Load and import a module at compile time: `use Module qw(func);`.\n\n```perl\nuse List::Util qw(sum max)\nmy @vals = 1..10\np sum(@vals)   # 55\np max(@vals)   # 10\n```",
        "no" => "Unimport a module or pragma: `no strict 'refs';`.\n\n```perl\nno warnings 'experimental'\ngiven ($x) { when (1) { p \"one\" } }\n```",
        "require" => "Load a module at runtime: `require Module;`.\n\n```perl\nrequire JSON\nmy $data = JSON::decode_json($text)\np $data->{name}\n```",
        "return" => "Return a value from a subroutine.\n\n```perl\nfn clamp($v, $lo, $hi) {\n    return $lo if $v < $lo\n    return $hi if $v > $hi\n    $v\n}\n```",
        "BEGIN" => "`BEGIN { }` — runs at compile time, before the rest of the program.\n\n```perl\nBEGIN { p \"compiling...\" }\np \"running\"\n# output: compiling... then running\n```",
        "END" => "`END { }` — runs after the program finishes (or on exit).\n\n```perl\nEND { p \"cleanup done\" }\np \"main work\"\n# output: main work then cleanup done\n```",

        // ── Control flow ──
        "if" => "The fundamental conditional construct. Evaluates its condition in boolean context and executes the block if true. stryke supports both the block form `if (COND) { BODY }` and the postfix form `EXPR if COND`, which is idiomatic for single-statement guards. The condition can be any expression — numbers (0 is false), strings (empty and `\"0\"` are false), undef (false), and references (always true). Postfix `if` cannot have `elsif`/`else` — use the block form for multi-branch logic.\n\n```perl\nmy $n = 42\nif ($n > 0) { p \"positive\" }\np \"big\" if $n > 10          # postfix — clean one-liner\nmy $label = \"even\" if $n % 2 == 0\np \"got: $n\" if defined $n   # guard against undef\n```",
        "elsif" => "Chain additional conditions after an `if` block without nesting. Each `elsif` is tested in order; the first one whose condition is true has its block executed, and the rest are skipped. There is no limit on the number of `elsif` branches. In stryke, prefer `match` for complex multi-branch dispatch since it supports pattern matching, destructuring, and guards — but `elsif` remains the right tool for simple linear condition chains. Note: it is `elsif`, not `elseif` or `else if` — the latter is a syntax error.\n\n```perl\nfn classify($n) {\n    if    ($n < 0)   { \"negative\" }\n    elsif ($n == 0)  { \"zero\" }\n    elsif ($n < 10)  { \"small\" }\n    elsif ($n < 100) { \"medium\" }\n    else             { \"large\" }\n}\n1..200 |> map classify |> frequencies |> dd\n```",
        "else" => "The final fallback branch of an `if`/`elsif` chain, executed when no preceding condition was true. Every `if` can have at most one `else`, and it must come last. For ternary-style expressions, use `COND ? A : B` instead of an `if`/`else` block — it composes better in `|>` pipes and assignments.\n\n```perl\nif ($n % 2 == 0) { p \"even\" }\nelse             { p \"odd\" }\n\n# Ternary is often cleaner in pipes:\n1..10 |> map { $_ % 2 == 0 ? \"even\" : \"odd\" } |> e p\n```",
        "unless" => "A negated conditional — executes the block when the condition is *false*. This reads more naturally than `if (!COND)` for guard clauses and early returns. stryke supports both block and postfix forms. Convention: use `unless` for simple negative guards; avoid `unless` with complex compound conditions, as double-negatives hurt readability. There is no `unlessif` — use `if`/`elsif` chains for multi-branch logic.\n\n```perl\nunless ($ENV{QUIET}) { p \"verbose output\" }\np \"missing!\" unless -e $path   # postfix guard\ndie \"no input\" unless @ARGV\nreturn unless defined $val    # early return pattern\n```",
        "foreach" | "for" => "Iterate over a list, binding each element to a loop variable (or `$_` by default). `for` and `foreach` are interchangeable keywords. The loop variable is automatically localized and aliases the original element — modifications to `$_` inside the loop mutate the list in-place. In stryke, `for` loops work with ranges, arrays, hash slices, and iterator results. For parallel iteration, see `pfor`; for pipeline-style processing, prefer `|> e` or `|> map`. C-style `for (INIT; COND; STEP)` is also supported.\n\n```perl\nfor my $f (glob \"*.txt\") { p $f }\nfor (1..5) { p $_ * 2 }            # $_ is default\nmy @names = qw(alice bob carol)\nfor (@names) { $_ = uc $_ }         # mutates in-place\np join \", \", @names                 # ALICE, BOB, CAROL\n```",
        "while" => "Loop that re-evaluates its condition before each iteration and continues as long as it is true. Commonly used for reading input line-by-line, polling, and indefinite iteration. The condition is tested in boolean context. `while` integrates naturally with the diamond operator `<>` for reading filehandles. The loop variable can be declared in the condition with `my`, scoping it to the loop body. Postfix form is also supported: `EXPR while COND;`.\n\n```perl\nwhile (my $line = <STDIN>) {\n    $line |> tm |> p             # trim + print each line\n}\nmy $i = 0\nwhile ($i < 5) { p $i++ }    # counted loop\nwhile (1) { last if done() }     # infinite loop with break\n```",
        "until" => "Loop that continues as long as its condition is *false* — the logical inverse of `while`. Useful when the termination condition is more naturally expressed as a positive assertion (\"keep going until X happens\"). Supports both block and postfix forms. Prefer `while` with a negated condition if `until` makes the logic harder to read.\n\n```perl\nmy $n = 1\nuntil ($n > 1000) { $n *= 2 }\np $n   # 1024\n\nmy $tries = 0\nuntil (connected()) {\n    $tries++\n    sleep 1\n}\np \"connected after $tries tries\"\n```",
        "do" => "Execute a block and return its value, or execute a file. As a block, `do { ... }` creates an expression scope — the last expression in the block is the return value, making it useful for complex initializations. As a file operation, `do \"file.pl\"` executes the file in the current scope and returns its last expression. Unlike `require`, `do` does not cache and re-executes on each call. `do { ... } while (COND)` creates a loop that always runs at least once.\n\n```perl\nmy $val = do { my $x = 10\n    $x ** 2 }   # 100\np $val\nmy $cfg = do \"config.pl\"               # load config\n# do-while: body runs at least once\nmy $input\ndo { $input = readline(STDIN) |> tm } while ($input eq \"\")\n```",
        "last" => "Immediately exit the innermost enclosing loop (equivalent to `break` in C/Rust). Execution continues after the loop. `last LABEL` can target a labeled outer loop to break out of nested loops. Works in `for`, `foreach`, `while`, `until`, and `do-while`. Does *not* work inside `map`, `grep`, or `|>` pipeline stages — use `take`, `first`, or `take_while` for early termination in functional contexts.\n\n```perl\nfor (1..1_000_000) {\n    last if $_ > 5\n    p $_\n}   # prints 1 2 3 4 5\n\nOUTER: for my $i (1..10) {\n    for my $j (1..10) {\n        last OUTER if $i * $j > 50   # break both loops\n    }\n}\n```\n\nFor pipeline early-exit: `1..1000 |> take_while { $_ < 50 } |> e p`.",
        "next" => "Skip the rest of the current loop iteration and jump to the next one. The loop condition (for `while`/`until`) or the next element (for `for`/`foreach`) is evaluated immediately. Like `last`, `next` supports labeled loops with `next LABEL` for skipping in nested loops. This is the primary tool for filtering within imperative loops. In stryke, consider `grep`/`filter` or `|> reject` for functional-style filtering instead.\n\n```perl\nfor (1..10) {\n    next if $_ % 2        # skip odds\n    p $_\n}   # 2 4 6 8 10\n\nfor my $file (glob \"*\") {\n    next unless -f $file   # skip non-files\n    next if $file =~ /^\\./  # skip hidden\n    p $file\n}\n```",
        "redo" => "Restart the current loop iteration from the top of the loop body *without* re-evaluating the loop condition or advancing to the next element. The loop variable retains its current value. This is a niche but powerful tool for retry logic within loops — when an iteration fails, `redo` lets you try again with the same input. Use sparingly, as it can create infinite loops if the retry condition never resolves. Always pair with a guard or counter. For automated retry with backoff, prefer `retry { ... } times => N, backoff => 'exponential'`.\n\n```perl\nfor my $url (@urls) {\n    my $body = eval { fetch($url) }\n    if ($@) {\n        warn \"retry $url: $@\"\n        sleep 1\n        redo   # try same URL again\n    }\n    p length($body)\n}\n```",
        "continue" => "A block attached to a `for`/`foreach`/`while` loop that executes after each iteration, even when `next` is called. Analogous to the increment expression in a C-style `for` loop. The `continue` block does *not* run when `last` or `redo` is used. Useful for unconditional per-iteration bookkeeping like incrementing counters, logging progress, or flushing buffers. Rarely used but fully supported in stryke.\n\n```perl\nmy $count = 0\nfor my $item (@work) {\n    next if $item->{skip}\n    process($item)\n} continue {\n    $count++\n    p \"processed $count so far\" if $count % 100 == 0\n}\n```",
        "given" => "A switch-like construct that evaluates an expression and dispatches to `when` blocks via smartmatch semantics. The topic variable `$_` is set to the `given` expression for the duration of the block. Each `when` clause is tested in order; the first match executes its block and control passes out of the `given` (implicit break). A `default` block handles the no-match case. In stryke, prefer the `match` keyword for new code — it offers pattern destructuring, typed patterns, array/hash shape matching, and `if` guards that `given`/`when` cannot express.\n\n```perl\ngiven ($cmd) {\n    when (\"start\")   { p \"starting up\" }\n    when (\"stop\")    { p \"shutting down\" }\n    when (/^re/)     { p \"restarting\" }\n    default          { p \"unknown: $cmd\" }\n}\n```\n\nSee `match` for stryke-native pattern matching with destructuring.",
        "when" => "A case clause inside a `given` block. The expression is matched against the topic `$_` using smartmatch semantics: strings match exactly, regexes match against `$_`, arrayrefs check membership, coderefs are called with `$_` as argument, and numbers compare numerically. When a `when` clause matches, its block executes and control exits the enclosing `given` (implicit break). Multiple `when` clauses are tried in order until one matches.\n\n```perl\ngiven ($val) {\n    when (/^\\d+$/)      { p \"number\" }\n    when ([\"a\",\"b\",\"c\"]) { p \"early letter\" }\n    when (42)            { p \"the answer\" }\n    default              { p \"something else\" }\n}\n```\n\nIn stryke, the `match` keyword provides more powerful pattern matching.",
        "default" => "The fallback clause in a `given` block, executed when no `when` clause matched. Every `given` should have a `default` to handle unexpected values, similar to `else` in an `if` chain or the wildcard `_` arm in stryke `match`. If no `default` is present and nothing matches, execution simply continues after the `given` block. In stryke `match`, use `_ => ...` for the default arm instead.\n\n```perl\ngiven ($exit_code) {\n    when (0) { p \"success\" }\n    when (1) { p \"general error\" }\n    when (2) { p \"misuse\" }\n    default  { p \"unknown exit code: $exit_code\" }\n}\n```",

        // ── Exception handling ──
        "try" => "Structured exception handling that cleanly separates the happy path from error recovery. The `try` block runs the code; if it throws (via `die`), execution jumps to the `catch` block with the exception bound to the declared variable. An optional `finally` block runs unconditionally afterward — ideal for cleanup like closing filehandles or releasing locks. Unlike `eval`, `try`/`catch` is a first-class statement with proper scoping and no `$@` pollution. In stryke, `try` integrates with all exception types including string messages, hashrefs, and objects.\n\n```perl\ntry {\n    my $data = fetch_json($url)\n    p $data->{name}\n} catch ($e) {\n    warn \"request failed: $e\"\n    return fallback()\n} finally {\n    log_info(\"fetch attempt complete\")\n}\n```\n\nPrefer `try`/`catch` over `eval { }` for new code — it reads better and avoids `$@` clobbering races.",
        "catch" => "The error-handling clause that follows a `try` block. When the `try` block throws an exception, execution transfers to `catch` with the exception value bound to the declared variable (e.g. `$e`). The catch variable is lexically scoped to the catch block. You can inspect the exception — it may be a string, a hashref with structured error info, or an object with methods. Multiple error types can be differentiated with `ref` or `match` inside the catch body. If the catch block itself throws, the exception propagates upward (the `finally` block still runs first, if present).\n\n```perl\ntry { die { code => 404, msg => \"not found\" } }\ncatch ($e) {\n    if (ref $e eq 'HASH') {\n        p \"error $e->{code}: $e->{msg}\"\n    } else {\n        p \"caught: $e\"\n    }\n}\n```",
        "finally" => "A cleanup block that runs after `try`/`catch` regardless of whether an exception was thrown or not. This guarantees resource cleanup even if the `try` block throws or the `catch` block re-throws. The `finally` block cannot change the exception or the return value — it is strictly for side effects like closing filehandles, releasing locks, or logging. If `finally` itself throws, that exception replaces the original one (avoid throwing in finally). `finally` is optional — you can use `try`/`catch` without it.\n\n```perl\nmy $fh\ntry {\n    open $fh, '<', $path or die $!\n    process(<$fh>)\n} catch ($e) {\n    log_error(\"failed: $e\")\n} finally {\n    close $fh if $fh   # always cleanup\n}\n```",
        "eval" => "The classic Perl exception-catching mechanism. `eval { BLOCK }` executes the block in an exception-trapping context: if the block throws (via `die`), execution continues after the `eval` with the error stored in `$@`. `eval \"STRING\"` compiles and executes Perl code at runtime — powerful but dangerous (code injection risk). In stryke, prefer `try`/`catch` for exception handling as it avoids the `$@` clobbering pitfalls and reads more clearly. `eval` remains useful for dynamic code evaluation and backward compatibility with Perl 5 idioms.\n\n```perl\neval { die \"oops\" }\nif ($@) { p \"caught: $@\" }\n\n# Eval string — dynamic code execution\nmy $expr = \"2 + 2\"\nmy $result = eval $expr\np $result   # 4\n```\n\nCaveat: `$@` can be clobbered by intervening `eval`s or destructors — `try`/`catch` avoids this.",
        "die" => "Raise an exception, immediately unwinding the call stack until caught by `try`/`catch` or `eval`. The argument can be a string (most common), a reference (hashref for structured errors), or an object. If uncaught, the program terminates and the message is printed to STDERR. In stryke, `die` works identically to Perl 5 and integrates with `try`/`catch`, `eval`, and the `$@` mechanism. Convention: end die messages with `\\n` to suppress the automatic \"at FILE line LINE\" suffix, or omit it to get location info for debugging.\n\n```perl\nfn divide($a, $b) {\n    die \"division by zero\\n\" unless $b\n    $a / $b\n}\n\n# Structured error\ndie { code => 400, msg => \"bad request\", field => \"email\" }\n\n# With automatic location\ndie \"something broke\"   # prints: something broke at script.pl line 5.\n```",
        "warn" => "Print a warning message to STDERR without terminating the program. Behaves like `die` but only emits the message instead of throwing an exception. If the message does not end with `\\n`, Perl appends the current file and line number. Warnings can be intercepted with `$SIG{__WARN__}` or suppressed with `no warnings`. In stryke, `warn` is useful for non-fatal diagnostics; for structured logging, use `log_warn` instead.\n\n```perl\nwarn \"file not found: $path\" unless -e $path\nwarn \"deprecated: use fetch_json instead\\n\"  # no line number\n\n# Intercept warnings\nlocal $SIG{__WARN__} = fn ($msg) { log_warn($msg) }\nwarn \"redirected to logger\"\n```",
        "croak" => "Die from the caller's perspective — the error message reports the file and line of the *caller*, not the function that called `croak`. This is the right choice for library/module functions where the error is the caller's fault (bad arguments, misuse). Without `croak`, the user sees an error pointing at library internals, which is unhelpful. In stryke, `croak` is available as a builtin without `use Carp`. For debugging deep call chains, use `confess` instead to get a full stack trace.\n\n```perl\nfn parse($s) {\n    croak \"parse: empty input\" unless length $s\n    croak \"parse: not JSON\" unless $s =~ /^[{\\[]/\n    json_decode($s)\n}\n\n# Error will point at the call site, not inside parse()\nmy $data = parse(\"\")   # dies: \"parse: empty input at caller.pl line 5\"\n```",
        "confess" => "Die with a full stack trace from the point of the error all the way up through every caller. This is invaluable for debugging deep call chains where `die` or `croak` only show one frame. Each frame includes the package, file, and line number. In stryke, `confess` is available as a builtin without `use Carp`. Use `confess` during development for maximum diagnostic info; switch to `croak` in production-facing libraries where the trace would confuse end users.\n\n```perl\nfn validate($data) {\n    confess \"missing required field 'name'\" unless $data->{name}\n}\nfn process($input) { validate($input) }\nfn main { process({}) }   # full trace: main -> process -> validate\n```\n\nThe trace output shows: `missing required field 'name' at script.pl line 2.\\n\\tmain::validate called at line 4\\n\\t...`.",

        // ── I/O ──
        "say" => "Print operands followed by an automatic newline to `STDOUT`. In stryke, `say` is always available without `-E` or `use feature 'say'` — it is a first-class builtin. The shorthand `p` is an alias for `say` and is preferred in most stryke code. When given a list, `say` joins elements with `$,` (output field separator, empty by default) and appends `$\\` plus a newline. For streaming output over pipelines, combine with `e` (each) to print one element per line. Gotcha: `say` always adds a newline — if you need raw output without one, use `print` instead.\n\n```perl\np \"hello world\"\nmy @names = (\"alice\", \"bob\", \"eve\")\n@names |> e p\n1..5 |> maps { $_ * 2 } |> e p\n```",
        "print" => "Write operands to the selected output handle (default `STDOUT`) without appending a newline. The output field separator `$,` is inserted between arguments, and `$\\` is appended at the end — both default to empty string. You can direct output to a specific handle by passing it as the first argument with no comma: `print STDERR \"msg\"`. In stryke, `print` behaves identically to Perl 5 and is useful when you need precise control over output formatting, such as building progress bars, writing binary data, or emitting partial lines. For most line-oriented output, prefer `p` (say) instead since it handles the newline automatically.\n\n```perl\nprint \"no newline\"\nprint STDERR \"error msg\\n\"\nprint $fh \"data to filehandle\\n\"\nfor my $pct (0..100) {\n    printf \"\\rprogress: %3d%%\", $pct\n}\nprint \"\\n\"\n```",
        "printf" => "Formatted print to a filehandle (default `STDOUT`), using C-style format specifiers. The first argument is the format string with `%s` (string), `%d` (integer), `%f` (float), `%x` (hex), `%o` (octal), `%e` (scientific), `%g` (general float), and `%%` (literal percent). Width and precision modifiers work as in C: `%-10s` left-aligns in a 10-char field, `%05d` zero-pads to 5 digits, `%.2f` gives 2 decimal places. In stryke, `printf` supports all standard Perl 5 format specifiers including `%v` (version strings) and `%n` is disabled for safety. Direct output to a handle by placing it before the format: `printf STDERR \"...\", @args`. Unlike `sprintf`, `printf` outputs directly and returns a boolean indicating success.\n\n```perl\nprintf \"%-10s %5d\\n\", $name, $score\nprintf STDERR \"error %d: %s\\n\", $code, $msg\nprintf \"%08.2f\\n\", 3.14159   # 00003.14\nprintf \"%s has %d item%s\\n\", $user, $n, $n == 1 ? \"\" : \"s\"\n```",
        "sprintf" => "Return a formatted string without printing it, using the same C-style format specifiers as `printf`. This is the go-to function for building formatted strings for later use — constructing log messages, building padded table columns, converting numbers to hex or binary representations, or assembling strings that will be passed to other functions. In stryke, `sprintf` is often combined with the pipe operator: `$val |> t { sprintf \"0x%04x\", $_ } |> p`. The return value is always a string. All format specifiers from `printf` apply here.\n\n```perl\nmy $hex = sprintf \"0x%04x\", 255\nmy $padded = sprintf \"%08d\", $id\nmy $msg = sprintf \"%-20s: %s\", $key, $value\nmy @rows = map { sprintf \"%3d. %s\", $_, $names[$_] } 0..$ #names\n@rows |> e p\n```",
        "open" => "Open a filehandle for reading, writing, appending, or piping. The three-argument form `open my $fh, MODE, EXPR` is strongly preferred for safety — it prevents shell injection and makes the mode explicit. Modes include `<` (read), `>` (write/truncate), `>>` (append), `+<` (read-write), `|-` (pipe to command), and `-|` (pipe from command). In stryke, always use lexical filehandles (`my $fh`) rather than bareword globals. PerlIO layers can be specified in the mode: `<:utf8`, `<:raw`, `<:encoding(UTF-16)`. Always check the return value — `open ... or die \"...: $!\"` is idiomatic. The `$!` variable contains the OS error message on failure. Forgetting to check `open` is one of the most common bugs in Perl code.\n\n```perl\nopen my $fh, '<', 'data.txt' or die \"open: $!\"\nmy @lines = <$fh>\nclose $fh\n\nopen my $out, '>>', 'log.txt' or die \"append: $!\"\nprint $out tm \"event happened\\n\"\nclose $out\n\nopen my $pipe, '-|', 'ls', '-la' or die \"pipe: $!\"\nwhile (<$pipe>) { p tm $_ }\n```",
        "close" => "Close a filehandle, flushing any buffered output and releasing the underlying OS file descriptor. Returns true on success, false on failure — and failure is more common than you might think. For write handles, `close` is where buffered data actually hits disk, so a full disk or network error may only surface at `close` time. Always check the return value when writing: `close $fh or die \"close: $!\"`. For pipe handles opened with `|-` or `-|`, `close` waits for the child process to exit and sets `$?` to the child's exit status. In stryke, lexical filehandles are automatically closed when they go out of scope, but explicit `close` is clearer and lets you handle errors.\n\n```perl\nopen my $fh, '>', 'out.txt' or die $!\nprint $fh \"done\\n\"\nclose $fh or die \"write failed: $!\"\n\nopen my $p, '|-', 'gzip', '-c' or die $!\nprint $p $data\nclose $p\np \"gzip exited: $?\" if $?\n```",
        "read" => "Read a specified number of bytes from a filehandle into a scalar buffer. The signature is `read FH, SCALAR, LENGTH [, OFFSET]`. Returns the number of bytes actually read (which may be less than requested at EOF or on partial reads), 0 at EOF, or `undef` on error. The optional OFFSET argument lets you append to an existing buffer at a given position, which is useful for accumulating data in a loop. For text files, the bytes are decoded according to the handle's PerlIO layer — use `<:raw` for binary data to avoid encoding transforms. In stryke, `read` works identically to Perl 5. For line-oriented input, prefer `<$fh>` or `readline` instead.\n\n```perl\nopen my $fh, '<:raw', $path or die $!\nmy $buf = ''\nwhile (read $fh, my $chunk, 4096) {\n    $buf .= $chunk\n}\np \"total: \" . length($buf) . \" bytes\"\nclose $fh\n```",
        "readline" => "Read one line (or all remaining lines in list context) from a filehandle. The angle-bracket operator `<$fh>` is syntactic sugar for `readline($fh)`. In scalar context, returns the next line including the trailing newline (or `undef` at EOF). In list context, returns all remaining lines as a list. The line ending is determined by `$/` (input record separator, default `\\n`). Set `$/ = undef` to slurp the entire file in one read. In stryke, `readline` integrates with the pipe operator — you can pipe filehandle lines through `maps`, `greps`, and other streaming combinators. Always `chomp` after reading if you don't want trailing newlines.\n\n```perl\nwhile (my $line = <$fh>) {\n    chomp $line\n    p $line\n}\n\n# Slurp entire file\nlocal $/\nmy $content = <$fh>\np length $content\n```",
        "eof" => "Test whether a filehandle has reached end-of-file. Returns 1 if the next read on the handle would return EOF, 0 otherwise. Called without arguments, `eof()` (with parens) checks the last file in the `<>` / `ARGV` stream. Called with no parens as `eof`, it tests whether the current ARGV file is exhausted but more files may follow. In stryke, `eof` is typically used in `until` loops or as a guard before `read` calls. Note that `eof` may trigger a blocking read on interactive handles (like STDIN from a terminal) to determine if data is available, so avoid calling it speculatively on interactive input. For most line-processing, `while (<$fh>)` is simpler and implicitly handles EOF.\n\n```perl\nuntil (eof $fh) {\n    my $line = <$fh>\n    p tm $line\n}\n\n# Process multiple files via ARGV\nwhile (<>) {\n    p \"new file: $ARGV\" if eof()\n}\n```",
        "seek" => "Reposition a filehandle to an arbitrary byte offset. The signature is `seek FH, POSITION, WHENCE` where WHENCE is 0 (absolute from start), 1 (relative to current position), or 2 (relative to end of file). Use the `Fcntl` constants `SEEK_SET`, `SEEK_CUR`, `SEEK_END` for clarity. Returns 1 on success, 0 on failure. `seek` is essential for random-access file I/O — re-reading headers, skipping to known offsets in binary formats, or rewinding a file for a second pass. In stryke, `seek` flushes the PerlIO buffer before repositioning. Do not mix `seek`/`tell` with `sysread`/`syswrite` — they use separate buffering layers.\n\n```perl\nseek $fh, 0, 0         # rewind to start\nmy $header = <$fh>\n\nseek $fh, -100, 2      # last 100 bytes\nread $fh, my $tail, 100\np $tail\n```",
        "tell" => "Return the current byte offset of a filehandle's read/write position. Returns a non-negative integer on success, or -1 if the handle is invalid or not seekable (e.g., pipes, sockets). Useful for bookmarking a position before a speculative read so you can `seek` back if the data doesn't match expectations. In stryke, `tell` reflects the PerlIO buffered position, not the raw OS file descriptor position — so it correctly accounts for encoding layers and buffered reads. Pair with `seek` for random-access patterns.\n\n```perl\nmy $pos = tell $fh\np \"at byte $pos\"\n\n# Bookmark and restore\nmy $mark = tell $fh\nmy $line = <$fh>\nunless ($line =~ /^HEADER/) {\n    seek $fh, $mark, 0   # rewind to before the read\n}\n```",
        "binmode" => "Set the I/O layer on a filehandle, controlling how bytes are translated during reads and writes. Without a layer argument, `binmode $fh` switches the handle to raw binary mode (no CRLF translation on Windows, no encoding transforms). With a layer, `binmode $fh, ':utf8'` enables UTF-8 decoding, `binmode $fh, ':raw'` strips all layers for pure byte I/O, and `binmode $fh, ':encoding(ISO-8859-1)'` sets a specific encoding. In stryke, encoding layers can also be specified directly in `open`: `open my $fh, '<:utf8', $path`. Call `binmode` before any I/O on the handle — calling it mid-stream may produce garbled output. For binary file processing (images, archives, network protocols), always use `:raw`.\n\n```perl\nopen my $fh, '<', $path or die $!\nbinmode $fh, ':utf8'\nmy @lines = <$fh>\n\nopen my $bin, '<', $img_path or die $!\nbinmode $bin, ':raw'\nread $bin, my $header, 8\np sprintf \"magic: %s\", unpack(\"H*\", $header)\n```",
        "fileno" => "Return the underlying OS file descriptor number for a filehandle, or `undef` if the handle is not connected to a real file descriptor (e.g., tied handles, in-memory handles opened to scalar refs). File descriptor numbers are small non-negative integers managed by the OS kernel: 0 is STDIN, 1 is STDOUT, 2 is STDERR. This function is mainly useful for interfacing with system calls that require raw fd numbers, checking whether two handles share the same underlying fd, or passing descriptors to child processes. In stryke, `fileno` is rarely needed in everyday code but is essential for low-level I/O multiplexing and process management.\n\n```perl\nmy $fd = fileno STDOUT\np \"stdout fd: $fd\"   # 1\n\nif (defined fileno $fh) {\n    p \"handle is backed by fd \" . fileno($fh)\n} else {\n    p \"not a real file descriptor\"\n}\n```",
        "truncate" => "Truncate a file to a specified byte length. Accepts either a filehandle or a filename as the first argument, and the desired length as the second. `truncate $fh, 0` empties the file entirely — a common pattern when rewriting a file in place. `truncate $fh, $len` discards everything beyond byte `$len`. Returns true on success, false on failure (check `$!` for the error). In stryke, truncate works on any seekable filehandle. When using truncate to rewrite a file, remember to also `seek $fh, 0, 0` to rewind the write position — truncating does not move the file pointer. Gotcha: truncating a file opened read-only will fail.\n\n```perl\nopen my $fh, '+<', 'data.txt' or die $!\ntruncate $fh, 0       # empty the file\nseek $fh, 0, 0        # rewind to start\nprint $fh \"fresh content\\n\"\nclose $fh\n```",
        "flock" => "Advisory file locking for coordinating access between processes. `flock $fh, OPERATION` where OPERATION is `LOCK_SH` (shared/read lock), `LOCK_EX` (exclusive/write lock), or `LOCK_UN` (unlock). Add `LOCK_NB` for non-blocking mode: `LOCK_EX | LOCK_NB` returns false immediately if the lock is held rather than waiting. Import constants from `Fcntl`. Advisory locks are cooperative — they only work if all processes accessing the file use `flock`. In stryke, `flock` is the standard mechanism for safe concurrent file access in multi-process scripts, cron jobs, and daemons. Always unlock explicitly or let the handle close (which releases the lock). Gotcha: `flock` does not work on NFS on many systems.\n\n```perl\nuse Fcntl ':flock'\nopen my $fh, '>>', 'shared.log' or die $!\nflock $fh, LOCK_EX or die \"lock: $!\"\nprint $fh tm \"pid $$ wrote this\\n\"\nflock $fh, LOCK_UN\nclose $fh\n\nunless (flock $fh, LOCK_EX | LOCK_NB) {\n    p \"file is locked by another process\"\n}\n```",
        "getc" => "Read a single character from a filehandle (default `STDIN`). Returns `undef` at EOF. The character returned respects the handle's encoding layer — under `:utf8`, `getc` returns a full Unicode character which may be multiple bytes on disk. This function is useful for interactive single-key input, parsing binary formats one byte at a time, or implementing character-level tokenizers. In stryke, `getc` blocks until a character is available on the handle. For terminal input, note that most terminals are line-buffered by default, so `getc STDIN` won't return until the user presses Enter unless you put the terminal into raw mode.\n\n```perl\nmy $ch = getc STDIN\np \"you pressed: $ch\"\n\nopen my $fh, '<:utf8', $path or die $!\nwhile (defined(my $c = getc $fh)) {\n    p \"char: $c (ord \" . ord($c) . \")\"\n}\n```",
        "select" => "In its one-argument form, `select HANDLE` sets the default output handle for `print`, `say`, and `write`, returning the previously selected handle. This is useful for temporarily redirecting output — for example, sending diagnostics to STDERR while a report goes to a file. In its four-argument form, `select RBITS, WBITS, EBITS, TIMEOUT` performs POSIX `select(2)` I/O multiplexing, waiting for one or more file descriptors to become ready for reading, writing, or to report exceptions. The four-argument form is low-level and rarely used directly in stryke — prefer `IO::Select` or async I/O patterns for multiplexing. `select` with `$|` is also the classic way to enable autoflush on a handle.\n\n```perl\nmy $old = select STDERR\np \"this goes to stderr\"\nselect $old\n\n# Enable autoflush on STDOUT\nselect STDOUT; $| = 1\nprint \"immediately flushed\"\n```",
        "sysread" => "Low-level unbuffered read directly from a file descriptor, bypassing PerlIO buffering layers. The signature is `sysread FH, SCALAR, LENGTH [, OFFSET]`. Returns the number of bytes read, 0 at EOF, or `undef` on error. Unlike `read`, `sysread` issues a single `read(2)` system call and may return fewer bytes than requested (short read). This is the right choice for sockets, pipes, and non-blocking I/O where you need precise control over how many system calls occur and cannot tolerate buffering. In stryke, never mix `sysread`/`syswrite` with buffered I/O (`print`, `read`, `<$fh>`) on the same handle — the buffered and unbuffered positions will diverge and produce corrupted reads.\n\n```perl\nopen my $fh, '<:raw', $path or die $!\nmy $buf = ''\nwhile (my $n = sysread $fh, $buf, 4096, length($buf)) {\n    p \"read $n bytes (total: \" . length($buf) . \")\"\n}\nclose $fh\n```",
        "syswrite" => "Low-level unbuffered write directly to a file descriptor, bypassing PerlIO buffering layers. The signature is `syswrite FH, SCALAR [, LENGTH [, OFFSET]]`. Returns the number of bytes actually written, which may be less than requested on non-blocking handles or when writing to pipes/sockets (short write). Returns `undef` on error. Like `sysread`, this issues a single `write(2)` system call and must not be mixed with buffered I/O on the same handle. In stryke, `syswrite` is essential for socket programming, IPC, and performance-critical binary output where you need to avoid double-buffering. Always check the return value and handle short writes in a loop for robust code.\n\n```perl\nmy $data = \"hello, world\"\nmy $n = syswrite $fh, $data\np \"wrote $n bytes\"\n\n# Robust write loop for sockets\nmy $off = 0\nwhile ($off < length $data) {\n    my $written = syswrite $fh, $data, length($data) - $off, $off\n    die \"syswrite: $!\" unless defined $written\n    $off += $written\n}\n```",
        "sysseek" => "Low-level seek on a file descriptor, bypassing PerlIO buffering. The signature is `sysseek FH, POSITION, WHENCE` with the same WHENCE values as `seek` (0=start, 1=current, 2=end). Returns the new position as a true value, or `undef` on failure. Unlike `seek`, `sysseek` does not flush PerlIO buffers — it operates directly on the underlying OS file descriptor. Use `sysseek` when working with `sysread`/`syswrite` for consistent positioning. In stryke, `sysseek` with `SEEK_CUR` and position 0 is an idiom for querying the current fd position without moving: `my $pos = sysseek $fh, 0, 1`.\n\n```perl\nsysseek $fh, 0, 0   # rewind to start\nsysread $fh, my $buf, 512\n\nmy $pos = sysseek $fh, 0, 1\np \"fd position: $pos\"\n```",
        "sysopen" => "Low-level open using POSIX flags for precise control over how a file is opened. The signature is `sysopen FH, FILENAME, FLAGS [, PERMS]`. Flags are bitwise-OR combinations from `Fcntl`: `O_RDONLY`, `O_WRONLY`, `O_RDWR`, `O_CREAT`, `O_EXCL`, `O_TRUNC`, `O_APPEND`, `O_NONBLOCK`, and others. The optional PERMS argument (e.g., `0644`) sets the file mode when `O_CREAT` creates a new file, subject to the process umask. `sysopen` is the right tool when you need `O_EXCL` for atomic file creation (lock files, temp files), `O_NONBLOCK` for non-blocking I/O, or other flags that `open` cannot express. In stryke, prefer three-argument `open` for routine file access and reserve `sysopen` for cases requiring specific POSIX semantics.\n\n```perl\nuse Fcntl\n# Atomic create — fails if file already exists\nsysopen my $lock, '/tmp/my.lock', O_WRONLY|O_CREAT|O_EXCL, 0644\n    or die \"already running: $!\"\nprint $lock $$\n\nsysopen my $log, 'app.log', O_WRONLY|O_APPEND|O_CREAT, 0644\n    or die \"open log: $!\"\n```",
        "write" => "Output a formatted record to a filehandle using a `format` declaration. `write` looks up the format associated with the current (or specified) filehandle, evaluates the format's picture lines against the current variables, and outputs the result. This is Perl's original report-generation mechanism, predating modules like `Text::Table` and template engines. In stryke, `write` and `format` are supported for backward compatibility but are rarely used in new code — `printf`/`sprintf` or template strings are more flexible. The format name defaults to the filehandle name (e.g., format `STDOUT` is used by `write STDOUT`).\n\n```perl\nformat STDOUT =\n@<<<<<<<<<< @>>>>>>\n$name,       $score\n.\n\nmy ($name, $score) = (\"alice\", 42)\nwrite   # outputs: alice           42\n```",
        "format" => "Declare a picture-format template for generating fixed-width text reports. The syntax is `format NAME = ... .` where each line alternates between picture lines (containing field placeholders) and argument lines (listing the variables to fill in). Placeholders include `@<<<<` (left-align), `@>>>>` (right-align), `@||||` (center), `@###.##` (numeric with decimal), and `@*` (multiline block fill). The format ends with a lone `.` on its own line. In stryke, formats are a legacy feature preserved for compatibility with Perl 5 code — for new reports, prefer `printf`/`sprintf` for simple alignment or a templating module for complex layouts. Formats interact with the special variables `$~` (current format name), `$^` (header format), and `$=` (lines per page).\n\n```perl\nformat REPORT =\n@<<<<<<<<<<<<<<<< @>>>>> @ ###.##\n$item,            $qty,  $price\n.\n\nmy ($item, $qty, $price) = (\"Widget\", 100, 9.99)\nmy $old = select REPORT\n$~ = 'REPORT'\nwrite REPORT\nselect $old\n```",

        // ── Strings ──
        "chomp" => "`chomp STRING` — remove the trailing record separator (usually `\\n`) from a string in place and return the number of characters removed.\n\n`chomp` is the idiomatic way to strip newlines after reading input; unlike `chop`, it only removes the value of `$/` (the input record separator), so it is safe to call on strings that do not end with a newline — it simply does nothing. You can also `chomp` an entire array to strip every element at once. In stryke, `chomp` operates on UTF-8 strings and respects multi-byte `$/` values. Prefer `chomp` over `chop` in virtually all input-processing code; `chop` is only for when you truly need to remove an arbitrary trailing character regardless of what it is.\n\n```perl\nmy $line = <STDIN>\nchomp $line\np $line\nchomp(my @lines = <$fh>)  # strip all at once\np scalar @lines\n```",
        "chop" => "`chop STRING` — remove and return the last character of a string, modifying the string in place.\n\nUnlike `chomp`, `chop` unconditionally removes whatever the final character is — newline, letter, digit, or even a multi-byte UTF-8 codepoint in stryke. The return value is the removed character. This makes `chop` useful for peeling off known trailing delimiters or building parsers that consume input character-by-character, but dangerous for general newline removal because it will silently eat a real character if the string does not end with `\\n`. When called on an array, `chop` removes the last character of every element and returns the last one removed.\n\n```perl\nmy $s = \"hello!\"\nmy $last = chop $s  # $last = \"!\", $s = \"hello\"\np $s\nmy @words = (\"foo\\n\", \"bar\\n\")\nchop @words  # strips trailing newline from each\n@words |> e p\n```",
        "chr" => "`chr NUMBER` — return the character represented by the given ASCII or Unicode code point.\n\n`chr` is the inverse of `ord`: `chr(ord($c)) eq $c` always holds. It accepts any non-negative integer and returns a single-character string. In stryke, values above 127 produce valid UTF-8 characters, so `chr 0x1F600` gives you a smiley emoji with no special encoding gymnastics. For string literals, stryke supports all standard escapes: `\\x{hex}`, `\\u{hex}`, `\\o{oct}`, `\\NNN` (octal), `\\cX` (control), `\\N{U+hex}`, `\\N{UNICODE NAME}`, plus case modifiers `\\U..\\E`, `\\L..\\E`, `\\u`, `\\l`, `\\Q..\\E`.\n\n```perl\np chr 65       # A\np chr 0x1F600  # smiley emoji\np \"\\u{0301}\"   # combining acute accent\np \"\\N{SNOWMAN}\" # ☃\nmy @abc = map { chr($_ + 64) } 1..26\n@abc |> join \"\" |> p  # ABCDEFGHIJKLMNOPQRSTUVWXYZ\n```",
        "hex" => "`hex STRING` — interpret a hexadecimal string and return its numeric value.\n\nThe leading `0x` prefix is optional: both `hex \"ff\"` and `hex \"0xff\"` return 255. The string is case-insensitive, so `hex \"DeAdBeEf\"` works fine. If the string contains non-hex characters, stryke warns and converts up to the first invalid character. This is the standard way to parse hex-encoded values from config files, color codes, or protocol dumps. For the reverse operation (number to hex string), use `sprintf \"%x\"`. Note that `hex` always returns a number, never a string — arithmetic is immediate.\n\n```perl\nmy $n = hex \"deadbeef\"\nprintf \"0x%x = %d\\n\", $n, $n\np hex \"ff\"   # 255\n\"cafe\" |> t { hex } |> p   # 51966\n```",
        "oct" => "`oct STRING` — interpret an octal, hexadecimal, or binary string and return its numeric value.\n\n`oct` is the multi-base cousin of `hex`: it auto-detects the base from the prefix. Strings starting with `0b` are binary, `0x` are hex, and bare digits or `0`-prefixed digits are octal. This makes it the go-to for parsing Unix file permissions (`oct \"0755\"` gives 493), binary literals, or any string where the radix is embedded in the value. In stryke, `oct` handles arbitrarily large integers via the same big-number pathway as other arithmetic. A common gotcha: `oct \"8\"` warns because 8 is not a valid octal digit — use `hex` or a bare numeric literal instead.\n\n```perl\np oct \"0755\"    # 493\np oct \"0b1010\"  # 10\np oct \"0xff\"    # 255\nmy $perms = oct \"644\"\np sprintf \"%04o\", $perms  # 0644\n```",
        "index" => "`index STRING, SUBSTRING [, POSITION]` — return the zero-based position of the first occurrence of SUBSTRING within STRING, or -1 if not found.\n\nThe optional POSITION argument lets you start the search at a given offset, which is essential for scanning forward through a string in a loop (call `index` repeatedly, advancing POSITION past each hit). In stryke, `index` operates on UTF-8 character positions, not byte offsets, so it is safe for multi-byte strings. For finding the *last* occurrence, use `rindex` instead. A common pattern is pairing `index` with `substr` to extract fields from fixed-format data without the overhead of a regex or `split`.\n\n```perl\nmy $i = index \"hello world\", \"world\"  # 6\np $i\nmy $s = \"a.b.c.d\"\nmy $pos = 0\nwhile (($pos = index $s, \".\", $pos) != -1) {\n    p \"dot at $pos\"\n    $pos++\n}\n```",
        "rindex" => "`rindex STRING, SUBSTRING [, POSITION]` — return the zero-based position of the last occurrence of SUBSTRING within STRING, searching backward from POSITION (or the end).\n\n`rindex` mirrors `index` but searches from right to left, making it ideal for extracting file extensions, final path components, or the last delimiter in a string. The optional POSITION argument caps how far right the search begins — characters after POSITION are ignored. Returns -1 when the substring is not found. In stryke, positions are UTF-8 character offsets. Combine with `substr` for efficient right-side extraction without regex.\n\n```perl\nmy $path = \"foo/bar/baz.tar.gz\"\nmy $i = rindex $path, \"/\"     # 7\np substr $path, $i + 1        # baz.tar.gz\nmy $ext = rindex $path, \".\"   # 15\np substr $path, $ext + 1      # gz\n```",
        "lc" => "`lc STRING` — return a fully lowercased copy of the string.\n\n`lc` performs Unicode-aware lowercasing in stryke, so `lc \"\\x{C4}\"` (capital A with diaeresis) correctly returns the lowercase form. It does not modify the original string — it returns a new one. This is the standard way to normalize strings for case-insensitive comparison: `if (lc $a eq lc $b)`. In stryke `|>` pipelines, wrap `lc` with `t` to apply it as a streaming transform. For lowercasing only the first character, use `lcfirst` instead.\n\n```perl\np lc \"HELLO\"               # hello\n\"SHOUT\" |> t lc |> t rev |> p  # tuohs\nmy @norm = map lc, @words\n@norm |> e p\n```",
        "lcfirst" => "`lcfirst STRING` — return a copy of the string with only the first character lowercased, leaving the rest unchanged.\n\nThis is useful for converting PascalCase identifiers to camelCase, or for formatting output where only the initial letter matters. In stryke, `lcfirst` is Unicode-aware, so it handles accented capitals and multi-byte first characters correctly. Like `lc`, it returns a new string rather than modifying in place. If you need the entire string lowercased, use `lc` instead.\n\n```perl\np lcfirst \"Hello\"    # hello\np lcfirst \"XMLParser\" # xMLParser\nmy @camel = map lcfirst, @PascalNames\n@camel |> e p\n```",
        "uc" => "`uc STRING` — return a fully uppercased copy of the string.\n\n`uc` performs Unicode-aware uppercasing in stryke, correctly handling multi-byte characters and locale-sensitive transformations. It returns a new string without modifying the original. Use `uc` for normalizing strings before comparison, formatting headers, or transforming pipeline output. In stryke `|>` chains, combine with `maps` or `t` for streaming uppercase transforms. For uppercasing only the first character (e.g., sentence capitalization), use `ucfirst` instead.\n\n```perl\np uc \"hello\"  # HELLO\n@words |> maps { uc } |> e p\n\"whisper\" |> t uc |> p   # WHISPER\n```",
        "ucfirst" => "`ucfirst STRING` — return a copy of the string with only the first character uppercased, leaving the rest unchanged.\n\nThis is the standard way to capitalize the first letter of a word for display, title-casing, or converting camelCase to PascalCase. In stryke, `ucfirst` is Unicode-aware and correctly handles multi-byte leading characters. It returns a new string and does not modify the original. For full uppercasing, use `uc`. A common idiom is `ucfirst lc $word` to normalize a word to \"Title\" form.\n\n```perl\np ucfirst \"hello\"            # Hello\np ucfirst lc \"hELLO\"         # Hello\nmy @titled = map { ucfirst lc } @raw\n@titled |> join \" \" |> p\n```",
        "length" => "`length STRING` — return the number of characters in a string, or the number of elements when given an array.\n\nIn string context, `length` counts Unicode characters, not bytes — so `length \"\\x{1F600}\"` is 1 in stryke even though the emoji occupies 4 bytes in UTF-8. When passed an array, stryke returns the element count (equivalent to `scalar @arr`), which diverges slightly from Perl where `length @arr` stringifies the array first. This dual behavior is intentional in stryke for convenience. To get byte length instead of character length, use `bytes::length` or encode first. Always use `length` rather than comparing against the empty string when checking for non-empty input.\n\n```perl\np length \"hello\"  # 5\nmy @a = (1..10)\np length @a       # 10\np length \"\\x{1F600}\"  # 1 (single emoji codepoint)\n```",
        "substr" => "`substr STRING, OFFSET [, LENGTH [, REPLACEMENT]]` — extract or replace a substring.\n\n`substr` is stryke's Swiss-army knife for positional string manipulation. With two arguments it extracts from OFFSET to end; with three it extracts LENGTH characters; with four it replaces that range with REPLACEMENT and returns the original extracted portion. Negative OFFSET counts from the end of the string (`substr $s, -3` gives the last three characters). In stryke, offsets are UTF-8 character positions, making it safe for multi-byte text. `substr` as an lvalue (`substr($s, 0, 1) = \"X\"`) is also supported for in-place mutation. Prefer `substr` over regex when you know exact positions — it is faster and clearer.\n\n```perl\nmy $s = \"hello world\"\np substr $s, 0, 5              # hello\np substr $s, -5                # world\nsubstr $s, 6, 5, \"stryke\"     # $s = \"hello stryke\"\np $s\n```",
        "quotemeta" => "`quotemeta STRING` — escape all non-alphanumeric characters with backslashes, returning a string safe for interpolation into a regex.\n\nThis is essential when building dynamic regexes from user input: without `quotemeta`, characters like `.`, `*`, `(`, `)`, `[`, and `\\` would be interpreted as regex metacharacters, leading to either broken patterns or security vulnerabilities (ReDoS). The equivalent inline syntax is `\\Q..\\E` inside a regex. In stryke, `quotemeta` handles the full Unicode range, escaping any character that is not `[A-Za-z0-9_]`. Use it liberally whenever you splice user-provided strings into patterns.\n\n```perl\nmy $input = \"file (1).txt\"\nmy $safe = quotemeta $input\np $safe  # file\\ \\(1\\)\\.txt\nmy $found = (\"file (1).txt\" =~ /^$safe$/)\np $found  # 1\n```",
        "ord" => "`ord STRING` — return the numeric (Unicode code point) value of the first character of the string.\n\n`ord` is the inverse of `chr`: `ord(chr($n)) == $n` always holds. When passed a multi-character string, only the first character is examined — the rest are ignored. In stryke, `ord` returns the full Unicode code point, not just 0-255, so `ord \"\\u{1F600}\"` returns 128512. This is useful for character classification, building lookup tables, or implementing custom encodings. For ASCII checks, `ord($c) >= 32 && ord($c) <= 126` tests printability.\n\n```perl\np ord \"A\"          # 65\np ord \"\\n\"         # 10\np ord \"\\u{1F600}\"  # 128512\nmy @codes = map ord, split //, \"hello\"\n@codes |> e p      # 104 101 108 108 111\n```",
        "join" => "`join SEPARATOR, LIST` — concatenate all elements of LIST into a single string, placing SEPARATOR between each pair of adjacent elements.\n\n`join` is the inverse of `split`. It stringifies each element before joining, so mixing numbers and strings is fine. An empty separator (`join \"\", @list`) concatenates without gaps. In stryke, `join` works naturally with `|>` pipelines: a range or filtered list can be piped directly into `join` with a separator argument. This is the standard way to build CSV lines, path strings, or human-readable lists from arrays. `join` never adds a trailing separator — if you need one, append it yourself.\n\n```perl\nmy $csv = join \",\", @fields\n1..5 |> join \"-\" |> p   # 1-2-3-4-5\nmy @parts = (\"usr\", \"local\", \"bin\")\np join \"/\", \"\", @parts   # /usr/local/bin\n```",
        "split" => "`split /PATTERN/, STRING [, LIMIT]` — divide STRING into a list of substrings by splitting on each occurrence of PATTERN.\n\n`split` is one of the most-used string functions. The PATTERN is a regex, so you can split on character classes, alternations, or lookaheads. A LIMIT caps the number of returned fields; the final field contains the unsplit remainder. The special pattern `\" \"` (a single space string, not regex) mimics `awk`-style splitting: it strips leading whitespace and splits on runs of whitespace. In stryke, `split` integrates with `|>` pipelines, accepting piped-in strings for ergonomic one-liners. Trailing empty fields are removed by default; pass -1 as LIMIT to preserve them.\n\n```perl\nmy @parts = split /,/, \"a,b,c\"\n\"one:two:three\" |> split /:/ |> e p   # one two three\nmy ($user, $domain) = split /@/, $email, 2\np \"$user at $domain\"\n```",
        "reverse" => "`reverse LIST` — in list context, return the elements in reverse order; in scalar context, reverse the characters of a string.\n\nThe dual nature of `reverse` is context-dependent: `reverse @array` flips element order, while `scalar reverse $string` (or just `reverse $string` in scalar context) reverses character order. In stryke, the string form is Unicode-aware, correctly reversing multi-byte characters rather than individual bytes. In `|>` pipelines, use the `t rev` shorthand for a concise streaming string reverse. `reverse` does not modify the original — it always returns a new list or string.\n\n```perl\np reverse \"hello\"       # olleh\nmy @r = reverse 1..5   # (5,4,3,2,1)\n@r |> e p               # 5 4 3 2 1\n\"abc\" |> t rev |> p     # cba\n```",
        "study" => "`study STRING` — hint to the regex engine that the given string will be matched against multiple patterns, allowing it to build an internal lookup table for faster matching.\n\nIn classic Perl, `study` builds a linked list of character positions so subsequent regex matches can skip impossible starting points. In practice, modern regex engines (including stryke's Rust-based engine) already perform these optimizations internally, so `study` is effectively a no-op in stryke — calling it is harmless but provides no measurable speedup. It exists for compatibility with Perl code that uses it. You only need `study` when porting legacy scripts that call it; do not add it to new stryke code expecting a performance benefit.\n\n```perl\nstudy $text   # no-op in stryke, kept for compatibility\nmy @hits = grep { /$pattern/ } @lines\np scalar @hits\n```",

        // ── Arrays & lists ──
        "push" => "`push @array, LIST` — appends one or more elements to the end of an array and returns the new length. This is the primary way to grow arrays in stryke and works identically to Perl's builtin. You can push scalars, lists, or even the result of a pipeline. In stryke, `push` is O(1) amortized thanks to the underlying Rust `Vec`.\n\n```perl\nmy @q\npush @q, 1..3\npush @q, \"four\", \"five\"\np scalar @q   # 5\n@q |> e p     # 1 2 3 four five\n```\n\nReturns the new element count, so `my $len = push @arr, $val;` is valid.",
        "pop" => "`pop @array` — removes and returns the last element of an array, or `undef` if the array is empty. This is the complement of `push` and together they give you classic stack (LIFO) semantics. In stryke the operation is O(1) because the underlying Rust `Vec::pop` simply decrements the length. When called without an argument inside a subroutine, `pop` operates on `@_`; at file scope it operates on `@ARGV`, matching standard Perl behavior.\n\n```perl\nmy @stk = 1..5\nmy $top = pop @stk\np $top          # 5\np scalar @stk   # 4\n@stk |> e p     # 1 2 3 4\n```",
        "shift" => "`shift @array` — removes and returns the first element of an array, sliding all remaining elements down by one index. Like `pop`, it returns `undef` on an empty array. Without an explicit argument it defaults to `@_` inside subroutines and `@ARGV` at file scope. Because every element must be moved, `shift` is O(n); if you only need LIFO access, prefer `push`/`pop`. Use `shift` when processing argument lists or implementing FIFO queues.\n\n```perl\nmy @args = @ARGV\nmy $cmd = shift @args\np $cmd\n@args |> e p   # remaining arguments\n```",
        "unshift" => "`unshift @array, LIST` — prepends one or more elements to the beginning of an array and returns the new length. It is the counterpart of `push` for the front of the array. Like `shift`, this is O(n) because existing elements must be moved to make room. When you need to build an array in reverse order or maintain a priority queue with newest items first, `unshift` is the idiomatic choice. You can pass multiple values and they will appear in the same order they were given.\n\n```perl\nmy @log = (\"b\", \"c\")\nunshift @log, \"a\"\n@log |> e p   # a b c\nmy $len = unshift @log, \"x\", \"y\"\np $len        # 5\n@log |> e p   # x y a b c\n```",
        "splice" => "`splice @array, OFFSET [, LENGTH [, LIST]]` — the Swiss-army knife for array mutation. It can insert, remove, or replace elements at any position in a single call. With just an offset it removes everything from that point to the end. With offset and length it removes that many elements. With a replacement list it inserts those elements in place of the removed ones. The return value is the list of removed elements, which is useful for saving what you cut. In stryke this compiles down to Rust's `Vec::splice` and `Vec::drain`.\n\n```perl\nmy @a = 1..5\nmy @removed = splice @a, 1, 2, 8, 9\n@a |> e p        # 1 8 9 4 5\n@removed |> e p  # 2 3\nsplice @a, 2     # remove from index 2 onward\n@a |> e p        # 1 8\n```",
        "sort" => "`sort [BLOCK] LIST` — returns a new list sorted in ascending order. Without a block, `sort` compares elements as strings (lexicographic). Pass a comparator block using `$_0` and `$_1` (stryke style) or `$a` and `$b` (classic Perl) to control ordering: use `<=>` for numeric and `cmp` for string comparison. The sort is stable in stryke, meaning equal elements preserve their original relative order. For descending order, reverse the operands in the comparator. Use `{ |$x, $y| body }` to name the two comparator params. Works naturally in `|>` pipelines.\n\n```perl\nmy @nums = (3, 1, 4, 1, 5)\nmy @asc = sort { $_0 <=> $_1 } @nums\n@asc |> e p   # 1 1 3 4 5\nsort { |$x, $y| $y <=> $x }, @nums   # named params\nmy @desc = sort { $_1 <=> $_0 } @nums\n@desc |> e p  # 5 4 3 1 1\n@nums |> sort |> e p   # string sort in pipeline\n```",
        "map" => "`map BLOCK LIST` — evaluates the block for each element of the list, setting `$_` to the current element, and returns a new list of all the block's return values. This is the eager version: it consumes the entire input list and produces the entire output list before returning. Use `map` when you need the full result array or when the input is small. For large or infinite sequences, prefer `maps` (the streaming variant). The block can return zero, one, or multiple values per element, making `map` useful for both transformation and flattening. Use `{ |$var| body }` to name the block parameter instead of `$_`.\n\n```perl\nmy @sq = map { $_ ** 2 } 1..5\n@sq |> e p   # 1 4 9 16 25\nmap { |$n| $n * $n }, 1..5   # named param\nmy @pairs = map { ($_, $_ * 10) } 1..3\n@pairs |> e p   # 1 10 2 20 3 30\n```",
        "maps" => "`maps { BLOCK } LIST` — the lazy, streaming counterpart of `map`. Instead of materializing the entire output list, `maps` returns a pull iterator that evaluates the block on demand as downstream consumers request values. This makes it ideal for `|>` pipeline chains, especially when combined with `take`, `greps`, or `collect`. Use `maps` over `map` when working with large ranges, infinite sequences, or when you want to short-circuit processing early with `take`. Memory usage is constant regardless of input size.\n\n```perl\n1..10 |> maps { $_ * 3 } |> take 4 |> e p\n# 3 6 9 12\n1..1_000_000 |> maps { $_ ** 2 } |> greps { $_ > 100 } |> take 3 |> e p\n```",
        "flat_maps" => "`flat_maps { BLOCK } LIST` — a lazy streaming flat-map that evaluates the block for each element and flattens the resulting lists into a single iterator. Where `maps` expects the block to return one value, `flat_maps` handles blocks that return zero or more values per element and concatenates them seamlessly. This is the streaming equivalent of calling `map` with a multi-value block. Use it in `|>` chains when each input element fans out into multiple outputs and you want lazy evaluation.\n\n```perl\n1..3 |> flat_maps { ($_, $_ * 10) } |> e p\n# 1 10 2 20 3 30\n@nested |> flat_maps { @$_ } |> e p   # flatten array-of-arrays\n```",
        "grep" => "`grep { BLOCK } LIST` — filters the list, returning only elements for which the block evaluates to true. The current element is available as `$_`. This is the eager version: it processes the entire list and returns a new list. It is Perl-compatible and works exactly like Perl's builtin `grep`. For streaming/lazy filtering in `|>` pipelines, use `greps` or `filter` instead. Use `{ |$var| body }` to name the block parameter instead of `$_`.\n\n```perl\nmy @evens = grep { $_ % 2 == 0 } 1..10\n@evens |> e p   # 2 4 6 8 10\ngrep { |$x| $x > 3 }, 1..6   # named param\nmy @long = grep { length($_) > 3 } @words\n```",
        "greps" => "`greps { BLOCK } LIST` — the lazy, streaming counterpart of `grep`. Returns a pull iterator that only evaluates the predicate as elements are requested downstream. This is the preferred filtering function in `|>` pipelines because it avoids materializing intermediate lists. Combine with `take` to short-circuit early, or with `maps` and `collect` for full lazy pipelines. The block receives `$_` just like `grep`.\n\n```perl\n1..100 |> greps { $_ % 7 == 0 } |> take 3 |> e p\n# 7 14 21\n@lines |> greps { /ERROR/ } |> maps { tm } |> e p\n```",
        "filter" | "fi" => "`filter` (alias `fi`) `{ BLOCK } LIST` — stryke-native lazy filter that returns a pull iterator, functionally identical to `greps` but named for familiarity with Rust/Ruby/JS conventions. Use `filter` or `greps` interchangeably in `|>` chains; both are streaming and both set `$_` in the block. The result must be consumed with `collect`, `e p`, `foreach`, or another terminal. Prefer `filter` when writing stryke-idiomatic code; prefer `grep`/`greps` when porting from Perl.\n\n```perl\nmy @big = 1..1000 |> filter { $_ > 990 } |> collect\n@big |> e p   # 991 992 993 994 995 996 997 998 999 1000\n1..50 |> fi { $_ % 2 } |> take 5 |> e p   # 1 3 5 7 9\n```",
        "compact" | "cpt" => "`compact` (alias `cpt`) — stryke-native streaming operator that removes `undef` and empty-string values from a list or iterator. This is a common data-cleaning step when dealing with parsed input, optional fields, or split results that produce empty segments. It is equivalent to `greps { defined($_) && $_ ne \"\" }` but more concise and faster because the check is inlined in Rust. Numeric zero and the string `\"0\"` are preserved since they are defined and non-empty.\n\n```perl\nmy @raw = (1, undef, \"\", 2, undef, 3)\n@raw |> compact |> e p   # 1 2 3\nsplit(/,/, \"a,,b,,,c\") |> cpt |> e p   # a b c\n```",
        "reject" => "`reject { BLOCK } LIST` — stryke-native streaming inverse of `filter`/`greps`. It keeps only the elements for which the block returns false. This reads more naturally than `filter { !(...) }` when the condition describes what you want to exclude rather than what you want to keep. Like `filter` and `greps`, it returns a lazy iterator suitable for `|>` chains. The block receives `$_` as the current element.\n\n```perl\n1..10 |> reject { $_ % 3 == 0 } |> e p\n# 1 2 4 5 7 8 10\n@files |> reject { /\\.bak$/ } |> e p   # skip backups\n```",
        "concat" | "chain" => "`concat` (alias `chain`) — stryke-native streaming operator that concatenates multiple lists or iterators into a single sequential iterator. Pass array references and they will be yielded in order without copying. This is useful for merging data from multiple sources into a unified pipeline. The operation is lazy: each source is drained in turn, so memory usage stays proportional to the largest single element, not the total.\n\n```perl\nmy @a = 1..3\nmy @b = 7..9\nconcat(\\@a, \\@b) |> e p   # 1 2 3 7 8 9\nmy @c = (\"x\")\nconcat(\\@a, \\@b, \\@c) |> maps { uc } |> e p\n```",
        "scalar" => "`scalar EXPR` — forces the expression into scalar context. The most common use is `scalar @array` to get the element count instead of the list of elements. In stryke, scalar context on an array returns its length as a Rust `usize`. You can also use `scalar` on a hash to get the number of key-value pairs, or on a function call to force it to return a single value. This is essential when you want a count inside string interpolation or as a function argument where list context would be ambiguous.\n\n```perl\nmy @items = (\"a\", \"b\", \"c\")\np scalar @items   # 3\np \"count: \" . scalar @items   # count: 3\nmy %h = (x => 1, y => 2)\np scalar %h       # 2\n```",
        "defined" => "`defined EXPR` — returns true if the expression has a value that is not `undef`. This is the canonical way to distinguish between \"no value\" and \"a value that happens to be false\" (such as `0`, `\"\"`, or `\"0\"`). Use `defined` before dereferencing optional values or checking return codes from functions that return `undef` on failure. In stryke, `defined` compiles to a Rust `Option::is_some()` check internally. Note that `defined` on an aggregate (hash or array) is deprecated in Perl 5 and is a no-op in stryke.\n\n```perl\nmy $x = undef\np defined($x) ? \"yes\" : \"no\"   # no\n$x = 0\np defined($x) ? \"yes\" : \"no\"   # yes\nmy $val = fn_that_may_fail()\np $val if defined $val\n```",
        "exists" => "`exists EXPR` — tests whether a specific key is present in a hash or an index is present in an array, regardless of whether the associated value is `undef`. This is different from `defined`: a key can exist but hold `undef`. Use `exists` to check hash membership before accessing a value to avoid autovivification side effects. In stryke, `exists` on a hash compiles to Rust's `HashMap::contains_key`, and on an array it checks bounds. You can also use it with nested structures: `exists $h{a}{b}` only checks the final level.\n\n```perl\nmy %h = (a => 1, b => undef)\np exists $h{b} ? \"yes\" : \"no\"   # yes (key present, value undef)\np exists $h{c} ? \"yes\" : \"no\"   # no\nmy @a = (10, 20, 30)\np exists $a[5] ? \"yes\" : \"no\"   # no\n```",
        "delete" => "`delete EXPR` — removes a key-value pair from a hash or an element from an array and returns the removed value (or `undef` if it did not exist). For hashes this is the only way to truly remove a key; assigning `undef` to `$h{key}` leaves the key in place. For arrays, `delete` sets the element to `undef` but does not shift indices (use `splice` if you need to close the gap). In stryke, hash deletion maps to Rust's `HashMap::remove`. You can delete multiple keys at once with a hash slice: `delete @h{@keys}`.\n\n```perl\nmy %h = (x => 1, y => 2, z => 3)\nmy $old = delete $h{x}\np $old   # 1\np exists $h{x} ? \"yes\" : \"no\"   # no\ndelete @h{\"y\", \"z\"}   # delete multiple keys\n```",
        "each" => "`each %hash` — returns the next (key, value) pair from a hash as a two-element list, or an empty list when the iterator is exhausted. Each hash has its own internal iterator, which is reset when you call `keys` or `values` on the same hash. This is memory-efficient for large hashes because it does not build the full key list. Gotcha: do not add or delete keys while iterating with `each`; that can cause skipped or duplicated entries. In stryke, the iteration order is non-deterministic (Rust `HashMap` order).\n\n```perl\nmy %h = (a => 1, b => 2, c => 3)\nwhile (my ($k, $v) = each %h) { p \"$k=$v\" }\n# output order varies: a=1 b=2 c=3\n```",
        "keys" => "`keys %hash` — returns the list of all keys in a hash in no particular order. When called on an array, it returns the list of valid indices (0 to `$#array`). In scalar context, `keys` returns the number of keys. Calling `keys` resets the `each` iterator on that hash, which is the standard way to restart iteration. In stryke, this calls Rust's `HashMap::keys()` and collects into a `Vec`. For sorted output, chain with `sort` via the pipe operator.\n\n```perl\nmy %env = (HOME => \"/root\", USER => \"me\")\nkeys(%env) |> sort |> e p   # HOME USER\np scalar keys %env           # 2\nmy @a = (10, 20, 30)\nkeys(@a) |> e p              # 0 1 2\n```",
        "values" => "`values %hash` — returns the list of all values in a hash in no particular order (matching the order of `keys` for the same hash state). When called on an array, it returns the array elements themselves. In scalar context, returns the count of values. Like `keys`, calling `values` resets the `each` iterator. In stryke this maps to Rust's `HashMap::values()`. Combine with `sum`, `sort`, or pipeline operators for common aggregation patterns.\n\n```perl\nmy %scores = (alice => 90, bob => 85)\np sum(values %scores)   # 175\nvalues(%scores) |> sort { $_1 <=> $_0 } |> e p   # 90 85\n```",
        "ref" => "`ref EXPR` — returns a string indicating the reference type of the value, or an empty string if it is not a reference. Common return values are `SCALAR`, `ARRAY`, `HASH`, `CODE`, `REF`, and `Regexp`. For blessed objects it returns the class name. Use `ref` to dispatch on data type or validate arguments in polymorphic functions. In stryke, `ref` inspects the Rust enum variant of the underlying value. Note that `ref` does not recurse; it only tells you the top-level type.\n\n```perl\nmy $r = [1, 2, 3]\np ref($r)       # ARRAY\np ref(\\%ENV)    # HASH\np ref(\\&main)   # CODE\np ref(42)       # (empty string)\n```",
        "undef" => "`undef` — the undefined value, representing the absence of a value. As a function, `undef $var` explicitly undefines a variable, freeing any value it held. `undef` is falsy in boolean context and triggers \"use of uninitialized value\" warnings under `use warnings`. In stryke, `undef` maps to Rust's `None` in an `Option` type internally. Use `undef` to reset variables, signal missing return values, or clear hash entries without deleting the key.\n\n```perl\nmy $x = 42\nundef $x\np defined($x) ? \"def\" : \"undef\"   # undef\nfn maybe { return undef if !@_\n    return $_[0] }\np defined(maybe()) ? \"got\" : \"nothing\"   # nothing\n```",
        "wantarray" => "`wantarray()` — returns true if the current subroutine was called in list context, false in scalar context, and `undef` in void context. This lets a function adapt its return value to the caller's expectations. A common pattern is returning a list in list context and a count or reference in scalar context. In stryke, use `fn` to define subroutines and `wantarray()` inside them just like in Perl. Note that `wantarray` only reflects the immediate call site, not nested contexts.\n\n```perl\nfn ctx { wantarray() ? \"list\" : \"scalar\" }\nmy @r = ctx()\np $r[0]   # list\nmy $r = ctx()\np $r      # scalar\nfn flexible { wantarray() ? (1, 2, 3) : 3 }\nmy @all = flexible()\np scalar @all   # 3\nmy $cnt = flexible()\np $cnt          # 3\n```",
        "caller" => "`caller [LEVEL]` — returns information about the calling subroutine's context. In list context it returns `(package, filename, line)` for the given call-stack level (default 0, the immediate caller). With higher levels you can walk up the call stack for debugging or generating stack traces. In scalar context, `caller` returns just the package name. In stryke, `caller` works with `fn`-defined subroutines and integrates with the runtime's frame tracking. This is invaluable for building custom error reporters or trace utilities.\n\n```perl\nfn trace {\n    my ($pkg, $f, $ln) = caller()\n    p \"$f:$ln\"\n}\ntrace()   # prints current file:line\nfn deep {\n    my ($pkg, $f, $ln) = caller(1)\n    p \"grandparent: $f:$ln\"\n}\n```",
        "pos" => "`pos SCALAR` — gets or sets the position where the next `m//g` (global match) will start searching in the given string. After a successful `m//g` match, `pos` returns the offset just past the end of the match. You can assign to `pos($s)` to manually reposition the search. If the last `m//g` failed, `pos` resets to `undef`. This is essential for writing lexers or tokenizers that consume a string incrementally with `\\G`-anchored patterns. In stryke, `pos` tracks per-scalar state just like Perl.\n\n```perl\nmy $s = \"abcabc\"\nwhile ($s =~ /a/g) { p pos($s) }\n# 1 4\npos($s) = 0   # reset to scan again\np pos($s)     # 0\n```",

        // ── List::Util & friends ──
        "all" => "`all { COND } @list` — returns true (1) if every element in the list satisfies the predicate, false (\"\") otherwise. The block receives each element as `$_` and should return a boolean. Short-circuits on the first failing element, so it never evaluates more than necessary. Works with `|>` pipelines and accepts bare lists or array variables.\n\n```perl\nmy @nums = 2, 4, 6, 8\np all { $_ % 2 == 0 } @nums   # 1\n1..100 |> all { $_ > 0 } |> p  # 1\n```",
        "any" => "`any { COND } @list` — returns true (1) if at least one element satisfies the predicate, false (\"\") if none do. The block receives each element as `$_`. Short-circuits on the first match, making it efficient even on large lists. This is the stryke equivalent of Perl's `List::Util::any` and can be used in `|>` pipelines.\n\n```perl\nmy @vals = 1, 3, 5, 8\np any { $_ > 7 } @vals   # 1\n1..1000 |> any { $_ == 42 } |> p  # 1\n```",
        "none" => "`none { COND } @list` — returns true (1) if no element satisfies the predicate, false (\"\") if any element matches. Logically equivalent to `!any { COND } @list` but reads more naturally in guard clauses. Short-circuits on the first match. Useful for validation checks where you want to assert the absence of a condition.\n\n```perl\nmy @words = (\"cat\", \"dog\", \"bird\")\np none { /z/ } @words   # 1\np \"all non-negative\" if none { $_ < 0 } @vals\n```",
        "first" => "`first { COND } @list` — returns the first element for which the block returns true, or `undef` if no element matches. The alias `fst` is also available. Short-circuits immediately upon finding a match, so only the minimum number of elements are tested. Ideal for searching sorted or unsorted lists when you need a single result.\n\n```perl\nmy $f = first { $_ > 10 } 3, 7, 12, 20\np $f   # 12\n1..1_000_000 |> first { $_ % 9999 == 0 } |> p  # 9999\n```",
        "min" => "`min @list` — returns the numerically smallest value from a list. Compares all elements using numeric (`<=>`) comparison, so stringy values are coerced to numbers. Returns `undef` for an empty list. In stryke, `min` is a built-in that does not require `use List::Util` and works directly in `|>` pipelines.\n\n```perl\np min(5, 3, 9, 1)   # 1\nmy @temps = (72.1, 68.5, 74.3)\n@temps |> min |> p  # 68.5\n```",
        "max" => "`max @list` — returns the numerically largest value from a list. Compares all elements using numeric (`<=>`) comparison. Returns `undef` for an empty list. Like `min`, this is a stryke built-in available without imports and works in `|>` pipelines. Combine with `map` to extract max values from complex structures.\n\n```perl\np max(5, 3, 9, 1)   # 9\n1..100 |> map { $_ ** 2 } |> max |> p  # 10000\n```",
        "sum" | "sum0" => "`sum @list` returns the numeric sum of all elements. Returns `undef` for an empty list. `sum0` is identical except it returns `0` for an empty list, which avoids the need for a fallback `// 0` guard. Both are stryke built-ins that work in `|>` pipelines. Use `sum0` in contexts where an empty input is expected and you need a safe numeric default.\n\n```perl\np sum(1..100)    # 5050\np sum0()          # 0\n@prices |> map { $_->{amount} } |> sum0 |> p\n```",
        "product" => "`product @list` — returns the product of all numeric elements in the list. Returns `undef` for an empty list. Useful for computing factorials, combinatoric products, and compound multipliers. In stryke this is a built-in that composes naturally with `|>` pipelines and `range`.\n\n```perl\np product(1..5)   # 120\nrange(1, 10) |> product |> p  # 3628800\n```",
        "reduce" => "`reduce { $_0 OP $_1 } @list` — performs a sequential left fold over the list. The first two elements are passed as `$_0` and `$_1` to the block; the result becomes `$_0` for the next iteration. The traditional `$a`/`$b` names are also supported for Perl compatibility. Use `{ |$acc, $val| body }` to name the two params. Returns `undef` for an empty list and the single element for a one-element list. For a fold with an explicit initial value, see `fold`.\n\n```perl\nmy $fac = reduce { $_0 * $_1 } 1..6\np $fac   # 720\nreduce { |$acc, $val| $acc + $val }, 1..10   # 55 (named params)\nmy $longest = reduce { length($_0) > length($_1) ? $_0 : $_1 } @words\n```",
        "fold" => "`fold { $_0 OP $_1 } INIT, @list` — left fold with an explicit initial accumulator value. The initial value is passed as the first `$_0`, and each list element arrives as `$_1`. Unlike `reduce`, `fold` never returns `undef` for an empty list — it returns the initial value instead. Both `$a`/`$b` and `$_0`/`$_1` are supported in the block. Use `fold` when you need a guaranteed starting point for the accumulation.\n\n```perl\nmy $total = fold { $_0 + $_1 } 100, 1..5\np $total   # 115\nmy $csv = fold { \"$_0,$_1\" } \"\", @fields\n```",
        "reductions" => "`reductions { $_0 OP $_1 } @list` — returns the running (cumulative) results of a left fold, also known as a scan or prefix-sum. Each element of the output is the accumulator state after processing the corresponding input element. The output list has the same length as the input. Both `$_0`/`$_1` and `$a`/`$b` naming conventions are supported.\n\n```perl\nmy @pfx = reductions { $_0 + $_1 } 1..4\n@pfx |> e p   # 1 3 6 10\n1..5 |> reductions { $_0 * $_1 } |> e p  # 1 2 6 24 120\n```",
        "mean" => "`mean @list` — returns the arithmetic mean (average) of a numeric list. Computed as `sum / count` in a single pass. Returns `undef` for an empty list. The result is always a floating-point value even if all inputs are integers. Combine with `map` to compute averages over extracted fields.\n\n```perl\np mean(2, 4, 6, 8)   # 5\n@students |> map { $_->{score} } |> mean |> p\n```",
        "median" => "`median @list` — returns the median value of a numeric list. For odd-length lists, this is the middle element after sorting. For even-length lists, it is the arithmetic mean of the two middle elements. The input list does not need to be pre-sorted. Returns `undef` for an empty list.\n\n```perl\np median(1, 3, 5, 7, 9)   # 5\np median(1, 3, 5, 7)      # 4\n1..100 |> median |> p     # 50.5\n```",
        "mode" => "`mode @list` — returns the most frequently occurring value in the list. If there is a tie, the value that appears first in the list wins. Returns `undef` for an empty list. Works with both numeric and string values — comparison is done by stringification. Useful for finding the dominant category in a dataset.\n\n```perl\np mode(1, 2, 2, 3, 3, 3)   # 3\nmy @logs = qw(INFO WARN INFO ERROR INFO)\np mode(@logs)  # INFO\n```",
        "stddev" | "std" => "`stddev @list` (alias `std`) — returns the population standard deviation of a numeric list. This is the square root of the population variance, measuring how spread out values are from the mean. Uses N (not N-1) in the denominator, so it computes the population statistic rather than the sample statistic. Returns `undef` for an empty list.\n\n```perl\np stddev(2, 4, 4, 4, 5, 5, 7, 9)   # 2\n1..10 |> std |> p\n```",
        "variance" => "`variance @list` — returns the population variance of a numeric list, computed as the mean of the squared deviations from the mean. Like `stddev`, this uses N (not N-1) as the divisor. The variance is `stddev ** 2`. Returns `undef` for an empty list. Useful for statistical analysis and as a building block for higher-order stats.\n\n```perl\np variance(2, 4, 4, 4, 5, 5, 7, 9)   # 4\nmy @samples = map { rand(100) } 1..1000\np variance(@samples)\n```",
        "sample" => "`sample N, @list` — returns a random sample of N elements drawn without replacement from the list. The returned elements are in random order. If N exceeds the list length, the entire list is returned (shuffled). Uses a Fisher-Yates partial shuffle internally for efficiency. Each call produces a different result due to random selection.\n\n```perl\nmy @pick = sample 3, 1..100\n@pick |> e p   # 3 random values\nmy @test_cases = sample 10, @all_cases\n```",
        "shuffle" => "`shuffle @list` — returns a new list with all elements in random order using a Fisher-Yates shuffle. The original list is not modified. Every permutation is equally likely. In stryke this is a built-in; no `use List::Util` is needed. The alias `shuf` is also available. Commonly used with `|>` and `take` to draw random subsets.\n\n```perl\nmy @deck = shuffle 1..52\n@deck |> take 5 |> e p   # 5 random cards\nmy @randomized = shuffle @questions\n```",
        "uniq" => "`uniq @list` — removes duplicate elements from a list, preserving the order of first occurrence. Comparison is done by string equality. The alias `uq` is also available. This is eager (not streaming) and returns a new list. For streaming deduplication in `|>` pipelines, use `distinct`. For type-specific comparison, see `uniqnum`, `uniqstr`, and `uniqint`.\n\n```perl\nmy @u = uniq 1, 2, 2, 3, 1, 3\n@u |> e p   # 1 2 3\nmy @hosts = uniq @all_hosts\n```",
        "uniqint" => "`uniqint @list` — removes duplicate elements comparing values as integers. Each element is truncated to its integer part before comparison, so `1.1` and `1.9` are considered equal (both become `1`). The first occurrence is kept. This is useful when you have floating-point data but care only about the integer portion for uniqueness.\n\n```perl\nmy @u = uniqint 1, 1.1, 1.9, 2\n@u |> e p   # 1 2\nmy @distinct_ids = uniqint @raw_ids\n```",
        "uniqnum" => "`uniqnum @list` — removes duplicate elements comparing values as floating-point numbers. Unlike `uniq` (which compares as strings), `uniqnum` treats `1.0` and `1.00` as equal because they have the same numeric value. The first occurrence of each numeric value is preserved. Use this when your data contains numbers that may have different string representations.\n\n```perl\nmy @u = uniqnum 1.0, 1.00, 2.5, 2.50\n@u |> e p   # 1 2.5\nmy @prices = uniqnum @all_prices\n```",
        "uniqstr" => "`uniqstr @list` — removes duplicate elements comparing values strictly as strings. This is the same comparison as `uniq` but makes the intent explicit. Numeric values `1` and `1.0` are considered different because their string representations differ. Use `uniqstr` when you want to be explicit that string semantics are intended.\n\n```perl\nmy @u = uniqstr \"a\", \"b\", \"a\", \"c\"\n@u |> e p   # a b c\nmy @tags = uniqstr @all_tags\n```",
        "zip" => "`zip(\\@a, \\@b, ...)` — combines multiple arrays element-wise into a list of arrayrefs. Each output arrayref contains one element from each input array at the corresponding index. Accepts two or more array references. The alias `zp` is also available. By default, `zip` pads shorter arrays with `undef` (equivalent to `zip_longest`). For truncating behavior, use `zip_shortest`.\n\n```perl\nmy @a = 1..3\nmy @b = (\"a\",\"b\",\"c\")\nzip(\\@a, \\@b) |> e p   # [1,a] [2,b] [3,c]\nmy @matrix = zip(\\@xs, \\@ys, \\@zs)\n```",
        "zip_longest" => "`zip_longest(\\@a, \\@b, ...)` — combines arrays element-wise, padding shorter arrays with `undef` to match the longest. This is the explicit version of the default `zip` behavior. Every input array contributes exactly one element per output tuple; missing elements become `undef`. Useful when you need to process all data from unequal-length sources.\n\n```perl\nmy @a = 1..3\nmy @b = (\"x\")\nzip_longest(\\@a, \\@b) |> e p   # [1,x] [2,undef] [3,undef]\n```",
        "zip_shortest" => "`zip_shortest(\\@a, \\@b, ...)` — combines arrays element-wise, stopping at the shortest input array. No `undef` padding is produced; the output length equals the minimum input length. Use this when you only want complete tuples and extra trailing elements should be discarded.\n\n```perl\nmy @a = 1..5\nmy @b = (\"x\",\"y\")\nzip_shortest(\\@a, \\@b) |> e p   # [1,x] [2,y]\nmy @paired = zip_shortest(\\@keys, \\@values)\n```",
        "mesh" => "`mesh(\\@a, \\@b, ...)` — interleaves multiple arrays into a flat list rather than arrayrefs. While `zip` returns `([1,\"a\"], [2,\"b\"])`, `mesh` returns `(1, \"a\", 2, \"b\")`. This makes it ideal for constructing hashes from parallel key and value arrays. The result is a flat list suitable for direct hash assignment.\n\n```perl\nmy @k = (\"a\",\"b\")\nmy @v = (1,2)\nmy %h = mesh(\\@k, \\@v)\np $h{a}   # 1\nmy %lookup = mesh(\\@ids, \\@names)\n```",
        "mesh_longest" => "`mesh_longest(\\@a, \\@b, ...)` — interleaves arrays into a flat list, padding shorter arrays with `undef` to match the longest. Like `mesh`, the output is flat (not arrayrefs). Missing elements become `undef` in the output sequence. Use this when building a flat interleaved list from arrays of unequal length where you need every element represented.\n\n```perl\nmy @a = 1..3\nmy @b = (\"x\")\nmy @r = mesh_longest(\\@a, \\@b)\n@r |> e p   # 1 x 2 undef 3 undef\n```",
        "mesh_shortest" => "`mesh_shortest(\\@a, \\@b, ...)` — interleaves arrays into a flat list, stopping at the shortest input array. No `undef` padding is produced; trailing elements from longer arrays are silently dropped. Use this when you only want complete interleaved groups and partial data should be discarded.\n\n```perl\nmy @a = 1..3\nmy @b = (\"x\",\"y\")\nmy @r = mesh_shortest(\\@a, \\@b)\n@r |> e p   # 1 x 2 y\n```",
        "chunked" => "`chunked N, @list` — splits a list into non-overlapping chunks of N elements each. Each chunk is an arrayref. The final chunk may contain fewer than N elements if the list length is not evenly divisible. The alias `chk` is also available. In stryke, `chunked` is eager and returns a list of arrayrefs; for streaming chunk behavior in pipelines, use `chunk`.\n\n```perl\nmy @ch = chunked 3, 1..7\n@ch |> e p   # [1,2,3] [4,5,6] [7]\n1..12 |> chunked 4 |> e { p join \",\", @$_ }\n```",
        "windowed" => "`windowed N, @list` — returns a sliding window of N consecutive elements over the list. Each window is an arrayref. The output contains `len - N + 1` windows. Unlike `chunked`, windows overlap: each successive window advances by one element. The alias `win` is also available. Useful for computing moving averages, detecting patterns in sequences, or n-gram extraction.\n\n```perl\nmy @w = windowed 3, 1..5\n@w |> e p   # [1,2,3] [2,3,4] [3,4,5]\nmy @deltas = windowed(2, @vals) |> map { $_->[1] - $_->[0] }\n```",
        "tail" | "tl" => "`tail N, @list` — returns the last N elements of the list. If N exceeds the list length, the entire list is returned. The alias `tl` is also available. This is the complement of `head`/`take` and mirrors Perl's `List::Util::tail`. In stryke, it works both as a function call and in `|>` pipelines.\n\n```perl\nmy @t = tail 2, 1..5\n@t |> e p   # 4 5\n1..100 |> tl 3 |> e p  # 98 99 100\n```",
        "pairs" => "`pairs @list` — takes a flat list and groups consecutive elements into pairs, returning a list of two-element arrayrefs `([$k, $v], ...)`. The input list must have an even number of elements. Each pair can be accessed via array indexing (`$_->[0]`, `$_->[1]`). This is the inverse of `unpairs` and is commonly used to iterate over hash-like flat lists in a structured way.\n\n```perl\nmy @p = pairs \"a\", 1, \"b\", 2\n@p |> e { p \"$_->[0]=$_->[1]\" }   # a=1 b=2\nmy @entries = pairs %hash\n```",
        "unpairs" => "`unpairs @list_of_pairs` — flattens a list of two-element arrayrefs back into a flat key-value list. This is the inverse of `pairs`. Each arrayref `[$k, $v]` becomes two consecutive elements in the output. Useful for converting structured pair data back into a format suitable for hash assignment or flat list processing.\n\n```perl\nmy @flat = unpairs [\"a\",1], [\"b\",2]\n@flat |> e p   # a 1 b 2\nmy %h = unpairs @filtered_pairs\n```",
        "pairkeys" => "`pairkeys @list` — extracts the keys (even-indexed elements) from a flat pairlist. Given a list like `(\"a\", 1, \"b\", 2, \"c\", 3)`, returns `(\"a\", \"b\", \"c\")`. This is equivalent to `map { $_->[0] } pairs @list` but more concise and efficient. Useful for extracting just the key side of a key-value flat list without constructing intermediate pair objects.\n\n```perl\nmy @k = pairkeys \"a\", 1, \"b\", 2, \"c\", 3\n@k |> e p   # a b c\nmy @config_pairs = (host => \"localhost\", port => 8080)\nmy @names = pairkeys @config_pairs\n```",
        "pairvalues" => "`pairvalues @list` — extracts the values (odd-indexed elements) from a flat pairlist. Given `(\"a\", 1, \"b\", 2)`, returns `(1, 2)`. This is equivalent to `map { $_->[1] } pairs @list` but more concise. Pair it with `pairkeys` to split a flat key-value list into separate key and value arrays.\n\n```perl\nmy @v = pairvalues \"a\", 1, \"b\", 2\n@v |> e p   # 1 2\nmy @defaults = (timeout => 30, retries => 3)\nmy @settings = pairvalues @defaults\n```",
        "pairmap" => "`pairmap BLOCK @list` — maps over consecutive pairs in a flat list, passing the key as `$_0` (or `$a`) and the value as `$_1` (or `$b`). The block can return any number of elements. This is the pair-aware equivalent of `map` and is ideal for transforming hash-like flat lists. The result is a flat list of whatever the block returns.\n\n```perl\nmy @out = pairmap { \"$_0=$_1\" } \"a\", 1, \"b\", 2\n@out |> e p   # a=1 b=2\nmy @cfg_pairs = (host => \"x\", port => 80)\nmy @upper = pairmap { uc($_0), $_1 } @cfg_pairs\n```",
        "pairgrep" => "`pairgrep { BLOCK } @list` — filters consecutive pairs from a flat list, keeping only those where the block returns true. The key is available as `$_0` (or `$a`) and the value as `$_1` (or `$b`). Returns a flat list of the matching key-value pairs. This is the pair-aware equivalent of `grep` and is useful for filtering hash-like data by both key and value simultaneously.\n\n```perl\nmy @big = pairgrep { $_1 > 5 } \"a\", 3, \"b\", 9, \"c\", 1\n@big |> e p   # b 9\nmy @alert_pairs = (info => 1, critical_a => 9, critical_b => 7)\nmy @important = pairgrep { $_0 =~ /^critical/ } @alert_pairs\n```",
        "pairfirst" => "`pairfirst { BLOCK } @list` — returns the first pair from a flat list where the block returns true, as a two-element list `($key, $value)`. The key is `$_0` (or `$a`) and the value is `$_1` (or `$b`). Short-circuits on the first match. Returns an empty list if no pair matches. This is the pair-aware equivalent of `first`.\n\n```perl\nmy @hit = pairfirst { $_1 > 5 } \"x\", 2, \"y\", 8\np \"@hit\"   # y 8\nmy @flags = (info => 1, debug => 7, trace => 0)\nmy ($k, $v) = pairfirst { $_0 eq \"debug\" } @flags\n```",

        // ── Functional list ops ──
        "flatten" | "fl" => "`flatten LIST` / `fl LIST` — recursively flatten nested arrayrefs into a single flat list.\n\nFlatten walks every element: scalars pass through unchanged, arrayrefs are opened and their contents are recursively flattened, so arbitrarily deep nesting is handled in one call. In pipeline mode (`|> fl`) it streams element-by-element, so you can chain it with `map`, `grep`, or `take` without materializing the entire intermediate list. The `fl` alias keeps pipeline chains concise.\n\n```perl\nmy @flat = flatten([1,[2,3]],[4])     # (1,2,3,4)\n[1,[2,[3,4]]] |> fl |> e p            # 1 2 3 4\n@nested |> fl |> grep { $_ > 0 } |> e p\n```",
        "distinct" => "`distinct LIST` — remove duplicate elements, preserving first-occurrence order (alias for `uniq`).\n\nEach element is compared as a string; the first time a value is seen it is emitted, and all subsequent occurrences are silently dropped. When used in a pipeline the deduplication state is maintained across streamed chunks, so `distinct` works correctly on lazy iterators and generators. This is a stryke built-in backed by a hash set internally, so it runs in O(n) time regardless of list size.\n\n```perl\nmy @u = distinct(3,1,2,1,3)           # (3,1,2)\n1,2,2,3,3,3 |> distinct |> e p        # 1 2 3\nstdin |> distinct |> e p              # unique lines\n```",
        "collect" => "`collect ITERATOR` — materialize a lazy iterator or pipeline into a concrete list.\n\nPipeline stages in stryke are lazy: chaining `|> map {...} |> grep {...}` builds up a deferred computation without consuming any elements. Calling `collect` forces evaluation and returns all results as a regular Perl list. This is the standard way to terminate a lazy chain when you need the full result in an array. Without `collect`, the iterator is consumed element-by-element by `e`, `take`, or other streaming sinks.\n\n```perl\nmy @out = range(1,5) |> map { $_ * 2 } |> collect\ngen { yield $_ for 1..3 } |> collect |> e p\nmy @data = stdin |> grep /INFO/ |> collect\n```",
        "drop" | "skip" | "drp" => "`drop N, LIST` / `skip N, LIST` / `drp N, LIST` — skip the first N elements and return the rest.\n\nThis operation is fully streaming: when used in a pipeline, the first N elements are consumed and discarded without buffering, and all subsequent elements flow through to the next stage. If the list contains fewer than N elements, the result is empty. The `skip` and `drp` aliases exist for readability in different contexts — all three compile to the same internal op.\n\n```perl\n1..10 |> drop 3 |> e p                # 4 5 6 7 8 9 10\nmy @rest = drp 2, @data\nstdin |> skip 1 |> e p                # skip header line\n```",
        "take" | "head" | "hd" => "`take N, LIST` / `head N, LIST` / `hd N, LIST` — return at most the first N elements.\n\nIn streaming mode, `take` pulls exactly N elements from the upstream iterator and then stops, so it short-circuits infinite or very large sources efficiently. This makes it safe to write `range(1, 1_000_000) |> take 5` without allocating a million-element list. If the source has fewer than N elements, all of them are returned. The `head` and `hd` aliases mirror Unix `head` semantics.\n\n```perl\n1..100 |> take 5 |> e p               # 1 2 3 4 5\nmy @top = hd 3, @sorted\nstdin |> hd 10 |> e p                 # first 10 lines\n```",
        "drop_while" => "`drop_while { COND } LIST` — skip leading elements while the predicate returns true, then emit everything after.\n\nThe block receives each element as `$_`. Once the predicate returns false for the first time, that element and all remaining elements pass through unconditionally — the predicate is never consulted again. This is streaming: elements are tested one at a time without buffering. Useful for skipping headers, preamble, or sorted prefixes in data streams.\n\n```perl\n1..10 |> drop_while { $_ < 5 } |> e p # 5 6 7 8 9 10\n@log |> drop_while { /^ #/ } |> e p    # skip comment header\n```",
        "skip_while" => "`skip_while { COND } LIST` — skip leading elements while the predicate is true (alias for `drop_while`).\n\nBehavior is identical to `drop_while`: once the predicate returns false, that element and all subsequent elements are emitted. The `skip_while` name is provided for users coming from Rust or Kotlin where this is the conventional name. Both compile to the same streaming operation internally.\n\n```perl\n1..10 |> skip_while { $_ < 5 } |> e p # 5 6 7 8 9 10\n@sorted |> skip_while { $_ le \"m\" } |> e p\n```",
        "take_while" => "`take_while { COND } LIST` — emit leading elements while the predicate returns true, then stop.\n\nThe block receives each element as `$_`. Elements are emitted as long as the predicate holds; the moment it returns false, the pipeline terminates immediately without consuming further input. This short-circuit behavior makes `take_while` efficient on infinite iterators and large streams. It is the complement of `drop_while`.\n\n```perl\n1..10 |> take_while { $_ < 5 } |> e p # 1 2 3 4\nstdin |> take_while { !/^END/ } |> e p\n```",
        "first_or" => "`first_or DEFAULT, LIST` — return the first element of the list, or DEFAULT if the list is empty.\n\nThis is a streaming terminal: it pulls exactly one element from the upstream iterator and returns it, or returns the default value if the iterator is exhausted. It never buffers the entire list. This is especially useful at the end of a `grep` or `map` pipeline where you need a safe fallback instead of `undef` when no match is found.\n\n```perl\nmy $v = first_or 0, @maybe_empty\nmy $x = grep { $_ > 99 } @nums |> first_or -1\nstdin |> grep /^ERROR/ |> first_or \"(none)\" |> p\n```",
        "lines" | "ln" => "`lines STRING` / `ln STRING` — split a string on newline boundaries, returning a streaming iterator of lines.\n\nEach line is yielded without the trailing newline character. When piped from `slurp`, this gives you a lazy line-by-line view of a file without loading all lines into memory at once. Both `\\n` and `\\r\\n` line endings are handled. The `ln` alias keeps pipelines compact.\n\n```perl\nslurp(\"data.txt\") |> lines |> e p\nmy @rows = lines $multiline_str\n$body |> ln |> grep /TODO/ |> e p\n```",
        "chars" | "ch" => "`chars STRING` / `ch STRING` — split a string into individual characters, returning a streaming iterator.\n\nEach Unicode grapheme cluster is yielded as a separate string element. This is useful for character-level processing such as frequency counting, transliteration, or building character n-grams. In pipeline mode the characters stream one at a time, so you can chain with `take`, `grep`, or `with_index` without materializing the full character array.\n\n```perl\n\"hello\" |> chars |> e p               # h e l l o\nmy @c = chars \"abc\"\n\"emoji: \\x{1F600}\" |> ch |> e p\n```",
        "stdin" => "`stdin` — return a streaming iterator over lines read from standard input.\n\nEach call to the iterator reads one line from STDIN, strips the trailing newline, and yields it. The iterator terminates at EOF. Because it is lazy, combining `stdin` with `take`, `grep`, or `first_or` processes only as many lines as needed — the rest of STDIN is left unconsumed. This is the idiomatic stryke way to build Unix-style filters.\n\n```perl\nstdin |> grep /error/i |> e p\nstdin |> take 5 |> e p\nstdin |> en |> e { p \"$_->[0]: $_->[1]\" }\n```",
        "trim" | "tm" => "`trim STRING` or `trim LIST` — strip leading and trailing ASCII whitespace.\n\nWhen given a single string, `trim` returns the stripped result. When given a list or used in a pipeline, it operates in streaming mode, trimming each element individually as it flows through. Whitespace includes spaces, tabs, carriage returns, and newlines. The `tm` alias is convenient in pipeline chains where brevity matters.\n\n```perl\n\" hello \" |> tm |> p                  # \"hello\"\n@raw |> tm |> e p\nslurp(\"data.csv\") |> ln |> tm |> e p\n```",
        "pluck" => "`pluck KEY, LIST_OF_HASHREFS` — extract a single key from each hashref in the list.\n\nFor each element, `pluck` dereferences it as a hashref and returns the value at the given key. Elements where the key is missing yield `undef`. This is a streaming operation: in a pipeline, each hashref is processed and the extracted value is emitted immediately. It is the stryke equivalent of `map { $_->{KEY} }` but more readable and optimized internally.\n\n```perl\n@users |> pluck \"name\" |> e p\nmy @ids = pluck \"id\", @records\n@rows |> pluck \"email\" |> distinct |> e p\n```",
        "grep_v" => "`grep_v PATTERN, LIST` — inverse grep: reject elements that match the pattern, keep the rest.\n\nThis is the complement of `grep` — any element where the pattern matches is dropped, and non-matching elements pass through. It accepts a regex, a string, or a code block as the pattern. In streaming mode, each element is tested and either forwarded or discarded without buffering. This is a stryke built-in that avoids the awkward `grep { !/pattern/ }` double-negation.\n\n```perl\n@words |> grep_v /^ #/ |> e p          # drop comments\nmy @clean = grep_v qr/tmp/, @files\nstdin |> grep_v /^\\s*$/ |> e p        # drop blank lines\n```",
        "with_index" | "wi" => "`with_index LIST` / `wi LIST` — pair each element with its 0-based index as `[$item, $index]`.\n\nEach element is wrapped in a two-element arrayref where `$_->[0]` is the original value and `$_->[1]` is its position. This is useful when you need positional information during a map or grep without maintaining a manual counter. Note the order is `[item, index]`, which differs from `enumerate` which yields `[index, item]`.\n\n```perl\nqw(a b c) |> wi |> e { p \"$_->[1]: $_->[0]\" }\n# 0: a  1: b  2: c\n@data |> wi |> grep { $_->[1] % 2 == 0 } |> e { p $_->[0] }\n```",
        "enumerate" | "en" => "`enumerate ITERATOR` / `en ITERATOR` — yield `[$index, $item]` pairs from a streaming iterator.\n\nEach element is wrapped as `[$index, $item]` where the index starts at 0 and increments for each element. Unlike `with_index` which returns `[item, index]`, `enumerate` uses the Rust/Python convention of `[index, item]`. This is a streaming operation: the index counter is maintained lazily as elements flow through the pipeline.\n\n```perl\nstdin |> en |> e { p \"$_->[0]: $_->[1]\" }\n1..5 |> en |> e { p \"$_->[0]: $_->[1]\" }\n@lines |> en |> grep { $_->[0] < 10 } |> e { p $_->[1] }\n```",
        "chunk" | "chk" => "`chunk N, ITERATOR` / `chk N, ITERATOR` — group elements into N-sized arrayrefs as they stream through.\n\nElements are buffered until N are collected, then the group is emitted as a single arrayref. The final chunk may contain fewer than N elements if the source is not evenly divisible. This is fully streaming: only one chunk is held in memory at a time, making it safe for large or infinite iterators. Use `chunk` for batching work (e.g., bulk database inserts) or formatting output into rows.\n\n```perl\n1..9 |> chk 3 |> e { p join \",\", @$_ }\n# 1,2,3  4,5,6  7,8,9\nstdin |> chk 100 |> e { bulk_insert(@$_) }\n```",
        "dedup" | "dup" => "`dedup ITERATOR` / `dup ITERATOR` — drop consecutive duplicate elements from a stream.\n\nOnly adjacent duplicates are removed: if the same value appears later after an intervening different value, it is emitted again. Comparison is string-based by default. This is a streaming operation that holds only the previous element in memory, so it works on infinite iterators. For global deduplication across the entire stream, use `distinct` instead.\n\n```perl\n1,1,2,2,3,1,1 |> dedup |> e p        # 1 2 3 1\n@sorted |> dedup |> e p              # like uniq(1)\nstdin |> dedup |> e p                 # collapse repeated lines\n```",
        "range" => "`range(START, END [, STEP])` — create a lazy integer iterator from START to END with an optional step.\n\nThe range is inclusive on both ends. When START > END and no step is given, stryke automatically counts downward. An explicit STEP controls the increment and must be negative when counting down, or the range will be empty. The iterator is fully lazy: no list is allocated, and elements are generated on demand, making `range(1, 1_000_000)` as cheap to create as `range(1, 5)`. Combine with `|>` to feed into streaming pipelines.\n\n```perl\nrange(1, 5) |> e p                    # 1 2 3 4 5\nrange(5, 1) |> e p                    # 5 4 3 2 1\nrange(0, 10, 2) |> e p                # 0 2 4 6 8 10\nrange(10, 0, -2) |> e p               # 10 8 6 4 2 0\n```",
        "tap" => "`tap { BLOCK } LIST` — execute a side-effecting block for each element, then pass the element through unchanged.\n\nThe return value of the block is ignored; the original element is always forwarded to the next pipeline stage. This makes `tap` ideal for injecting logging, debugging, or metrics collection into the middle of a pipeline without altering the data flow. It is fully streaming and preserves element order.\n\n```perl\n1..5 |> tap { log_debug \"saw: $_\" } |> map { $_ * 2 } |> e p\n@files |> tap { p \"processing: $_\" } |> map slurp |> e p\n```",
        "tee" => "`tee FILE, ITERATOR` — write each element to a file as a side effect while passing it through the pipeline.\n\nEvery element that flows through `tee` is appended as a line to the specified file, and the element itself continues downstream unchanged. The file is opened once on first element and closed when the iterator is exhausted. This is the stryke equivalent of the Unix `tee` command, useful for auditing or logging intermediate pipeline results to disk.\n\n```perl\n1..10 |> tee \"/tmp/log.txt\" |> map { $_ * 2 } |> e p\nstdin |> tee \"/tmp/raw.log\" |> grep /ERROR/ |> e p\n```",
        "nth" => "`nth N, LIST` — return the Nth element using 0-based indexing.\n\nWhen used in a pipeline, `nth` consumes and discards the first N elements, returns the next one, and stops — so it short-circuits on infinite iterators. On a plain list, it is equivalent to `$list[N]` but works as a function call for pipeline composition. Returns `undef` if the list has fewer than N+1 elements.\n\n```perl\nmy $third = nth 2, @data\n1..10 |> nth 4 |> p                   # 5\nstdin |> nth 0 |> p                   # first line\n```",
        "to_set" => "`to_set LIST` — collect a list or iterator into a `set` object with O(1) membership testing.\n\nThe resulting set contains only unique elements (duplicates are discarded). This is a terminal operation that materializes the full stream. The returned set supports `->contains($val)`, `->union($other)`, `->intersection($other)`, and `->difference($other)`. Use this when you need fast repeated lookups against a collection of values.\n\n```perl\nmy $s = 1..5 |> to_set\n@words |> to_set                      # deduplicated set\nmy $allowed = to_set @whitelist\np $allowed->contains(\"foo\")\n```",
        "to_hash" => "`to_hash LIST` — collect a flat list of key-value pairs into a Perl hash.\n\nThe list is consumed two elements at a time: odd-positioned elements become keys and even-positioned elements become values. If there is an odd number of elements, the last key maps to `undef`. This is a terminal operation that materializes the full stream. Useful for converting pipeline output into a lookup table.\n\n```perl\nmy %h = qw(a 1 b 2) |> to_hash\n@pairs |> to_hash\nmy %freq = @words |> map { $_, 1 } |> to_hash\n```",
        "set" => "`set LIST` — create a set (unique unordered collection) from the given elements.\n\nDuplicate values are collapsed on construction so the set contains each value exactly once. The set object provides `->contains($val)` for O(1) membership testing, plus `->union($s)`, `->intersection($s)`, `->difference($s)`, and `->len` methods. Internally backed by a Rust `HashSet` for performance. Use `to_set` to convert an existing iterator into a set.\n\n```perl\nmy $s = set(1, 2, 3, 2, 1)\np $s->contains(2)                     # 1\np $s->len                             # 3\nmy $both = $s->union(set(3, 4, 5))\n```",
        "deque" => "`deque LIST` — create a double-ended queue initialized with the given elements.\n\nA deque supports efficient O(1) insertion and removal at both ends via `->push_front($val)`, `->push_back($val)`, `->pop_front`, and `->pop_back`. It also supports `->len` and iteration. Internally backed by a Rust `VecDeque`, it is ideal for sliding window algorithms, BFS traversals, or any scenario where you need fast access to both ends of a sequence.\n\n```perl\nmy $dq = deque(1, 2, 3)\n$dq->push_front(0)\n$dq->push_back(4)\np $dq->pop_front                      # 0\n```",
        "heap" => "`heap LIST` — create a min-heap (priority queue) from the given elements.\n\nElements are heapified on construction so that `->pop` always returns the smallest element in O(log n) time. `->push($val)` inserts a new element, also in O(log n). The heap supports `->peek` to inspect the minimum without removing it, and `->len` for the current size. Internally backed by a Rust `BinaryHeap` (inverted for min-heap semantics), it is the go-to structure for top-K queries, Dijkstra, and merge-K-sorted-lists problems.\n\n```perl\nmy $h = heap(5, 3, 8, 1)\np $h->pop                             # 1 (smallest first)\np $h->peek                            # 3 (next smallest)\n$h->push(0)\n```",
        "peek" => "`peek ITERATOR` — inspect the next element of an iterator without consuming it.\n\nThe peeked value is buffered internally so that the next call to `->next` or pipeline pull returns the same element. This is useful for lookahead parsing, conditional branching on the next value, or implementing `take_while`-style logic manually. Calling `peek` multiple times without advancing the iterator returns the same value each time. Works with any stryke iterator including `gen`, `range`, `stdin`, and pipeline results.\n\n```perl\nmy $g = gen { yield $_ for 1..5 }\np peek $g                             # 1 (not consumed)\np $g->next                            # 1\np peek $g                             # 2\n```",

        // ── Parallel extensions (stryke) ──
        "pmap" => "Parallel `map` powered by rayon's work-stealing thread pool. Every element of the input list is processed concurrently across all available CPU cores, and the output order is guaranteed to match the input order. This is the primary workhorse for CPU-bound transforms in stryke — use it whenever you have a pure function and a large list. Pass `progress => 1` to get a live progress bar on STDERR for long-running jobs.\n\nTwo equivalent surface syntaxes:\n  • Block form — `pmap BLOCK LIST` — element bound to `$_`\n  • Bare-fn form — `pmap FUNC, LIST` — single-arg function name as first argument\n\n```perl\n# Block form\nmy @out = pmap { $_ * 2 } 1..1_000_000\nmy @hashes = pmap sha256 @blobs, progress => 1\n1..100 |> pmap { fetch(\"https://api.example.com/item/$_\") } |> e p\n\n# Bare-fn form (works for builtins and user-defined subs)\nmy @hashes = pmap sha256, @blobs, progress => 1\nsub double { $_0 * 2 }\nmy @r = pmap double, (1..1_000_000)\n```",
        "pmaps" => "Streaming parallel `map` — returns a lazy iterator that processes items across all CPU cores using a persistent worker-thread pool. Unlike `pmap` which eagerly collects all results into an array, `pmaps` yields results as they complete through a bounded channel, so downstream consumers see output immediately. Output order is non-deterministic (completion order). Each worker thread reuses a single interpreter instance, making it faster than `pmap` for large inputs.\n\nBest for:\n  • Pipelines with `take`/`head` — avoids processing the full list\n  • Very large inputs where holding all results in memory is impractical\n  • Streaming pipelines where you want progressive output\n\n```perl\nrange(0, 1e9) |> pmaps { $_ * 2 } |> take 10 |> ep\nt 1..1e6 pmaps { expensive($_) } ep\nrange(0, 1e6) |> pmaps { fetch(\"https://api/item/$_\") } |> pgreps { $_->{ok} } |> ep\n```",
        "pflat_maps" => "Streaming parallel flat-map — like `pmaps` but flattens array results. Returns a lazy iterator.\n\n```perl\nrange(0, 1e6) |> pflat_maps { [$_, $_ * 10] } |> ep\n```",
        "pmap_chunked" => "Parallel map that groups input into contiguous batches of N elements before distributing to threads. This reduces per-item scheduling overhead when the per-element work is very cheap (e.g. a few arithmetic ops). Each thread receives a slice of N consecutive items, processes them sequentially within the batch, then returns the batch result. Use this instead of `pmap` when profiling shows rayon overhead dominates the actual computation.\n\n```perl\nmy @out = pmap_chunked 100, { $_ ** 2 } 1..1_000_000\nmy @parsed = pmap_chunked 50, { json_decode } @json_strings\n```",
        "pgrep" => "Parallel `grep` that evaluates the filter predicate concurrently across all CPU cores using rayon. The result preserves the original input order, so it is a drop-in replacement for `grep` on large lists. Best suited for predicates that do meaningful work per element — if the predicate is trivial (e.g. a single regex on short strings), sequential `grep` may be faster due to lower scheduling overhead.\n\nTwo equivalent surface syntaxes: `pgrep { BLOCK } LIST` or `pgrep FUNC, LIST`.\n\n```perl\n# Block form\nmy @matches = pgrep { /complex_pattern/ } @big_list\nmy @primes = pgrep { is_prime } 2..1_000_000\n@files |> pgrep { -s $_ > 1024 } |> e p\n\n# Bare-fn form\nsub even { $_0 % 2 == 0 }\nmy @e = pgrep even, 1..10        # (2,4,6,8,10)\n```",
        "pgreps" => "Streaming parallel `grep` — returns a lazy iterator that filters items across all CPU cores. Unlike `pgrep` which eagerly collects all matching items, `pgreps` yields matches as they are found through a bounded channel. Output order is non-deterministic (completion order). Each worker thread reuses a single interpreter instance.\n\n```perl\nrange(0, 1e6) |> pgreps { is_prime($_) } |> take 100 |> ep\nt 1..1e9 pgreps { $_ % 7 == 0 } take 10 ep\n```",
        "pfor" => "Parallel `foreach` that executes a side-effecting block across all CPU cores with no return value. Use this when you need to perform work for each element (writing files, sending requests, updating shared state) but don't need to collect results. The block receives each element as `$_`. Iteration order is non-deterministic, so the block must be safe to run concurrently.\n\nTwo equivalent surface syntaxes: `pfor { BLOCK } LIST` or `pfor FUNC, LIST`.\n\n```perl\n# Block form\npfor { write_report } @records\npfor { compress_file } glob(\"*.log\")\n@urls |> pfor { fetch\n    p \"done: $_\" }\n\n# Bare-fn form\nsub work { print \"did $_0\\n\" }\npfor work, (1, 2, 3)\n```",
        "psort" => "Parallel sort that uses rayon's parallel merge-sort algorithm. Accepts an optional comparator block using `$_0`/`$_1` (or `$a`/`$b`). For large lists (10k+ elements), this significantly outperforms the sequential `sort` by splitting the array, sorting partitions in parallel, and merging. The sort is stable — equal elements retain their relative order.\n\n```perl\nmy @sorted = psort { $_0 <=> $_1 } @big_list\nmy @by_name = psort { $_0->{name} cmp $_1->{name} } @records\n@nums |> psort { $a <=> $b } |> e p\n```",
        "pcache" => "Parallel memoized map — each element is processed concurrently, but results are cached by the stringified value of `$_` so duplicate inputs are computed only once. This is ideal when your input list contains many repeated values and the computation is expensive. The cache is a concurrent hash map shared across all threads, so there is no lock contention on reads after the first computation.\n\nTwo equivalent surface syntaxes: `pcache { BLOCK } LIST` or `pcache FUNC, LIST`.\n\n```perl\n# Block form\nmy @out = pcache { expensive_lookup } @list_with_dupes\nmy @resolved = pcache { dns_resolve } @hostnames\n\n# Bare-fn form\nmy @resolved = pcache dns_resolve, @hostnames\n```",
        "preduce" => "Parallel tree-fold using rayon's `reduce` — splits the list into chunks, reduces each chunk independently, then merges partial results. The combining operation **must be associative** (e.g. `+`, `*`, `max`); non-associative ops will produce incorrect results. Much faster than sequential `reduce` on large numeric lists because the tree structure allows O(log n) merge depth across cores.\n\n```perl\nmy $total = preduce { $_0 + $_1 } @nums\nmy $biggest = preduce { $_0 > $_1 ? $_0 : $_1 } @vals\nmy $product = preduce { $a * $b } 1..100\n```",
        "preduce_init" => "Parallel fold with identity value.\n\n```perl\nmy $total = preduce_init 0, { $_0 + $_1 } @list\n```",
        "pmap_reduce" => "Fused parallel map + tree reduce.\n\n```perl\nmy $sum = pmap_reduce { $_*2 } { $_0 + $_1 } @list\n```",
        "pany" => "`pany { COND } @list` — parallel short-circuit `any`.",
        "pfirst" => "`pfirst { COND } @list` — parallel first matching element.",
        "puniq" => "`puniq @list` — parallel unique elements.",
        "pselect" => "`pselect(@channels)` — wait on multiple `pchannel` receivers.",
        "pflat_map" => "Parallel flat-map: map + flatten results. Each element produces zero or more output values via the block/function, and the outputs are concatenated in input order.\n\nTwo equivalent surface syntaxes: `pflat_map BLOCK LIST` or `pflat_map FUNC, LIST`.\n\n```perl\n# Block form\nmy @out = pflat_map expand @list\n\n# Bare-fn form\nsub expand { ($_0, $_0 * 10) }\nmy @r = pflat_map expand, (1, 2, 3)   # (1, 10, 2, 20, 3, 30)\n```",
        "pflat_map_on" => "Distributed parallel flat-map over `cluster`.",
        "fan" => "Execute BLOCK or FUNC N times in parallel (`$_`/`$_0` = index 0..N-1). With no count, defaults to the rayon pool size (`stryke -j`).\n\nTwo equivalent surface syntaxes:\n  • Block form — `fan N { BLOCK }` or `fan { BLOCK }`\n  • Bare-fn form — `fan N, FUNC` or `fan FUNC`\n\n```perl\n# Block form\nfan 8 { work($_) }\nfan { work($_) } progress => 1\n\n# Bare-fn form\nsub work { print \"tick $_0\\n\" }\nfan 8, work\nfan work, progress => 1   # uses pool size\n```",
        "fan_cap" => "Like `fan` but captures return values in index order. Two surface syntaxes: `fan_cap N { BLOCK }` or `fan_cap N, FUNC`.\n\n```perl\n# Block form\nmy @results = fan_cap 8 { compute($_) }\n\n# Bare-fn form\nsub compute { $_0 * $_0 }\nmy @squares = fan_cap 8, compute\n```",

        // ── Cluster / distributed ──
        "cluster" => "`cluster([\"host1:N\", \"host2:M\", ...])` — build an SSH-backed worker pool for distributing stryke workloads across multiple machines.\n\nEach entry in the list is a hostname (or `user@host`) followed by a colon and the number of worker slots to allocate on that host. Under the hood, stryke opens persistent SSH multiplexed connections to each host, deploys lightweight `stryke --remote-worker` processes, and manages a work-stealing scheduler across the entire cluster. The cluster object is then passed to distributed primitives like `pmap_on` and `pflat_map_on`. Workers must have `stryke` installed and accessible on `$PATH`. If a host becomes unreachable mid-computation, its in-flight tasks are automatically re-queued to surviving hosts.\n\n```perl\nmy $cl = cluster([\"server1:8\", \"server2:4\", \"gpu-box:16\"])\nmy @results = pmap_on $cl, { heavy_compute } @jobs\n\n# Single-machine cluster for testing:\nmy $local = cluster([\"localhost:4\"])\n```",
        "pmap_on" => "`pmap_on $cluster, { BLOCK } @list` — distributed parallel map that fans work across a `cluster` of remote machines.\n\nElements from `@list` are serialized, shipped to remote `stryke --remote-worker` processes over SSH, executed in parallel across every worker slot in the cluster, and the results are gathered back in input order. This is the distributed equivalent of `pmap`: same interface, same order guarantee, but the thread pool spans multiple hosts instead of local cores. Use this when a single machine's CPU count is the bottleneck. The block must be self-contained — it cannot close over local file handles or database connections, since it executes in a separate process on a remote host. Large closures are serialized once and cached on each worker for the lifetime of the cluster.\n\n```perl\nmy $cl = cluster([\"host1:8\", \"host2:8\"])\nmy @hashes = pmap_on $cl, { sha256(slurp) } @file_paths\nmy @results = pmap_on $cl, { fetch(\"https://api.example.com/$_\") } 1..10_000\n```",
        "ssh" => "`ssh($host, $command)` — execute a shell command on a remote host via SSH and return its stdout as a string.\n\nThis is a simple synchronous wrapper around an SSH invocation. The command is run in the remote user's default shell, and stdout is captured and returned. If the remote command exits non-zero, stryke dies with the stderr output. For bulk remote work, prefer `cluster` + `pmap_on` which manages connection pooling and parallelism automatically. `ssh` is best for one-off administrative commands, health checks, or bootstrapping a remote environment before building a cluster.\n\n```perl\nmy $uptime = ssh(\"server1\", \"uptime\")\np ssh(\"deploy@prod\", \"cat /etc/hostname\")\nmy $free = ssh(\"gpu-box\", \"nvidia-smi --query-gpu=memory.free --format=csv,noheader\")\n```",

        // ── Async / concurrency ──
        "async" => "`async { BLOCK }` — schedule a block for execution on a background worker thread and return a task handle immediately.\n\nThe block begins executing as soon as a thread is available in stryke's global rayon thread pool, while the calling code continues without blocking. To retrieve the result, pass the task handle to `await`, which blocks until the task completes and returns its value. If the block panics, the panic is captured and re-raised at the `await` call site. Use `async` for fire-and-forget background work, overlapping I/O with computation, or launching multiple independent tasks that you later join. For structured fan-out with index-based parallelism, prefer `fan` or `fan_cap` instead.\n\n```perl\nmy $task = async { long_compute() }\ndo_other_work()\nmy $val = await $task\n\n# Overlapping multiple fetches:\nmy @tasks = map { async { fetch(\"https://api.example.com/$_\") } } 1..10\nmy @results = map await @tasks\n```",
        "spawn" => "`spawn { BLOCK }` — Rust-style alias for `async`; schedules a block on a background thread and returns a joinable task handle.\n\nThis is identical to `async` in every respect — same thread pool, same semantics, same `await` for joining. The name exists for developers coming from Rust's `tokio::spawn` or `std::thread::spawn` who find `spawn` more natural. Use whichever reads better in your code; mixing `async` and `spawn` in the same program is perfectly fine since they share the same underlying pool.\n\n```perl\nmy $task = spawn { expensive_io() }\nmy $val = await $task\n\nmy @handles = map { spawn { process } } @items\nmy @out = map await @handles\n```",
        "await" => "`await $task` — block the current thread until an async/spawn task completes and return its result value.\n\nIf the background task has already finished by the time `await` is called, the result is returned immediately with no scheduling overhead. If the task panicked, `await` re-raises the panic as a die in the calling thread, preserving the original error message and backtrace. You can `await` a task exactly once; calling `await` on an already-joined handle is a fatal error. For waiting on multiple tasks, simply map over the handles — stryke does not yet provide a `join_all` primitive, but `map await @tasks` achieves the same effect.\n\n```perl\nmy $task = async { 42 }\nmy $result = await $task              # 42\n\n# Await with error handling:\nmy $t = spawn { die \"oops\" }\neval { await $t }                     # $@ eq \"oops\"\n```",
        "pchannel" => "`pchannel(N)` — create a bounded multi-producer multi-consumer (MPMC) channel with capacity N, returning a `($tx, $rx)` pair.\n\nThe transmitter `$tx` supports `->send($val)` which blocks if the channel is full (backpressure). The receiver `$rx` supports `->recv` which blocks until a value is available, and `->try_recv` which returns `undef` immediately if the channel is empty. Both ends can be cloned and shared across threads — clone `$tx` with `$tx->clone` to create additional producers, or clone `$rx` for additional consumers. When all transmitters are dropped, `->recv` on the receiver returns `undef` to signal completion. Use `pchannel` to build producer-consumer pipelines, rate-limited work queues, or to communicate between `async`/`spawn` tasks.\n\n```perl\nmy ($tx, $rx) = pchannel(100)\nasync { $tx->send($_) for 1..1000\n    undef $tx }\nwhile (defined(my $val = $rx->recv)) {\n    p $val\n}\n\n# Multiple producers:\nmy ($tx, $rx) = pchannel(50)\nfor my $i (1..4) {\n    my $t = $tx->clone\n    spawn { $t->send(\"from worker $i: $_\") for 1..100 }\n}\nundef $tx  # drop original so channel closes when workers finish\n```",
        "barrier" => "`barrier(N)` — create a synchronization barrier that blocks until exactly N threads have arrived at the wait point.\n\nEach thread calls `$b->wait` and is suspended until all N participants have reached the barrier, at which point all are released simultaneously. This is useful for coordinating phased parallel algorithms where all workers must complete step K before any worker begins step K+1. The barrier is reusable — after all threads are released, it resets and can be waited on again. Internally backed by a Rust `std::sync::Barrier` for zero-overhead synchronization.\n\n```perl\nmy $b = barrier(4)\nfor my $i (0..3) {\n    spawn {\n        setup_phase($i)\n        $b->wait                      # all 4 threads sync here\n        compute_phase($i)\n    }\n}\n```",
        "ppool" => "`ppool(N, fn { ... })` — create a persistent thread pool of N worker threads, each running the provided subroutine in a loop.\n\nThe pool is typically paired with a `pchannel`: workers pull items from the channel's receiver, process them, and optionally send results to an output channel. Unlike `pmap` which is a one-shot parallel transform, `ppool` keeps threads alive for the lifetime of the pool, making it ideal for long-running server-style workloads, background drain loops, or scenarios where thread startup cost would dominate short-lived `pmap` calls. Workers exit when their input channel is closed (all transmitters dropped). The pool object supports `->join` to block until all workers have finished.\n\n```perl\nmy ($tx, $rx) = pchannel(100)\nmy $pool = ppool 4, fn {\n    while (defined(my $job = $rx->recv)) {\n        process($job)\n    }\n}\n$tx->send($_) for @work_items\nundef $tx                             # signal completion\n$pool->join                           # wait for drain\n```",
        "pwatch" => "`pwatch(PATH, fn { ... })` — watch a file or directory for filesystem changes and invoke the callback on each event.\n\nThe watcher runs on a background thread using OS-native notifications (FSEvents on macOS, inotify on Linux) so it consumes near-zero CPU while idle. The callback receives the event type and affected path in `$_`. Directory watches are recursive by default. The watcher continues until the returned handle is dropped or the program exits. This is useful for building live-reload dev servers, file-triggered pipelines, or audit logs. Combine with `debounce` or a `pchannel` if the callback is expensive and rapid bursts of events need to be coalesced.\n\n```perl\nmy $w = pwatch \"./src\", fn {\n    p \"changed: $_\"\n    rebuild()\n}\n\n# Watch multiple paths:\nmy $w1 = pwatch \"/var/log/app.log\", fn { p \"log updated\" }\nmy $w2 = pwatch \"./config\", fn { reload_config() }\nsleep                                 # block forever\n```",

        // ── Pipeline / lazy iterators ──
        "pipeline" => "`pipeline(@list)` — wrap a list (or iterator) in a lazy pipeline object supporting chained `->map`, `->filter`, `->take`, `->skip`, `->flat_map`, `->tap`, and other transforms that execute zero work until a terminal method is called.\n\nNo intermediate lists are allocated between stages — each element flows through the full chain one at a time, making pipelines memory-efficient even on very large or infinite inputs. Terminal methods include `->collect` (materialize to list), `->reduce { ... }` (fold), `->for_each { ... }` (side-effect iteration), and `->count`. Pipelines compose naturally with stryke's `|>` operator: you can feed `pipeline(...)` output into further `|>` stages or vice versa. Use `pipeline` when you want explicit method-chaining style rather than the flat `|>` pipe syntax — both compile to the same lazy evaluation engine.\n\n```perl\nmy @out = pipeline(@data)\n  ->filter { $_ > 0 }\n  ->map { $_ * 2 }\n  ->take(10)\n  ->collect\n\npipeline(1..1_000_000)\n  ->filter { $_ % 3 == 0 }\n  ->map { $_ ** 2 }\n  ->take(5)\n  ->for_each { p $_ }                # 9 36 81 144 225\n\nmy $sum = pipeline(@scores)\n  ->filter { $_ >= 60 }\n  ->reduce { $_0 + $_1 }\n```",
        "par_pipeline" => "`par_pipeline(source => \\@data, stages => [...], workers => N)` — build a multi-stage parallel pipeline where each stage's map/filter block runs concurrently across N worker threads.\n\nUnlike `pmap` which parallelizes a single transform, `par_pipeline` lets you define a sequence of named stages — each with its own block — that execute in parallel while preserving input order in the final output. Internally, stryke partitions the source into chunks, distributes them across workers, and pipelines the stages so that stage 2 can begin processing a chunk as soon as stage 1 finishes it, overlapping computation across stages. This is ideal for multi-step ETL workloads where each step is CPU-bound and the data volume is large. The `workers` parameter defaults to the number of logical CPUs.\n\n```perl\nmy @results = par_pipeline(\n    source  => \\@raw_records,\n    stages  => [\n        { name => \"parse\",     map => fn { json_decode } },\n        { name => \"transform\", map => fn { enrich } },\n        { name => \"validate\",  filter => fn { $_->{valid} } },\n    ],\n    workers => 8,\n)\n```",
        "par_pipeline_stream" => "`par_pipeline_stream(source => ..., stages => [...], workers => N)` — streaming variant of `par_pipeline` that connects stages via bounded `pchannel` queues instead of materializing intermediate arrays.\n\nEach stage runs as an independent pool of workers, pulling from an input channel and pushing to an output channel. This gives true pipelined parallelism: stage 1 workers produce items while stage 2 workers consume them concurrently, bounded by channel capacity to prevent memory blowup. The streaming design makes this suitable for infinite or very large data sources (file streams, network feeds) where materializing the full dataset between stages is impractical. Results are emitted in arrival order by default; pass `ordered => 1` to reorder them to match input order at the cost of buffering.\n\n```perl\npar_pipeline_stream(\n    source  => fn { while (my $line = <STDIN>) { yield $line } },\n    stages  => [\n        { name => \"parse\", map => fn { json_decode } },\n        { name => \"score\", map => fn { compute_score } },\n    ],\n    workers => 4,\n    on_item => fn { p $_ },           # process results as they arrive\n)\n```",

        // ── Parallel I/O ──
        "par_lines" => "`par_lines PATH, { code }` — memory-map a file and scan its lines in parallel across all available CPU cores.\n\nThe file is `mmap`'d into memory rather than read sequentially, and line boundaries are detected in parallel chunks. Each line is passed to the callback as `$_`. Because the file is memory-mapped, there is no read-buffer overhead and the OS kernel pages data in on demand, making `par_lines` extremely efficient for multi-gigabyte log files or CSV data. Line order within the callback is not guaranteed (lines run in parallel), so the callback should be a self-contained side-effecting operation (accumulate into a shared structure via `pchannel`, write to a file, etc.) or use `par_lines` with a reducer. For ordered processing, use `read_lines` with `pmap` instead.\n\n```perl\npar_lines \"data.txt\", fn { process }\n\n# Count matching lines in a large log:\nmy $count = 0\npar_lines \"/var/log/syslog\", fn { $count++ if /ERROR/ }\np $count\n\n# Feed lines into a channel for downstream processing:\nmy ($tx, $rx) = pchannel(1000)\nasync { par_lines \"huge.csv\", fn { $tx->send($_) }\n    undef $tx }\n```",
        "par_walk" => "`par_walk PATH, { code }` — recursively walk a directory tree in parallel, invoking the callback for every file path found.\n\nDirectory traversal is parallelized using a work-stealing thread pool: multiple directories are read concurrently, and the callback fires as each file is discovered. The path is passed as `$_` (absolute). This is significantly faster than a sequential `find`-style walk on SSDs and networked filesystems where directory `readdir` latency dominates. Symlinks are not followed by default to avoid cycles. The walk visits files only — directories themselves are not passed to the callback unless you pass `dirs => 1`. Combine with `pmap` for a two-phase pattern: first collect paths with `par_walk`, then process file contents in parallel.\n\n```perl\npar_walk \"./src\", fn { p $_ if /\\.rs$/ }\n\n# Collect all JSON files under a directory:\nmy @json_files\npar_walk \"/data\", fn { push @json_files, $_ if /\\.json$/ }\np scalar @json_files\n\n# Parallel content search:\npar_walk \".\", fn {\n    if (/\\.log$/) {\n        my @hits = grep /FATAL/, rl\n        p \"$_: \", scalar @hits, \" fatals\" if @hits\n    }\n}\n```",
        "par_sed" => "`par_sed PATTERN, REPLACEMENT, @files` — perform an in-place regex substitution across multiple files in parallel.\n\nEach file is processed by a separate worker thread: the file is read into memory, all matches of PATTERN are replaced with REPLACEMENT, and the result is written back atomically (via a temp file + rename, so readers never see a partially-written file). This is the stryke equivalent of `sed -i` but parallelized across the file list — ideal for codebase-wide refactors, log scrubbing, or bulk config updates. The pattern uses stryke regex syntax (PCRE-style). Returns the total number of substitutions made across all files.\n\n```perl\npar_sed qr/oldFunc/, \"newFunc\", glob(\"src/*.pl\")\n\n# Case-insensitive replace across a project:\npar_sed qr/TODO/i, \"DONE\", par_walk(\".\", fn { $_ if /\\.rs$/ })\n\n# Scrub sensitive data from logs:\nmy $n = par_sed qr/\\b\\d{3}-\\d{2}-\\d{4}\\b/, \"XXX-XX-XXXX\", @log_files\np \"redacted $n occurrences\"\n```",
        "par_fetch" => "`par_fetch @urls` — fetch a list of URLs in parallel using async HTTP, returning an array of response bodies in input order.\n\nUnder the hood, stryke spawns concurrent HTTP GET requests across a connection pool with keep-alive and automatic retry on transient failures (5xx, timeouts). The degree of parallelism is bounded by the connection pool size (default 64) to avoid overwhelming the target server. For heterogeneous HTTP methods or custom headers, use `http_request` inside `pmap` instead. `par_fetch` is the right tool when you have a homogeneous list of URLs and just need the bodies — it handles connection reuse, DNS caching, and TLS session resumption automatically for maximum throughput.\n\n```perl\nmy @bodies = par_fetch @urls\n\n# Fetch and decode JSON in one shot:\nmy @data = par_fetch(@api_urls) |> map json_decode\n\n# Download pages with progress:\nmy @pages = par_fetch @urls, progress => 1\n```",
        "serve" => "Start a blocking HTTP server.\n\n```perl\nserve 8080, fn ($req) {\n    # $req = { method, path, query, headers, body, peer }\n    { status => 200, body => \"hello\" }\n}\n\nserve 3000, fn ($req) {\n    my $data = { name => \"stryke\", version => \"0.4\" }\n    { status => 200, body => json_encode($data) }\n}, { workers => 8 }$1\n\nHandler returns: hashref `{ status, body, headers }`, string (200 OK), or undef (404).\nJSON content-type auto-detected when body starts with `{` or `[`.",
        "par_csv_read" => "`par_csv_read @files` — read multiple CSV files in parallel, returning an array of parsed datasets (one per file).\n\nEach file is read and parsed by a separate worker thread using a fast Rust CSV parser that handles quoting, escaping, and UTF-8 correctly. Headers are auto-detected from the first row of each file, and each row is returned as a hashref keyed by header names. This is dramatically faster than sequential CSV parsing when you have many files — common in data engineering pipelines where data arrives as daily/hourly CSV partitions. For a single large CSV file, prefer `par_lines` with manual splitting, since `par_csv_read` parallelizes across files, not within a single file.\n\n```perl\nmy @datasets = par_csv_read glob(\"data/*.csv\")\nfor my $ds (@datasets) {\n    p scalar @$ds, \" rows\"\n}\n\n# Merge all CSVs into one list:\nmy @all_rows = par_csv_read(@files) |> flat\np $all_rows[0]->{name}                # access by header\n\n# Filter and aggregate:\nmy @sales = par_csv_read(glob(\"sales_*.csv\")) |> flat\n  |> grep { $_->{region} eq \"US\" }\n```",

        // ── Typing (stryke) ──
        "typed" => "`typed` adds optional runtime type annotations to lexical variables and subroutine parameters. When a `typed` declaration is in effect, stryke inserts a lightweight check at assignment time that verifies the value matches the declared type (`Int`, `Str`, `Float`, `Bool`, `ArrayRef`, `HashRef`, or a user-defined `struct` name). This is especially useful for catching accidental type mismatches at function boundaries in larger programs. The annotation is purely a runtime guard — it has zero impact on pipeline performance because the check is only performed once at the point of assignment, not on every read.\n\n```perl\ntyped my $x : Int = 42\ntyped my $name : Str = \"hello\"\ntyped my $pi : Float = 3.14\nmy $add = fn ($a: Int, $b: Int) { $a + $b }\np $add->(3, 4)   # 7\n```\n\nNote: assigning a value of the wrong type raises a runtime exception immediately.\n\nYou can mix typed and untyped variables freely in the same scope, so adopting `typed` is incremental — annotate the variables that matter and leave the rest dynamic. Subroutine parameters declared with type annotations in `fn` are checked on every call, giving you contract-style validation at function boundaries without a separate assertion library.\n\n```perl\ntyped my @nums : Int = (1, 2, 3)\ntyped my %cfg : Str = (host => \"localhost\", port => \"8080\")\n```",
        "struct" => "`struct` declares a named record type with typed fields, giving stryke lightweight struct semantics similar to Rust structs or Python dataclasses. Structs support multiple construction syntaxes, default values, field mutation, user-defined methods, functional updates, and structural equality.\n\n**Declaration:**\n```perl\nstruct Point { x => Float, y => Float }           # typed fields\nstruct Point { x => Float = 0.0, y => Float = 0.0 } # with defaults\nstruct Pair { key, value }                        # untyped (Any)\n```\n\n**Construction:**\n```perl\nmy $p = Point(x => 1.5, y => 2.0)  # function-call with named args\nmy $p = Point(1.5, 2.0)            # positional (declaration order)\nmy $p = Point->new(x => 1.5, y => 2.0) # traditional OO style\nmy $p = Point()                    # uses defaults if defined\n```\n\n**Field access (getter/setter):**\n```perl\np $p->x       # getter (0 args)\n$p->x(3.0)    # setter (1 arg)\n```\n\n**User-defined methods:**\n```perl\nstruct Circle {\n    radius => Float,\n    fn area { 3.14159 * $self->radius ** 2 }\n    fn scale($factor: Float) {\n        Circle(radius => $self->radius * $factor)\n    }\n}\nmy $c = Circle(radius => 5)\np $c->area        # 78.53975\np $c->scale(2)    # Circle(radius => 10)\n```\n\n**Built-in methods:**\n```perl\nmy $q = $p->with(x => 5)  # functional update — new instance\nmy $h = $p->to_hash       # { x => 1.5, y => 2.0 }\nmy @f = $p->fields        # (x, y)\nmy $c = $p->clone         # deep copy\n```\n\n**Smart stringify:**\n```perl\np $p  # Point(x => 1.5, y => 2)\n```\n\n**Structural equality:**\n```perl\nmy $a = Point(1, 2)\nmy $b = Point(1, 2)\np $a == $b  # 1 (compares all fields)\n```\n\nNote: field type is checked at construction and mutation; unknown field names are fatal errors.",

        // ── Classes (full OOP) ──
        "class" => "`class` declares a full object-oriented class with typed fields, inheritance, traits, instance methods, and static methods. Classes provide modern OOP semantics with a clean syntax.\n\n**Declaration:**\n```perl\nclass Animal {\n    name: Str\n    age: Int = 0\n    fn speak { p \"Animal: \" . $self->name }\n}\n```\n\n**Inheritance with `extends`:**\n```perl\nclass Dog extends Animal {\n    breed: Str = \"Mixed\"\n    fn bark { p \"Woof! I am \" . $self->name }\n    fn speak { p $self->name . \" barks!\" }  # override\n}\n```\n\n**Construction (named or positional):**\n```perl\nmy $dog = Dog(name => \"Rex\", age => 5, breed => \"Lab\")\nmy $dog = Dog(\"Rex\", 5, \"Lab\")  # positional\n```\n\n**Field access (getter/setter):**\n```perl\np $dog->name      # getter\n$dog->age(6)      # setter\n```\n\n**Static methods (`fn Self.name`):**\n```perl\nclass Math {\n    fn Self.add($a, $b) { $a + $b }\n    fn Self.pi { 3.14159 }\n}\np Math::add(3, 4)  # 7\np Math::pi()       # 3.14159\n```\n\n**Traits (interfaces):**\n```perl\ntrait Printable { fn to_str }\nclass Item impl Printable {\n    name: Str\n    fn to_str { $self->name }\n}\n```\n\n**Multiple inheritance:**\n```perl\nclass C extends A, B { }\n```\n\n**isa checks:**\n```perl\np $dog->isa(\"Dog\")    # 1\np $dog->isa(\"Animal\") # 1 (parent)\np $dog->isa(\"Cat\")    # \"\" (false)\n```\n\n**Built-in methods:**\n```perl\nmy @f = $dog->fields()        # (name, age, breed)\nmy $h = $dog->to_hash()       # { name => \"Rex\", ... }\nmy $d2 = $dog->with(age => 1) # functional update\nmy $d3 = $dog->clone()        # deep copy\n```\n\n**Visibility (pub/priv):**\n```perl\nclass Secret {\n    pub visible: Int = 1\n    priv hidden: Int = 42\n    pub fn get_hidden { $self->hidden }  # internal access ok\n}\n```\n\nNote: inherited fields come first in the values array; method lookup walks the inheritance chain.",

        "trait" => "`trait` declares an interface that classes can implement via `impl`. Traits define method signatures that implementing classes must provide.\n\n**Declaration:**\n```perl\ntrait Printable {\n    fn to_str          # required (no body)\n    fn debug { ... }   # optional default impl\n}\n```\n\n**Implementation:**\n```perl\nclass Item impl Printable {\n    name: Str\n    fn to_str { $self->name }\n}\n```\n\n**Multiple traits:**\n```perl\nclass Widget impl Printable, Comparable {\n    ...\n}\n```\n\nNote: trait method bodies provide default implementations; classes can override them.",

        "extends" => "`extends` specifies parent classes for inheritance. Child classes inherit all fields and methods from parents.\n\n```perl\nclass Animal { name: Str }\nclass Dog extends Animal { breed: Str }\nclass Hybrid extends Dog, Cat { }  # multiple inheritance\n```\n\nInherited fields appear first in construction order. Methods are resolved child-first (override pattern).",

        "impl" => "`impl` declares which traits a class implements.\n\n```perl\ntrait Printable { fn to_str }\nclass Item impl Printable {\n    name: Str\n    fn to_str { $self->name }\n}\n```\n\nMultiple traits: `class X impl A, B, C { }`",

        "pub" => "`pub` marks a field or method as public (default visibility).\n\n```perl\nclass Example {\n    pub name: Str       # explicitly public\n    pub fn greet { }    # explicitly public\n}\n```",

        "priv" => "`priv` marks a field or method as private. Private members can only be accessed from within the class.\n\n```perl\nclass Secret {\n    priv hidden: Int = 42\n    priv fn internal { }\n    pub fn reveal { $self->hidden }  # internal access ok\n}\n# $obj->hidden  # ERROR: field is private\n```",

        // ── Data encoding / codecs ──
        "json_encode" => "`json_encode` serializes any Perl data structure — hashrefs, arrayrefs, nested combinations, numbers, strings, booleans, and undef — into a compact JSON string. It uses a fast Rust-backed serializer so it is significantly faster than `JSON::XS` for large payloads. The output is always valid UTF-8 JSON suitable for writing to files, sending over HTTP, or piping to other tools. Use `json_decode` to round-trip back.\n\n```perl\nmy %cfg = (debug => 1, paths => [\"/tmp\", \"/var\"])\nmy $j = json_encode(\\%cfg)\np $j   # {\"debug\":1,\"paths\":[\"/tmp\",\"/var\"]}\n$j |> spurt \"/tmp/cfg.json\"$1\n\nNote: undef becomes JSON `null`; Perl booleans serialize as `true`/`false`.",
        "json_decode" => "`json_decode` parses a JSON string and returns the corresponding Perl data structure — hashrefs for objects, arrayrefs for arrays, and native scalars for strings/numbers/booleans. It is strict by default: malformed JSON raises an exception rather than returning partial data. This makes it safe to use in pipelines where corrupt input should halt processing. The Rust parser underneath handles large documents efficiently and supports full Unicode.\n\n```perl\nmy $data = json_decode('{\"name\":\"stryke\",\"ver\":1}')\np $data->{name}   # stryke\nslurp(\"data.json\") |> json_decode |> dd$1\n\nNote: JSON `null` becomes Perl `undef`; trailing commas and comments are not allowed.",
        "stringify" | "str" => "`stringify` (alias `str`) converts any stryke value — scalars, array refs, hash refs, nested structures, undef — into a string representation that is a valid stryke literal. The output is designed for round-tripping: you can `eval` the returned string to reconstruct the original data structure. This makes it ideal for serializing state to a file in a Perl-native format, generating code fragments, or building reproducible test fixtures. Unlike `dd`, which targets human readability, `str` prioritizes parseability.\n\n```perl\nmy $s = str {a => 1, b => [2, 3]}\np $s               # {a => 1, b => [2, 3]}\nmy $copy = eval $s # round-trip back to hashref\nmy @list = (1, \"hello\", undef)\np str \\@list        # [1, \"hello\", undef]\n```\n\nNote: references are serialized recursively; circular references will cause infinite recursion.",
        "ddump" | "dd" => "`ddump` (alias `dd`) pretty-prints any stryke data structure to stderr in a human-readable, indented format similar to Perl's `Data::Dumper`. It is the go-to tool for quick debugging — drop a `dd` call anywhere in a pipeline to inspect intermediate values without disrupting the data flow. The output is colorized when stderr is a terminal. Unlike `str`, the output is not meant for `eval` round-tripping; it prioritizes clarity over parseability. `dd` returns its argument unchanged, so it can be inserted into pipelines transparently.\n\n```perl\nmy %h = (name => \"Alice\", scores => [98, 87, 95])\ndd \\%h                        # pretty-prints to stderr\nmy @result = @data |> dd |> grep { $_->{active} } |> dd\ndd [1, {a => 2}, [3, 4]]     # nested structure\n```\n\nNote: `dd` writes to stderr, not stdout, so it never contaminates pipeline output.",
        "to_json" | "tj" => "`to_json` (alias `tj`) converts a stryke data structure into a JSON string, functioning as a convenient shorthand for `json_encode`. It accepts hashrefs, arrayrefs, scalars, and nested combinations, producing compact JSON output suitable for APIs, config files, or inter-process communication. The alias `tj` is particularly useful at the end of a pipeline to serialize the final result. The Rust-backed serializer handles large structures efficiently and always produces valid UTF-8.\n\n```perl\nmy %user = (name => \"Bob\", age => 30)\np tj \\%user   # {\"age\":30,\"name\":\"Bob\"}\nmy @items = map { {id => $_, val => $_ * 2} } 1..3\np tj \\@items  # [{\"id\":1,\"val\":2},{\"id\":2,\"val\":4},{\"id\":3,\"val\":6}]\n@data |> tj |> spurt \"out.json\"\n```",
        "to_csv" | "tc" => "`to_csv` (alias `tc`) serializes a list of hashrefs or arrayrefs into a CSV-formatted string, complete with a header row derived from hash keys when given hashrefs. This is the fastest way to produce spreadsheet-ready output from structured data. Fields containing commas, quotes, or newlines are automatically escaped according to RFC 4180. The alias `tc` keeps one-liners terse when piping query results or API responses straight to CSV.\n\n```perl\nmy @rows = ({name => \"Alice\", age => 30}, {name => \"Bob\", age => 25})\np tc \\@rows   # name,age\\nAlice,30\\nBob,25\ntc(\\@rows) |> spurt \"people.csv\"\nmy @grid = ([1, 2, 3], [4, 5, 6])\np tc \\@grid   # 1,2,3\\n4,5,6\n```",
        "to_toml" | "tt" => "`to_toml` (alias `tt`) serializes a stryke hashref into a TOML-formatted string. TOML is a popular configuration format that maps cleanly to hash structures, making `tt` ideal for generating config files programmatically. Nested hashes become TOML sections, arrays become TOML arrays, and scalar values are serialized with their natural types. The output is always valid TOML that can be parsed back with `toml_decode`.\n\n```perl\nmy %cfg = (database => {host => \"localhost\", port => 5432}, debug => 1)\np tt \\%cfg\n# [database]\n# host = \"localhost\"\n# port = 5432\n# debug = 1\ntt(\\%cfg) |> spurt \"config.toml\"\n```",
        "to_yaml" | "ty" => "`to_yaml` (alias `ty`) serializes a stryke data structure into a YAML-formatted string. YAML is widely used for configuration and data exchange where human readability matters. Nested structures are represented with indentation, arrays with leading dashes, and strings are quoted only when necessary. The output is valid YAML 1.2 that round-trips cleanly through `yaml_decode`. The alias `ty` is convenient for quick inspection of complex data.\n\n```perl\nmy %app = (name => \"myapp\", deps => [\"tokio\", \"serde\"], version => \"1.0\")\np ty \\%app\n# name: myapp\n# version: \"1.0\"\n# deps:\n#   - tokio\n#   - serde\nty(\\%app) |> spurt \"app.yaml\"\n```",
        "to_xml" | "tx" => "`to_xml` (alias `tx`) serializes a stryke data structure into an XML string. Hash keys become element names, array values become repeated child elements, and scalar values become text content. This is useful for generating XML payloads for SOAP APIs, RSS feeds, or configuration files that require XML format. The output is well-formed XML that can be parsed back with `xml_decode`.\n\n```perl\nmy %doc = (root => {title => \"Hello\", items => [\"a\", \"b\", \"c\"]})\np tx \\%doc\n# <root><title>Hello</title><items>a</items><items>b</items><items>c</items></root>\ntx(\\%doc) |> spurt \"doc.xml\"\n```",
        "to_html" | "th" => "`to_html` (alias `th`) serializes a stryke data structure into a self-contained HTML document with cyberpunk styling (dark background, neon cyan/magenta accents, monospace fonts). Arrays of hashrefs render as full tables with headers, plain arrays as bullet lists, single hashes as key-value tables, and scalars as styled text blocks. Pipe to a file and open in a browser for instant data visualization.\n\n```perl\nfr |> map +{name => $_, size => format_bytes(size)} |> th |> to_file(\"report.html\")\nmy @rows = ({name => \"Alice\", age => 30}, {name => \"Bob\", age => 25})\n@rows |> th |> to_file(\"people.html\")\nth({host => \"localhost\", port => 5432}) |> p\n```",
        "to_markdown" | "to_md" | "tmd" => "`to_markdown` (aliases `to_md`, `tmd`) serializes a stryke data structure into Markdown text. Arrays of hashrefs render as GFM tables with headers and separator rows, plain arrays as bullet lists, single hashes as 2-column key-value tables, and scalars as plain text. The output is valid GitHub-Flavored Markdown suitable for README files, issue comments, or any Markdown renderer.\n\n```perl\nfr |> map +{name => $_, size => format_bytes(size)} |> tmd |> to_file(\"report.md\")\nmy @rows = ({name => \"Alice\", age => 30}, {name => \"Bob\", age => 25})\n@rows |> tmd |> p\n# | name | age |\n# | --- | --- |\n# | Alice | 30 |\n# | Bob | 25 |\n```",
        "xopen" | "xo" => "`xopen` (alias `xo`) opens a file or URL with the system default handler — `open` on macOS, `xdg-open` on Linux, `start` on Windows. Returns the path unchanged so it can sit transparently in a pipeline. This is the missing link between generating output files and viewing them: pipe the path through `xopen` and the OS opens it in the appropriate app (browser for HTML, viewer for PDF, editor for text).\n\n```perl\nfr |> map +{name => $_, size => format_bytes(size)} |> th |> to_file(\"report.html\") |> xopen\nxopen \"https://github.com/MenkeTechnologies/stryke\"\nxopen \"output.pdf\"\n```",
        "clip" | "clipboard" | "pbcopy" => "`clip` (aliases `clipboard`, `pbcopy`) copies text to the system clipboard (`pbcopy` on macOS, `xclip`/`xsel` on Linux). Returns the text unchanged for pipeline chaining. This is the missing link between generating output and sharing it — pipe any serializer's output through `clip` and paste directly into Slack, GitHub, docs, or email.\n\n```perl\nfr |> map +{name => $_, size => format_bytes(size)} |> tmd |> clip   # markdown table → clipboard\nqw(a b c) |> join \",\" |> clip |> p                                  # also prints\n\"some text\" |> clip                                                  # just copy\n```",
        "paste" | "pbpaste" => "`paste` (alias `pbpaste`) reads text from the system clipboard (`pbpaste` on macOS, `xclip`/`xsel` on Linux). Returns the clipboard contents as a string, ready for pipeline processing.\n\n```perl\np paste                              # print clipboard contents\npaste |> lines |> grep /error/i |> p  # search clipboard for errors\npaste |> wc |> p                      # word count of clipboard\n```",
        "to_table" | "table" | "tbl" => "`to_table` (aliases `table`, `tbl`) renders data as a plain-text column-aligned table with Unicode box-drawing borders. Arrays of hashrefs render as full tables with headers, plain arrays as numbered rows, single hashes as key-value tables. The output is fixed-width text suitable for terminal display, log files, or any monospace context.\n\n```perl\nmy @r = ({name => \"Alice\", age => 30}, {name => \"Bob\", age => 25})\n@r |> tbl |> p\n# ┌───────┬─────┐\n# │ name  │ age │\n# ├───────┼─────┤\n# │ Alice │ 30  │\n# │ Bob   │ 25  │\n# └───────┴─────┘\nfr |> map +{name => $_, size => format_bytes(size)} |> tbl |> p\n```",
        "from_json" | "fj" => "`from_json` (alias `fj`) parses a JSON string into a stryke data structure (hashref, arrayref, scalar). This is the inverse of `to_json` and the idiomatic way to decode JSON responses from APIs, config files, or inter-process communication. The Rust-backed parser handles large documents efficiently.\n\n```perl\nmy $json = '{\"name\":\"Alice\",\"age\":30}'\nmy $data = from_json($json)\np $data->{name}   # Alice\nmy $arr = fj '[1, 2, {\"x\": 3}]'\np $arr->[2]{x}    # 3\nslurp(\"config.json\") |> fj\n```",
        "from_yaml" | "fy" => "`from_yaml` (alias `fy`) parses a YAML string into a stryke data structure. YAML is commonly used for configuration files due to its human-readable syntax. Handles scalars, arrays, hashes, and nested combinations. The parser supports common YAML features like multi-line strings and bare unquoted keys.\n\n```perl\nmy $yaml = \"name: Alice\\nage: 30\"\nmy $data = from_yaml($yaml)\np $data->{name}   # Alice\nslurp(\"config.yaml\") |> fy |> say $_->{database}{host}\n```",
        "from_toml" | "ftoml" => "`from_toml` (alias `ftoml`) parses a TOML string into a stryke hashref. TOML is popular for Rust, Python, and other config files due to its explicit syntax. Sections become nested hashes, arrays stay arrays, and typed values (integers, floats, strings, booleans) are preserved.\n\n```perl\nmy $toml = \"[package]\\nname = \\\"myapp\\\"\\nversion = \\\"1.0\\\"\"\nmy $data = from_toml($toml)\np $data->{package}{name}   # myapp\nslurp(\"Cargo.toml\") |> ftoml |> say $_->{package}{version}\n```",
        "from_xml" | "fx" => "`from_xml` (alias `fx`) parses an XML string into a stryke hashref. Element names become hash keys, text content becomes string values, nested elements become nested hashes. The parser handles the XML declaration and basic structures.\n\n```perl\nmy $xml = '<root><name>Alice</name><age>30</age></root>'\nmy $data = from_xml($xml)\np $data->{root}{name}   # Alice\nslurp(\"config.xml\") |> fx |> say $_->{config}{database}{host}\n```",
        "from_csv" | "fcsv" => "`from_csv` (alias `fcsv`) parses a CSV string into an arrayref of hashrefs, treating the first line as headers. This is the inverse of `to_csv` and handles quoted fields containing commas according to RFC 4180.\n\n```perl\nmy $csv = \"name,age\\nAlice,30\\nBob,25\"\nmy $rows = from_csv($csv)\np $rows->[0]{name}   # Alice\np $rows->[1]{age}    # 25\nslurp(\"data.csv\") |> fcsv |> grep { $_->{age} > 25 }\n```",
        "sparkline" | "spark" => "`sparkline` (alias `spark`) renders a list of numbers as a compact Unicode sparkline string using block characters (▁▂▃▄▅▆▇█). Each value maps to a bar height proportional to the min/max range. Ideal for inline data visualization in terminal output, dashboards, or log summaries.\n\n```perl\n(3,7,1,9,4,2,8,5) |> spark |> p   # ▃▆▁█▄▂▇▅\n@daily_sales |> spark |> p         # quick trend line\nfr |> map { size } |> spark |> p   # file size distribution\n```",
        "bar_chart" | "bars" => "`bar_chart` (alias `bars`) renders a hashref as a colored horizontal bar chart in the terminal. Each key becomes a label, each value a proportional bar. Colors cycle automatically. Pairs naturally with `freq` for instant word/event counting visualization.\n\n```perl\nqw(a b a c a b) |> freq |> bars |> p\n# a │ ████████████████████████████████████████ 3\n# b │ ███████████████████████████ 2\n# c │ █████████████ 1\nbars({cpu => 73, mem => 45, disk => 91}) |> p\n```",
        "flame" | "flamechart" => "`flame` (alias `flamechart`) renders a hierarchical hashref as a terminal flamechart with colored stacked bars. Nested hashes become child rows; widths are proportional to leaf values (weights). Useful for visualizing call stacks, cost breakdowns, or any tree-structured data.\n\n```perl\nflame({main => {parse => 30, eval => {compile => 15, run => 45}}, init => 10}) |> p\nmy $profile = read_json \"profile.json\"\nflame($profile) |> p\n```",
        "histo" => "`histo` renders a vertical histogram in the terminal. Given a hashref (label → count), it draws colored vertical bars. Given a flat array of numbers, it auto-bins into 10 buckets and shows the distribution. Pairs with `freq` for categorical data.\n\n```perl\nqw(a b a c a b) |> freq |> histo |> p   # vertical bars for a/b/c\n(map { int(rand(100)) } 1..1000) |> histo |> p  # distribution of randoms\n```",
        "gauge" => "`gauge` renders a single value as a horizontal gauge bar with color coding (green ≥80%, yellow ≥50%, magenta ≥25%, red below). Accepts a fraction (0.0–1.0) or a value with max: `gauge(45, 100)`.\n\n```perl\np gauge(0.73)       # [██████████████████████░░░░░░░░] 73%\np gauge(45, 100)    # [█████████████░░░░░░░░░░░░░░░░] 45%\n```",
        "spinner" => "`spinner` shows an animated braille spinner on stderr while a block executes, then clears the spinner and returns the block's result. Useful for long-running one-liners where you want visual feedback without polluting stdout.\n\n```perl\nmy $r = spinner \"loading\" { fetch_json(\"https://api.example.com/data\") }\nspinner { sleep 2\n    42 }   # default message \"working\"\n```",
        "frequencies" | "freq" | "frq" => "`frequencies` (aliases `freq`, `frq`) counts how many times each distinct element appears in a list and returns a hashref mapping each value to its count. This is the stryke equivalent of a histogram or counter — useful for analyzing log files, counting word occurrences, tallying categorical data, or finding duplicates. The input list is flattened, so you can pass arrays directly. The returned hashref can be fed into `dd` for inspection or `to_json` for serialization.\n\n```perl\nmy @words = qw(apple banana apple cherry banana apple)\nmy $counts = freq @words\np $counts   # {apple => 3, banana => 2, cherry => 1}\nrl(\"access.log\") |> map { /^(\\S+)/ && $1 } |> freq |> dd\nmy @rolls = map { 1 + int(rand 6) } 1..1000\nfrq(@rolls) |> to_json |> p\n```",
        "interleave" | "il" => "`interleave` (alias `il`) merges two or more arrays by alternating their elements: first element of each array, then second element of each, and so on. If the arrays have different lengths, shorter arrays contribute `undef` for their missing positions. This is useful for building key-value pair lists from separate key and value arrays, creating round-robin schedules, or weaving parallel data streams together.\n\n```perl\nmy @keys = qw(name age city)\nmy @vals = (\"Alice\", 30, \"NYC\")\nmy @pairs = il \\@keys, \\@vals\np @pairs   # name, Alice, age, 30, city, NYC\nmy %h = il \\@keys, \\@vals\np $h{name} # Alice\nmy @rgb = il [255,0,0], [0,255,0], [0,0,255]\n```",
        "words" | "wd" => "`words` (alias `wd`) splits a string on whitespace boundaries and returns the resulting list of words. It handles leading, trailing, and consecutive whitespace gracefully — unlike a naive `split / /`, it never produces empty strings. This is the idiomatic way to tokenize a line of text in stryke, and the short alias `wd` keeps pipelines compact. It is equivalent to Perl's `split ' '` behavior.\n\n```perl\nmy @w = wd \"  hello   world  \"\np @w       # hello, world\nmy $line = \"  foo  bar  baz  \"\nmy $count = scalar wd $line\np $count   # 3\nrl(\"data.txt\") |> map { scalar wd $_ } |> e p\n```",
        "digits" | "dg" => "`digits` (alias `dg`) extracts all digit characters from a string and returns them as a list. This is a quick way to pull numbers out of mixed text without writing a regex. Pairs naturally with `join` to reconstruct the numeric string, or `freq` for digit distribution analysis.\n\n```perl\np join \"\", digits(\"phone: 555-1234\")     # 5551234\n\"abc123def456\" |> digits |> cnt |> p     # 6\ncat(\"log.txt\") |> digits |> freq |> bars |> p\n```",
        "letters" | "lts" => "`letters` (alias `lts`) extracts all alphabetic characters from a string and returns them as a list. Filters out digits, punctuation, whitespace — keeps only letters.\n\n```perl\np join \"\", letters(\"h3ll0 w0rld!\")   # hllwrld\n\"abc123DEF\" |> letters |> cnt |> p   # 6\n```",
        "letters_uc" => "`letters_uc` extracts only uppercase letters from a string.\n\n```perl\np join \"\", letters_uc(\"Hello World 123\")  # HW\n```",
        "letters_lc" => "`letters_lc` extracts only lowercase letters from a string.\n\n```perl\np join \"\", letters_lc(\"Hello World 123\")  # elloorld\n```",
        "punctuation" | "punct" => "`punctuation` (alias `punct`) extracts all ASCII punctuation characters from a string and returns them as a list. Filters out letters, digits, whitespace — keeps only punctuation.\n\n```perl\np join \"\", punctuation(\"Hello, world!\")  # ,!\n\"a.b-c\" |> punct |> cnt |> p             # 2\n```",
        "sentences" | "sents" => "`sentences` (alias `sents`) splits text on sentence boundaries (`.` `!` `?` followed by whitespace or end of string). Returns a list of trimmed sentences. Useful for NLP pipelines, text analysis, or splitting prose into processable units.\n\n```perl\n\"Hello world. How are you? Fine!\" |> sentences |> e p\n# Hello world.\n# How are you?\n# Fine!\ncat(\"essay.txt\") |> sentences |> cnt |> p    # sentence count\n```",
        "paragraphs" | "paras" => "`paragraphs` (alias `paras`) splits text on blank lines (one or more consecutive newlines). Returns a list of trimmed paragraph strings. Useful for processing structured documents, README files, or any text with paragraph breaks.\n\n```perl\ncat(\"README.md\") |> paragraphs |> cnt |> p   # paragraph count\ncat(\"essay.txt\") |> paragraphs |> map { sentences |> cnt } |> spark |> p  # sentences per paragraph\n```",
        "sections" | "sects" => "`sections` (alias `sects`) splits text on markdown-style headers (`# ...`, `## ...`) or lines of `===`/`---`. Returns a list of arrayrefs `[heading, body]` where each section's heading and body are separated. Useful for parsing structured documents.\n\n```perl\ncat(\"README.md\") |> sections |> cnt |> p     # section count\ncat(\"README.md\") |> sections |> map { $_->[0] } |> e p  # list all headings\n```",
        "numbers" | "nums" => "`numbers` (alias `nums`) extracts all numbers (integers and floats, including negatives) from a string and returns them as numeric values. Unlike `digits` which returns individual digit characters, `numbers` returns actual parsed values. Useful for pulling measurements, scores, or IDs out of mixed text.\n\n```perl\np join \",\", numbers(\"temp 98.6F, -20C, ver 3\")  # 98.6,-20,3\ncat(\"log.txt\") |> numbers |> avg |> p              # average of all numbers in file\n\"price: $12.99 qty: 5\" |> numbers |> e p            # 12.99, 5\n```",
        "graphemes" | "grs" => "`graphemes` (alias `grs`) splits a string into Unicode grapheme clusters. Unlike `chars` which splits on code points, `graphemes` keeps combining marks and emoji sequences together as single visual units. `\"cafe\\u{0301}\"` gives 4 graphemes (not 5 code points). Essential for correct text processing of accented characters, emoji, and non-Latin scripts.\n\n```perl\np cnt graphemes(\"cafe\\u{0301}\")    # 4 (not 5)\ngraphemes(\"hello\") |> e p          # h, e, l, l, o\n```",
        "columns" | "cols" => "`columns` (alias `cols`) splits fixed-width columnar text into fields. Without a widths argument, it auto-detects columns by splitting on runs of 2+ whitespace (ideal for `ps aux`, `ls -l`, `df` output). With an arrayref of widths, it splits at exact fixed positions. Pairs with `lines` for processing tabular command output.\n\n```perl\nmy @fields = columns(\"USER  PID  %CPU\")             # auto-detect: [USER, PID, %CPU]\nmy @fixed = columns(\"John   Doe   30\", [7, 7, 3])   # fixed-width: [John, Doe, 30]\ncapture(\"ps aux\") |> lines |> map { columns } |> tbl |> p\n```",
        "count" | "len" | "cnt" => "`count` (aliases `len`, `cnt`) returns the number of elements in a list, the number of characters in a string, the number of key-value pairs in a hash, or the cardinality of a set. It is a universal length function that dispatches based on the type of its argument. This replaces the need to use `scalar @array` or `length $string` — `cnt` is shorter and works uniformly across types. In a pipeline, it naturally reduces a collection to a single number.\n\nNote: `size` is NOT a count alias — it returns a file's byte size (see `size`).\n\n```perl\nmy @arr = (1, 2, 3, 4, 5)\np cnt @arr          # 5\np len \"hello\"       # 5\nmy %h = (a => 1, b => 2)\np cnt \\%h           # 2\nrl(\"file.txt\") |> cnt |> p  # line count\n```",
        "size" => "`size` returns the byte size of a file on disk — equivalent to Perl's `-s FILE` file test. With no arguments, it operates on `$_`; with one argument, it stats the given path; with multiple arguments (or a flattened list), it returns an array of sizes. Paths that can't be stat'd return `undef`. This is a stryke extension that makes pipelines over filenames concise.\n\n```perl\np size \"Cargo.toml\"                   # 2013\nf |> map +{ $_ => size } |> tj |> p   # [{name => bytes}, ...]\nf |> filter { size > 1024 } |> e p    # files larger than 1 KiB\n```",
        "list_count" | "list_size" => "`list_count` (alias `list_size`) returns the total number of elements after flattening a nested list structure. Unlike `count` which returns the top-level element count, `list_count` recursively descends into array references and counts only leaf values. This is useful when you have a list of lists and need to know the total number of individual items rather than the number of sublists.\n\n```perl\nmy @nested = ([1, 2], [3, 4, 5], [6])\np list_count @nested   # 6\nmy @deep = ([1, [2, 3]], [4])\np list_size @deep      # 4\np list_count 1, 2, 3   # 3  (flat list works too)\n```",
        "clamp" | "clp" => "`clamp` (alias `clp`) constrains each value in a list to lie within a specified minimum and maximum range. Values below the minimum are raised to it; values above the maximum are lowered to it; values already in range pass through unchanged. This is essential for sanitizing user input, bounding computed values before display, or enforcing physical constraints in simulations. When given a single scalar, it returns a single clamped value.\n\n```perl\nmy @scores = (105, -3, 42, 99, 200)\nmy @clamped = clp 0, 100, @scores\np @clamped   # 100, 0, 42, 99, 100\nmy $val = clp 0, 255, $input   # bound to byte range\nmy @pct = map { clp 0.0, 1.0, $_ } @raw_ratios\n```",
        "normalize" | "nrm" => "`normalize` (alias `nrm`) rescales a list of numeric values so that the minimum maps to 0 and the maximum maps to 1, using min-max normalization. This is a standard preprocessing step for machine learning features, data visualization (mapping values to color gradients or bar heights), and statistical analysis. If all values are identical, the result is a list of zeros to avoid division by zero. The output preserves the relative ordering of the input.\n\n```perl\nmy @temps = (32, 68, 100, 212)\nmy @normed = nrm @temps\np @normed   # 0, 0.2, 0.377..., 1\nmy @pixels = nrm @raw_intensities\n@pixels |> map { int($_ * 255) } |> e p  # scale to 0-255\n```",
        "snake_case" | "sc" => "`snake_case` (alias `sc`) converts a string from any common casing convention — camelCase, PascalCase, kebab-case, or mixed — into snake_case, where words are lowercase and separated by underscores. This is the standard naming convention for Perl and Rust variables and function names. Consecutive uppercase letters in acronyms are handled intelligently (e.g., `parseHTTPResponse` becomes `parse_http_response`).\n\n```perl\np sc \"camelCase\"          # camel_case\np sc \"PascalCase\"         # pascal_case\np sc \"kebab-case\"         # kebab_case\np sc \"parseHTTPResponse\"  # parse_http_response\nmy @methods = qw(getUserName setEmailAddr)\n@methods |> map sc |> e p\n```",
        "camel_case" | "cc" => "`camel_case` (alias `cc`) converts a string from any casing convention into camelCase, where the first word is lowercase and subsequent words are capitalized with no separators. This is the standard naming convention for JavaScript variables and Java methods. The function handles underscores, hyphens, and spaces as word boundaries and strips them during conversion.\n\n```perl\np cc \"snake_case\"         # snakeCase\np cc \"kebab-case\"         # kebabCase\np cc \"PascalCase\"         # pascalCase\np cc \"hello world\"        # helloWorld\nmy @cols = qw(first_name last_name email_addr)\n@cols |> map cc |> e p\n```",
        "kebab_case" | "kc" => "`kebab_case` (alias `kc`) converts a string from any casing convention into kebab-case, where words are lowercase and separated by hyphens. This is the standard naming convention for CSS classes, URL slugs, and CLI flag names. Like `snake_case`, it intelligently handles acronyms and mixed-case input. The function treats underscores, spaces, and case transitions as word boundaries.\n\n```perl\np kc \"camelCase\"          # camel-case\np kc \"PascalCase\"         # pascal-case\np kc \"snake_case\"         # snake-case\np kc \"parseHTTPResponse\"  # parse-http-response\nmy $slug = kc $title      # URL-safe slug\n```",
        "json_jq" => "`json_jq` applies a jq-style query expression to a stryke data structure and returns the matched value. This brings the power of the `jq` command-line JSON processor directly into stryke without shelling out. Dot notation traverses hash keys, bracket notation indexes arrays, and nested paths are separated by dots. It is ideal for extracting deeply nested values from API responses or config files without chains of hash dereferences.\n\n```perl\nmy $data = json_decode(slurp \"api.json\")\np json_jq($data, \".results[0].name\")\nmy $cfg = rj \"config.json\"\np json_jq($cfg, \".database.host\")  # deep extract\nfetch_json(\"https://api.example.com/users\") |> json_jq(\".data[0].email\") |> p\n```",
        "toml_decode" => "`toml_decode` (alias `td`) parses a TOML-formatted string and returns the corresponding stryke hash structure. TOML sections become nested hashrefs, arrays map to arrayrefs, and typed scalars (integers, floats, booleans, datetime strings) are preserved. This is useful for reading configuration files, parsing Cargo.toml manifests, or processing any TOML input. Malformed TOML raises an exception.\n\n```perl\nmy $cfg = toml_decode(slurp \"config.toml\")\np $cfg->{database}{host}   # localhost\nmy $cargo = toml_decode(slurp \"Cargo.toml\")\np $cargo->{package}{version}\nslurp(\"settings.toml\") |> toml_decode |> dd\n```",
        "toml_encode" => "`toml_encode` (alias `te`) serializes a stryke hashref into a valid TOML string. Nested hashes become TOML sections with `[section]` headers, arrays become TOML arrays, and scalars are serialized with appropriate quoting. This is the inverse of `toml_decode` and is useful for generating or updating configuration files programmatically. The output is human-readable and can be written directly to a `.toml` file.\n\n```perl\nmy %cfg = (server => {host => \"0.0.0.0\", port => 8080}, debug => 0)\ntoml_encode(\\%cfg) |> spurt \"server.toml\"\nmy $round = toml_decode(toml_encode(\\%cfg))\np $round->{server}{port}  # 8080\n```",
        "xml_decode" => "`xml_decode` (alias `xd`) parses an XML string and returns a stryke data structure. Elements become hash keys, text content becomes scalar values, repeated child elements become arrayrefs, and attributes are accessible through a conventions-based mapping. This is useful for consuming SOAP responses, RSS feeds, SVG files, or any XML-based API. Malformed XML raises an exception rather than returning partial data.\n\n```perl\nmy $doc = xml_decode('<root><name>stryke</name><ver>1</ver></root>')\np $doc->{root}{name}   # stryke\nslurp(\"feed.xml\") |> xml_decode |> dd\nmy $svg = xml_decode(fetch(\"https://example.com/image.svg\"))\np $svg->{svg}\n```",
        "xml_encode" => "`xml_encode` (alias `xe`) serializes a stryke data structure into a well-formed XML string. Hash keys become element names, scalar values become text content, and arrayrefs become repeated sibling elements. This is useful for generating XML payloads for SOAP APIs, creating RSS or Atom feeds, or producing configuration files in XML format. The output is the inverse of `xml_decode` and round-trips cleanly.\n\n```perl\nmy %data = (root => {title => \"Test\", items => [{id => 1}, {id => 2}]})\np xml_encode \\%data\nxml_encode(\\%data) |> spurt \"output.xml\"\nmy $payload = xml_encode {request => {action => \"query\", id => 42}}\nhttp_request(method => \"POST\", url => $endpoint, body => $payload)\n```",
        "yaml_decode" => "`yaml_decode` (alias `yd`) parses a YAML string and returns the corresponding stryke data structure. Mappings become hashrefs, sequences become arrayrefs, and scalars are coerced to their natural Perl types. This handles YAML 1.2 including multi-document streams, anchors/aliases, and flow notation. It is the go-to function for reading YAML configuration files, Kubernetes manifests, or CI pipeline definitions. Invalid YAML raises an exception.\n\n```perl\nmy $cfg = yaml_decode(slurp \"docker-compose.yml\")\np $cfg->{services}{web}{image}\nmy $ci = yaml_decode(slurp \".github/workflows/ci.yml\")\ndd $ci->{jobs}\nslurp(\"values.yaml\") |> yaml_decode |> json_encode |> p\n```",
        "yaml_encode" => "`yaml_encode` (alias `ye`) serializes a stryke data structure into a valid YAML string. Hashes become YAML mappings, arrays become sequences with dash prefixes, and scalars are quoted only when necessary for disambiguation. The output is human-readable and suitable for writing config files, generating Kubernetes resources, or producing YAML-based API payloads. It is the inverse of `yaml_decode`.\n\n```perl\nmy %svc = (name => \"api\", replicas => 3, ports => [80, 443])\nyaml_encode(\\%svc) |> spurt \"service.yaml\"\nmy $round = yaml_decode(yaml_encode(\\%svc))\np $round->{replicas}   # 3\nrj(\"config.json\") |> yaml_encode |> p  # JSON to YAML\n```",
        "csv_read" => "`csv_read` (alias `cr`) reads CSV data from a file path or string and returns an array of arrayrefs, where each inner arrayref represents one row. The parser handles RFC 4180 CSV correctly — quoted fields with embedded commas, newlines inside quotes, and escaped double quotes are all supported. The first row is treated as data (not a header) unless you process it separately. This is the fastest way to ingest tabular data in stryke.\n\n```perl\nmy @rows = csv_read \"data.csv\"\np $rows[0]             # first row as arrayref\n@rows |> e { p $_->[0] }  # print first column\nmy @inline = csv_read \"a,b,c\\n1,2,3\\n4,5,6\"\np scalar @inline       # 3 rows (including header)\n```",
        "csv_write" => "`csv_write` (alias `cw`) serializes an array of arrayrefs into a CSV-formatted string. Each inner arrayref becomes one row, and fields are automatically quoted when they contain commas, quotes, or newlines. This is the complement of `csv_read` and produces output conforming to RFC 4180. Use it to generate CSV files from computed data, export database query results, or prepare data for spreadsheet import.\n\n```perl\nmy @data = ([\"name\", \"age\"], [\"Alice\", 30], [\"Bob\", 25])\ncsv_write(\\@data) |> spurt \"people.csv\"\nmy @report = map { [$_->{id}, $_->{score}] } @results\np csv_write \\@report\n```",
        "dataframe" => "`dataframe` (alias `df`) creates a columnar dataframe from tabular data, providing a structured way to work with rows and columns. You can construct a dataframe from an array of hashrefs, an array of arrayrefs with a header row, or from a CSV file. The dataframe supports column selection, filtering, sorting, and aggregation operations. This is stryke's answer to Python's pandas DataFrame — lightweight but sufficient for common data manipulation tasks.\n\n```perl\nmy $df = df [{name => \"Alice\", age => 30}, {name => \"Bob\", age => 25}]\np $df\nmy $df2 = df csv_read \"data.csv\"\nmy @ages = $df->{age}\np @ages   # 30, 25\n```",
        "sqlite" => "`sqlite` (alias `sql`) executes a SQL statement against an SQLite database file and returns the results. For SELECT queries, it returns an array of hashrefs where each hashref represents one row with column names as keys. For INSERT, UPDATE, and DELETE statements, it returns the number of affected rows. Bind parameters prevent SQL injection and handle quoting automatically. The database file is created if it does not exist, making `sqlite` a zero-setup embedded database.\n\n```perl\nsqlite(\"app.db\", \"CREATE TABLE users (name TEXT, age INT)\")\nsqlite(\"app.db\", \"INSERT INTO users VALUES (?, ?)\", \"Alice\", 30)\nmy @rows = sqlite(\"app.db\", \"SELECT * FROM users WHERE age > ?\", 20)\n@rows |> e { p $_->{name} }\nsqlite(\"app.db\", \"SELECT count(*) as n FROM users\") |> dd\n```",

        // ── HTTP / networking ──
        "fetch" => "`fetch` (alias `ft`) performs a blocking HTTP GET request to the given URL and returns the response body as a string. It follows redirects automatically and raises an exception on network errors or non-2xx status codes. This is the simplest way to retrieve content from the web in stryke — no need to configure a client or parse response objects. For JSON APIs, prefer `fetch_json` which additionally decodes the response.\n\n```perl\nmy $html = fetch \"https://example.com\"\np $html\nmy $ip = fetch \"https://api.ipify.org\"\np \"My IP: $ip\"\nfetch(\"https://example.com/data.txt\") |> spurt \"local.txt\"$1\n\nNote: for POST, PUT, DELETE, or custom headers, use `http_request` instead.",
        "fetch_json" => "`fetch_json` (alias `ftj`) performs a blocking HTTP GET request and automatically decodes the JSON response body into a stryke data structure. This combines `fetch` and `json_decode` into a single call, which is the common case when consuming REST APIs. It raises an exception if the response is not valid JSON or the request fails. The returned value is typically a hashref or arrayref ready for immediate use.\n\n```perl\nmy $data = fetch_json \"https://api.github.com/repos/stryke/stryke\"\np $data->{stargazers_count}\nfetch_json(\"https://jsonplaceholder.typicode.com/todos\") |> e { p $_->{title} }\nmy $weather = fetch_json \"https://wttr.in/?format=j1\"\np $weather->{current_condition}[0]{temp_C}\n```",
        "fetch_async" => "`fetch_async` (alias `fta`) initiates a non-blocking HTTP GET request and returns a task handle that can be awaited later. This allows you to fire off multiple HTTP requests concurrently and wait for them all to complete, dramatically reducing total latency when fetching from several endpoints. The task resolves to the response body as a string, just like `fetch`. Use this when you need to parallelize network I/O without threads.\n\n```perl\nmy $t1 = fetch_async \"https://api.example.com/users\"\nmy $t2 = fetch_async \"https://api.example.com/posts\"\nmy $users = await $t1\nmy $posts = await $t2\np \"Got users and posts concurrently\"\n```",
        "fetch_async_json" => "`fetch_async_json` (alias `ftaj`) initiates a non-blocking HTTP GET request that automatically decodes the JSON response when awaited. This combines `fetch_async` with `json_decode`, making it the ideal choice for concurrent API calls where every response is JSON. The task resolves to the decoded stryke data structure — a hashref, arrayref, or scalar depending on the JSON content.\n\n```perl\nmy @urls = map { \"https://api.example.com/item/$_\" } 1..10\nmy @tasks = map fetch_async_json @urls\nmy @results = map await @tasks\n@results |> e { p $_->{name} }\n```",
        "http_request" => "`http_request` (alias `hr`) performs a fully configurable HTTP request with control over method, headers, body, and timeout. Unlike `fetch` which is limited to GET, `http_request` supports POST, PUT, PATCH, DELETE, and any other HTTP method. Pass named parameters for configuration. The response is returned as a hashref containing `status`, `headers`, and `body` fields, giving you full access to the HTTP response. This is the right tool when you need to send data, set authentication headers, or inspect status codes.\n\n```perl\nmy $res = http_request(method => \"POST\", url => \"https://api.example.com/users\",\n    headers => {\"Content-Type\" => \"application/json\", Authorization => \"Bearer $token\"},\n    body => tj {name => \"Alice\"})\np $res->{status}   # 201\nmy $data = json_decode $res->{body}\nmy $del = http_request(method => \"DELETE\", url => \"https://api.example.com/users/42\")\n```",

        // ── Crypto / hashing ──
        "sha256" => "`sha256` (alias `s256`) computes the SHA-256 cryptographic hash of the input data and returns it as a 64-character lowercase hexadecimal string. SHA-256 is the most widely used hash function for data integrity verification, content addressing, and digital signatures. The Rust implementation is significantly faster than pure-Perl alternatives. Accepts strings or byte buffers.\n\n```perl\np sha256 \"hello world\"   # b94d27b9934d3e08...\nmy $checksum = sha256 slurp \"release.tar.gz\"\np $checksum\nrl(\"passwords.txt\") |> map sha256 |> e p\n```",
        "sha224" => "`sha224` (alias `s224`) computes the SHA-224 cryptographic hash and returns a 56-character hex string. SHA-224 is a truncated variant of SHA-256 that produces a shorter digest while maintaining strong collision resistance. It is sometimes preferred when storage or bandwidth for the hash value is constrained, such as in compact data structures or short identifiers.\n\n```perl\np sha224 \"hello world\"   # 2f05477fc24bb4fa...\nmy $h = sha224(tj {key => \"value\"})\np $h\n```",
        "sha384" => "`sha384` (alias `s384`) computes the SHA-384 cryptographic hash and returns a 96-character hex string. SHA-384 is a truncated variant of SHA-512 that offers a middle ground between SHA-256 and SHA-512 in both digest length and security margin. It is commonly used in TLS certificate fingerprints and government security standards that require larger-than-256-bit digests.\n\n```perl\np sha384 \"hello world\"   # fdbd8e75a67f29f7...\nmy $sig = sha384(slurp \"document.pdf\")\nspurt \"document.pdf.sha384\", $sig\n```",
        "sha512" => "`sha512` (alias `s512`) computes the SHA-512 cryptographic hash and returns a 128-character hex string. SHA-512 provides the largest digest size in the SHA-2 family and is the strongest option when maximum collision resistance is needed. On 64-bit systems, SHA-512 is often faster than SHA-256 because it operates on 64-bit words natively. Use this for high-security applications or when you need a longer hash.\n\n```perl\np sha512 \"hello world\"   # 309ecc489c12d6eb...\nmy $hash = sha512(slurp \"firmware.bin\")\np $hash\nmy @hashes = map sha512 @files\n```",
        "sha1" => "`sha1` (alias `s1`) computes the SHA-1 hash and returns a 40-character hex string. SHA-1 is considered cryptographically broken for collision resistance and should not be used for security-sensitive applications. However, it remains widely used for non-security purposes such as Git object IDs, cache keys, and deduplication checksums where collision attacks are not a concern.\n\n```perl\np sha1 \"hello world\"   # 2aae6c35c94fcfb4...\nmy $git_id = sha1(\"blob \" . length($content) . \"\\0\" . $content)\np $git_id$1\n\nNote: prefer SHA-256 for any security-related use case.",
        "crc32" => "`crc32` computes the CRC-32 checksum of the input data and returns it as an unsigned 32-bit integer. CRC-32 is not a cryptographic hash — it is a fast error-detection code used in network protocols (Ethernet, ZIP, PNG), file integrity checks, and hash table bucketing. It is extremely fast compared to SHA functions, making it suitable for high-throughput deduplication or quick change detection where collision resistance is not required.\n\n```perl\np crc32 \"hello world\"   # 222957957\nmy $chk = crc32(slurp \"archive.zip\")\np sprintf \"0x%08x\", $chk  # hex representation\nmy @checksums = map crc32 @chunks\n```",
        "hmac_sha256" | "hmac" => "`hmac_sha256` (alias `hmac`) computes an HMAC-SHA256 message authentication code using the given data and secret key, returning a hex string. HMAC combines a cryptographic hash with a secret key to produce a signature that verifies both data integrity and authenticity. This is the standard mechanism for signing API requests (AWS, Stripe, GitHub webhooks), generating secure tokens, and verifying message authenticity.\n\n```perl\nmy $sig = hmac_sha256 \"request body\", \"my-secret-key\"\np $sig\nmy $webhook_sig = hmac(\"POST /hook\\n$body\", $secret)\np $webhook_sig eq $expected ? \"valid\" : \"tampered\"\n```",
        // ── BLAKE2 / BLAKE3 ──
        "blake2b" | "b2b" => "`blake2b` (alias `b2b`) computes the BLAKE2b-512 cryptographic hash and returns a 128-character hex string. BLAKE2b is faster than SHA-256 while being at least as secure. It is used in Argon2 password hashing, libsodium, WireGuard, and many modern cryptographic protocols. Prefer BLAKE2b or BLAKE3 for new projects over SHA-256.\n\n```perl\np blake2b \"hello world\"   # 021ced8799...\nmy $hash = b2b(slurp \"large-file.bin\")\nmy @hashes = map blake2b @messages\n```",
        "blake2s" | "b2s" => "`blake2s` (alias `b2s`) computes the BLAKE2s-256 cryptographic hash and returns a 64-character hex string. BLAKE2s is optimized for 8-32 bit platforms while BLAKE2b targets 64-bit. Use BLAKE2s when targeting embedded systems, WebAssembly, or when a 256-bit digest suffices.\n\n```perl\np blake2s \"hello world\"   # 9aec6806...\nmy $checksum = b2s($firmware_blob)\n```",
        "blake3" | "b3" => "`blake3` (alias `b3`) computes the BLAKE3 cryptographic hash and returns a 64-character hex string (256-bit). BLAKE3 is the latest evolution — it is parallelizable, even faster than BLAKE2, and suitable for hashing large files at maximum speed. It is the recommended hash function for new projects where legacy compatibility is not required.\n\n```perl\np blake3 \"hello world\"   # d74981efa...\nmy $hash = b3(slurp \"gigabyte-file.bin\")  # fast!\n```",
        // ── Password Hashing (KDFs) ──
        "argon2_hash" | "argon2" => "`argon2_hash` (alias `argon2`) hashes a password using Argon2id, the winner of the Password Hashing Competition. Returns a PHC-format string containing algorithm parameters, salt, and hash. Argon2 is memory-hard and resistant to GPU/ASIC attacks. Use this for user password storage — never use MD5/SHA for passwords.\n\n```perl\nmy $hash = argon2_hash(\"user-password\")\nto_file(\"user.hash\", $hash)\n# later...\nif (argon2_verify(\"user-password\", $hash)) {\n    p \"login successful\"\n}\n```",
        "argon2_verify" => "`argon2_verify` verifies a password against an Argon2 PHC hash string. Returns 1 if the password matches, 0 otherwise. The verification automatically extracts parameters from the stored hash, so changing Argon2 settings for new hashes does not break existing ones.\n\n```perl\nmy $stored = rl(\"user.hash\")\nif (argon2_verify($user_input, $stored)) {\n    p \"access granted\"\n} else {\n    p \"invalid password\"\n}\n```",
        "bcrypt_hash" | "bcrypt" => "`bcrypt_hash` (alias `bcrypt`) hashes a password using the bcrypt algorithm, returning a standard `$2b$...` format string. Bcrypt has been the industry standard for password hashing for over 20 years. While Argon2 is now preferred for new systems, bcrypt remains secure and widely deployed.\n\n```perl\nmy $hash = bcrypt_hash(\"my-password\")\np $hash  # $2b$12$...\nif (bcrypt_verify(\"my-password\", $hash)) {\n    p \"correct\"\n}\n```",
        "bcrypt_verify" => "`bcrypt_verify` verifies a password against a bcrypt hash string. Returns 1 if correct, 0 otherwise. The cost factor and salt are extracted from the stored hash automatically.\n\n```perl\nif (bcrypt_verify($password, $stored_hash)) {\n    grant_access()\n}\n```",
        "scrypt_hash" | "scrypt" => "`scrypt_hash` (alias `scrypt`) hashes a password using the scrypt algorithm, returning a PHC-format string. Scrypt is memory-hard like Argon2 and is used in cryptocurrency (Litecoin) and some enterprise systems. It predates Argon2 but remains secure.\n\n```perl\nmy $hash = scrypt_hash(\"password123\")\nif (scrypt_verify(\"password123\", $hash)) {\n    p \"verified\"\n}\n```",
        "scrypt_verify" => "`scrypt_verify` verifies a password against a scrypt PHC hash. Returns 1 on match, 0 otherwise.\n\n```perl\nif (scrypt_verify($input, $stored)) {\n    p \"valid\"\n}\n```",
        "pbkdf2" | "pbkdf2_derive" => "`pbkdf2` (alias `pbkdf2_derive`) derives a cryptographic key from a password using PBKDF2-HMAC-SHA256. Returns a 64-character hex string (32 bytes). Takes password, salt, and optional iteration count (default 100,000). Use this when you need a fixed-length key from a password — for encryption keys, not password storage (use Argon2/bcrypt for that).\n\n```perl\nmy $key = pbkdf2(\"password\", \"random-salt\")\np $key  # 64 hex chars = 32 bytes\nmy $strong = pbkdf2(\"pass\", $salt, 200_000)  # more iterations\n```",
        // ── Secure Random ──
        "random_bytes" | "randbytes" => "`random_bytes` (alias `randbytes`) generates cryptographically secure random bytes using the OS CSPRNG. Returns a byte buffer of the specified length. Use for encryption keys, nonces, salts, and any security-sensitive randomness. Never use `rand()` for cryptographic purposes.\n\n```perl\nmy $key = random_bytes(32)   # 256-bit key\nmy $nonce = random_bytes(12) # 96-bit nonce for AES-GCM\np hex_encode($key)\n```",
        "random_bytes_hex" | "randhex" => "`random_bytes_hex` (alias `randhex`) generates cryptographically secure random bytes and returns them as a hex string. Convenient when you need a random hex token or key without a separate hex_encode call.\n\n```perl\nmy $token = random_bytes_hex(16)  # 32 hex chars\np $token   # 7a3f9b2c1d...\nmy $api_key = \"sk_\" . randhex(24)\n```",
        // ── Symmetric Encryption ──
        "aes_encrypt" | "aes_enc" => "`aes_encrypt` (alias `aes_enc`) encrypts plaintext using AES-256-GCM authenticated encryption. Takes a 32-byte key (use `random_bytes(32)` or `pbkdf2`). Returns a base64 string containing the nonce and ciphertext. AES-GCM provides both confidentiality and integrity — tampering is detected on decryption.\n\n```perl\nmy $key = random_bytes(32)\nmy $cipher = aes_encrypt($key, \"secret message\")\np $cipher  # base64 encoded\nmy $plain = aes_decrypt($key, $cipher)\np $plain   # secret message\n```",
        "aes_decrypt" | "aes_dec" => "`aes_decrypt` (alias `aes_dec`) decrypts AES-256-GCM ciphertext produced by `aes_encrypt`. Takes the same 32-byte key and the base64 ciphertext. Dies if the key is wrong or the ciphertext was tampered with (authentication failure).\n\n```perl\nmy $plain = aes_decrypt($key, $ciphertext)\np $plain\n# wrong key or tampered data raises exception\n```",
        "chacha_encrypt" | "chacha_enc" => "`chacha_encrypt` (alias `chacha_enc`) encrypts plaintext using ChaCha20-Poly1305 authenticated encryption. Takes a 32-byte key. Returns base64(nonce || ciphertext || tag). ChaCha20-Poly1305 is the modern alternative to AES-GCM — faster in software, constant-time, and used in TLS 1.3, WireGuard, and SSH.\n\n```perl\nmy $key = random_bytes(32)\nmy $cipher = chacha_encrypt($key, \"secret data\")\nmy $plain = chacha_decrypt($key, $cipher)\np $plain\n```",
        "chacha_decrypt" | "chacha_dec" => "`chacha_decrypt` (alias `chacha_dec`) decrypts ChaCha20-Poly1305 ciphertext. Takes the same 32-byte key and base64 ciphertext. Dies on wrong key or tampering.\n\n```perl\nmy $plain = chacha_decrypt($key, $cipher)\np $plain\n```",
        // ── Asymmetric Crypto (Ed25519, X25519) ──
        "ed25519_keygen" | "ed_keygen" => "`ed25519_keygen` (alias `ed_keygen`) generates an Ed25519 signing keypair. Returns [private_key_hex, public_key_hex]. Ed25519 is the modern standard for digital signatures — fast, secure, and used in SSH, GPG, and cryptocurrency. The private key is 32 bytes (64 hex), the public key is also 32 bytes.\n\n```perl\nmy ($priv, $pub) = @{ ed25519_keygen() }\np \"Private: $priv\"\np \"Public:  $pub\"\nmy $sig = ed25519_sign($priv, \"message\")\n```",
        "ed25519_sign" | "ed_sign" => "`ed25519_sign` (alias `ed_sign`) signs a message with an Ed25519 private key. Takes the private key (hex) and message. Returns a 128-character hex signature (64 bytes). Signatures are deterministic — the same key+message always produces the same signature.\n\n```perl\nmy $sig = ed25519_sign($private_key, \"hello world\")\np $sig   # 128 hex chars\n```",
        "ed25519_verify" | "ed_verify" => "`ed25519_verify` (alias `ed_verify`) verifies an Ed25519 signature. Takes public_key_hex, message, signature_hex. Returns 1 if valid, 0 if invalid. Never trust unsigned data in security contexts.\n\n```perl\nif (ed25519_verify($pub, \"hello world\", $sig)) {\n    p \"signature valid\"\n} else {\n    p \"FORGED!\"\n}\n```",
        "x25519_keygen" | "x_keygen" => "`x25519_keygen` (alias `x_keygen`) generates an X25519 key-exchange keypair. Returns [private_key_hex, public_key_hex]. X25519 is the modern Diffie-Hellman — used in TLS 1.3, Signal, WireGuard. Both parties generate keypairs and exchange public keys to derive a shared secret.\n\n```perl\nmy ($my_priv, $my_pub) = @{ x25519_keygen() }\n# send $my_pub to peer, receive $their_pub\nmy $shared = x25519_dh($my_priv, $their_pub)\n```",
        "x25519_dh" | "x_dh" => "`x25519_dh` (alias `x_dh`) performs X25519 Diffie-Hellman key exchange. Takes your private key and their public key. Returns a 64-character hex shared secret. Both parties derive the same secret, which can be used as an encryption key.\n\n```perl\nmy $shared = x25519_dh($my_private, $their_public)\nmy $key = hex_decode(substr($shared, 0, 64))  # use as AES key\n```",
        // ── Special Math Functions ──
        "erf" => "`erf` computes the error function, which arises in probability, statistics, and solutions to the heat equation. erf(x) is the probability that a standard normal random variable falls in [-x√2, x√2]. Returns a value in (-1, 1).\n\n```perl\np erf(0)      # 0\np erf(1)      # 0.8427...\np erf(10)     # ~1\n```",
        "erfc" => "`erfc` computes the complementary error function erfc(x) = 1 - erf(x). Numerically stable for large x where erf(x) ≈ 1. Used in computing tail probabilities of normal distributions.\n\n```perl\np erfc(0)     # 1\np erfc(3)     # 0.0000220...\n```",
        "gamma" | "tgamma" => "`gamma` (alias `tgamma`) computes the gamma function Γ(x), the extension of factorial to real numbers. Γ(n) = (n-1)! for positive integers. Used throughout statistics, physics, and combinatorics.\n\n```perl\np gamma(5)    # 24 (= 4!)\np gamma(0.5)  # √π ≈ 1.7724...\np gamma(1)    # 1\n```",
        "lgamma" | "ln_gamma" => "`lgamma` (alias `ln_gamma`) computes the natural logarithm of the gamma function: ln(Γ(x)). Avoids overflow for large arguments where Γ(x) would be astronomical. Essential for computing log-probabilities in statistics.\n\n```perl\np lgamma(100)  # 359.13...\np lgamma(1000) # 5905.22...\n```",
        "digamma" | "psi" => "`digamma` (alias `psi`) computes the digamma function ψ(x) = d/dx ln(Γ(x)), the logarithmic derivative of gamma. Appears in Bayesian statistics (expected log of Dirichlet variables), optimization, and special function theory.\n\n```perl\np digamma(1)   # -γ ≈ -0.5772 (Euler-Mascheroni)\np digamma(2)   # 1 - γ ≈ 0.4228\n```",
        "beta_fn" => "`beta_fn` computes the beta function B(a,b) = Γ(a)Γ(b)/Γ(a+b). The beta function is the normalizing constant of the Beta distribution and appears throughout Bayesian statistics and combinatorics.\n\n```perl\np beta_fn(2, 3)   # 0.0833...\np beta_fn(0.5, 0.5)  # π\n```",
        "lbeta" | "ln_beta" => "`lbeta` (alias `ln_beta`) computes ln(B(a,b)), the log of the beta function. Avoids overflow for small a or b where B(a,b) is very large.\n\n```perl\np lbeta(0.01, 0.01)  # large positive\n```",
        "betainc" | "beta_reg" => "`betainc` (alias `beta_reg`) computes the regularized incomplete beta function I_x(a,b), the CDF of the Beta distribution. Takes (x, a, b). Essential for computing p-values of F-tests, t-tests, and binomial probabilities.\n\n```perl\np betainc(0.5, 2, 3)  # P(Beta(2,3) < 0.5)\n```",
        "gammainc" | "gamma_li" => "`gammainc` (alias `gamma_li`) computes the lower incomplete gamma function γ(a,x) = ∫₀ˣ t^(a-1) e^(-t) dt. Used in computing CDFs of gamma and chi-squared distributions.\n\n```perl\np gammainc(2, 1)  # γ(2, 1)\n```",
        "gammaincc" | "gamma_ui" => "`gammaincc` (alias `gamma_ui`) computes the upper incomplete gamma function Γ(a,x) = ∫ₓ^∞ t^(a-1) e^(-t) dt = Γ(a) - γ(a,x). Useful for tail probabilities.\n\n```perl\np gammaincc(2, 1)  # Γ(2, 1)\n```",
        "gammainc_reg" | "gamma_lr" => "`gammainc_reg` (alias `gamma_lr`) computes the regularized lower incomplete gamma P(a,x) = γ(a,x)/Γ(a), the CDF of the gamma distribution Gamma(a, 1).\n\n```perl\np gammainc_reg(2, 1)  # P(Gamma(2,1) < 1)\n```",
        "gammaincc_reg" | "gamma_ur" => "`gammaincc_reg` (alias `gamma_ur`) computes the regularized upper incomplete gamma Q(a,x) = 1 - P(a,x), the survival function of the gamma distribution.\n\n```perl\np gammaincc_reg(2, 1)  # P(Gamma(2,1) > 1)\n```",
        // ── SHA-3 / Keccak ──
        "sha3_256" | "s3_256" => "`sha3_256` (alias `s3_256`) computes the SHA3-256 hash (NIST FIPS 202) and returns a 64-character hex string. SHA-3 is the newest NIST-approved hash family, designed as a backup if SHA-2 is ever compromised. It uses the Keccak sponge construction which is fundamentally different from SHA-2.\n\n```perl\np sha3_256 \"hello world\"   # 644bcc7e...\nmy $h = s3_256(slurp \"file.bin\")\n```",
        "sha3_512" | "s3_512" => "`sha3_512` (alias `s3_512`) computes the SHA3-512 hash and returns a 128-character hex string. Provides a 512-bit digest with the SHA-3 sponge construction.\n\n```perl\np sha3_512 \"hello world\"   # 840006...\n```",
        "shake128" => "`shake128` is a SHA-3 extendable-output function (XOF). Unlike fixed-length hashes, SHAKE can produce arbitrary-length output. Takes data and output length in bytes, returns hex. Useful for key derivation or when you need a variable-length digest.\n\n```perl\np shake128(\"seed\", 32)   # 64 hex chars (32 bytes)\np shake128(\"seed\", 64)   # 128 hex chars\n```",
        "shake256" => "`shake256` is a SHA-3 XOF with higher security margin than SHAKE128. Takes data and output length in bytes, returns hex.\n\n```perl\np shake256(\"seed\", 32)\np shake256(\"key material\", 64)\n```",
        // ── RIPEMD-160 ──
        "ripemd160" | "rmd160" => "`ripemd160` (alias `rmd160`) computes the RIPEMD-160 hash and returns a 40-character hex string. RIPEMD-160 is used in Bitcoin addresses (Hash160 = RIPEMD160(SHA256(pubkey))) and some legacy systems. Not recommended for new designs but essential for Bitcoin/crypto compatibility.\n\n```perl\np ripemd160 \"hello world\"   # 98c615784c...\nmy $hash160 = ripemd160(hex_decode(sha256($pubkey)))\n```",
        "md4" => "`md4` computes the MD4 hash and returns a 32-character hex string. MD4 is completely broken — only use for legacy NTLM compatibility or historical systems.\n\n```perl\np md4(\"hello\")   # 32 hex chars\n```",
        // ── xxHash ──
        "xxh32" | "xxhash32" => "`xxh32` (alias `xxhash32`) computes xxHash32 — extremely fast non-cryptographic hash. Optional seed parameter. Returns 8 hex chars.\n\n```perl\np xxh32(\"hello\")        # default seed 0\np xxh32(\"hello\", 42)    # with seed\n```",
        "xxh64" | "xxhash64" => "`xxh64` (alias `xxhash64`) computes xxHash64 — fast 64-bit hash. Returns 16 hex chars.\n\n```perl\np xxh64(\"hello\")\n```",
        "xxh3" | "xxhash3" => "`xxh3` (alias `xxhash3`) computes xxHash3-64 — newest xxHash variant, fastest on modern CPUs. Returns 16 hex chars.\n\n```perl\np xxh3(\"hello\")\n```",
        "xxh3_128" | "xxhash3_128" => "`xxh3_128` computes xxHash3-128. Returns 32 hex chars.\n\n```perl\np xxh3_128(\"hello\")\n```",
        // ── MurmurHash ──
        "murmur3" | "murmur3_32" => "`murmur3` (alias `murmur3_32`) computes MurmurHash3 32-bit. Fast non-cryptographic hash, widely used in hash tables and bloom filters. Optional seed. Returns 8 hex chars.\n\n```perl\np murmur3(\"hello\")       # default seed 0\np murmur3(\"hello\", 42)   # with seed\n```",
        "murmur3_128" => "`murmur3_128` computes MurmurHash3 128-bit (x64 variant). Returns 32 hex chars.\n\n```perl\np murmur3_128(\"hello\")\n```",
        // ── SipHash ──
        "siphash" => "`siphash` computes SipHash-2-4 with default keys (0,0) and returns a 16-character hex string (64-bit). SipHash is designed for hash table DoS resistance — it's fast and keyed, preventing attackers from crafting collisions. Used in Rust's HashMap, Python dict, and many other hash tables.\n\n```perl\np siphash \"key\"   # 16 hex chars\n```",
        "siphash_keyed" => "`siphash_keyed` computes SipHash-2-4 with a custom 128-bit key (two 64-bit integers). The key should be random per application to prevent collision attacks.\n\n```perl\np siphash_keyed(\"data\", 0x123456789, 0xabcdef012)  # keyed hash\n```",
        // ── HMAC Variants ──
        "hmac_sha1" => "`hmac_sha1` computes HMAC-SHA1 and returns a 40-character hex string. Used in OAuth 1.0, TOTP (Google Authenticator), and legacy APIs. Prefer HMAC-SHA256 for new designs.\n\n```perl\np hmac_sha1(\"secret\", \"message\")   # 40 hex chars\n```",
        "hmac_sha384" => "`hmac_sha384` computes HMAC-SHA384 and returns a 96-character hex string. Middle ground between SHA-256 and SHA-512.\n\n```perl\np hmac_sha384(\"key\", \"data\")   # 96 hex chars\n```",
        "hmac_sha512" => "`hmac_sha512` computes HMAC-SHA512 and returns a 128-character hex string. Maximum security margin in the SHA-2 family.\n\n```perl\np hmac_sha512(\"key\", \"data\")   # 128 hex chars\n```",
        "hmac_md5" => "`hmac_md5` computes HMAC-MD5 and returns a 32-character hex string. Legacy only — MD5 is broken for collision resistance but HMAC-MD5 is still considered safe for authentication. Only use for compatibility with old systems.\n\n```perl\np hmac_md5(\"key\", \"data\")   # 32 hex chars\n```",
        // ── HKDF ──
        "hkdf_sha256" | "hkdf" => "`hkdf_sha256` (alias `hkdf`) is HKDF key derivation (RFC 5869) using HMAC-SHA256. Extracts entropy from input key material and expands it to the desired length. Used to derive encryption keys from shared secrets (after ECDH/X25519). Args: ikm, salt, info, output_length.\n\n```perl\nmy $key = hkdf(\"shared_secret\", \"salt\", \"context\", 32)  # 64 hex\nmy $enc_key = hex_decode($key)  # 32 bytes for AES-256\n```",
        "hkdf_sha512" => "`hkdf_sha512` is HKDF using HMAC-SHA512. Higher security margin than SHA256 variant.\n\n```perl\nmy $key = hkdf_sha512(\"ikm\", \"salt\", \"info\", 64)  # 128 hex chars\n```",
        // ── Poly1305 ──
        "poly1305" | "poly1305_mac" => "`poly1305` computes a Poly1305 one-time MAC. Takes a 32-byte key and message, returns a 32-character hex tag (128-bit). Poly1305 is used with ChaCha20 in TLS 1.3. CRITICAL: each key must only be used once — reusing a key completely breaks security.\n\n```perl\nmy $key = random_bytes(32)\np poly1305($key, \"message\")   # 32 hex chars\n```",
        // ── RSA ──
        "rsa_keygen" => "`rsa_keygen` generates an RSA keypair. Takes key size in bits (2048, 3072, or 4096). Returns [private_key_pem, public_key_pem]. RSA is the most widely used asymmetric algorithm — essential for TLS, SSH, and JWT RS256 signing.\n\n```perl\nmy @kp = rsa_keygen(2048)\nmy ($priv, $pub) = @kp\nspurt(\"private.pem\", $priv)\nspurt(\"public.pem\", $pub)\n```",
        "rsa_encrypt" | "rsa_enc" => "`rsa_encrypt` (alias `rsa_enc`) encrypts data with RSA-OAEP-SHA256. Takes public_key_pem and plaintext. Returns base64 ciphertext. Message size is limited to key_size/8 - 66 bytes (e.g. 190 bytes for 2048-bit key).\n\n```perl\nmy $cipher = rsa_encrypt($pub_pem, \"secret\")\nmy $plain = rsa_decrypt($priv_pem, $cipher)\n```",
        "rsa_decrypt" | "rsa_dec" => "`rsa_decrypt` (alias `rsa_dec`) decrypts RSA-OAEP ciphertext. Takes private_key_pem and base64 ciphertext.\n\n```perl\nmy $plain = rsa_decrypt($priv, $ciphertext)\np $plain\n```",
        "rsa_encrypt_pkcs1" => "`rsa_encrypt_pkcs1` encrypts with legacy RSA-PKCS1v15 padding. Only for compatibility with old systems — prefer OAEP.\n\n```perl\nmy $cipher = rsa_encrypt_pkcs1($pub, \"data\")\n```",
        "rsa_decrypt_pkcs1" => "`rsa_decrypt_pkcs1` decrypts RSA-PKCS1v15 ciphertext.\n\n```perl\nmy $plain = rsa_decrypt_pkcs1($priv, $cipher)\n```",
        "rsa_sign" => "`rsa_sign` signs a message with RSA-PKCS1v15-SHA256. Takes private_key_pem and message. Returns base64 signature. This is the RS256 algorithm used in JWT.\n\n```perl\nmy $sig = rsa_sign($priv, \"message\")\nif (rsa_verify($pub, \"message\", $sig)) {\n    p \"valid\"\n}\n```",
        "rsa_verify" => "`rsa_verify` verifies an RSA-PKCS1v15-SHA256 signature. Takes public_key_pem, message, and base64 signature. Returns 1 if valid, 0 if not.\n\n```perl\nif (rsa_verify($pub_pem, $msg, $sig)) {\n    p \"signature valid\"\n}\n```",
        // ── ECDSA ──
        "ecdsa_p256_keygen" | "p256_keygen" => "`ecdsa_p256_keygen` (alias `p256_keygen`) generates an ECDSA P-256 (secp256r1/prime256v1) keypair. Returns [private_hex, public_hex_compressed]. P-256 is the NIST curve used in TLS, ES256 JWT, and WebAuthn.\n\n```perl\nmy @kp = ecdsa_p256_keygen()\nmy ($priv, $pub) = @kp\n```",
        "ecdsa_p256_sign" | "p256_sign" => "`ecdsa_p256_sign` (alias `p256_sign`) signs a message with ECDSA P-256. Takes private_key_hex and message. Returns DER-encoded signature as hex.\n\n```perl\nmy $sig = ecdsa_p256_sign($priv, \"hello\")\n```",
        "ecdsa_p256_verify" | "p256_verify" => "`ecdsa_p256_verify` (alias `p256_verify`) verifies an ECDSA P-256 signature. Returns 1 if valid.\n\n```perl\nif (ecdsa_p256_verify($pub, \"hello\", $sig)) {\n    p \"valid\"\n}\n```",
        "ecdsa_p384_keygen" | "p384_keygen" => "`ecdsa_p384_keygen` generates an ECDSA P-384 keypair. P-384 offers more security margin than P-256.\n\n```perl\nmy @kp = ecdsa_p384_keygen()\n```",
        "ecdsa_p384_sign" | "p384_sign" => "`ecdsa_p384_sign` signs with ECDSA P-384.\n\n```perl\nmy $sig = ecdsa_p384_sign($priv, $msg)\n```",
        "ecdsa_p384_verify" | "p384_verify" => "`ecdsa_p384_verify` verifies an ECDSA P-384 signature.\n\n```perl\np ecdsa_p384_verify($pub, $msg, $sig)\n```",
        "ecdsa_secp256k1_keygen" | "secp256k1_keygen" => "`ecdsa_secp256k1_keygen` generates an ECDSA secp256k1 keypair. This is the Bitcoin/Ethereum curve — different from P-256.\n\n```perl\nmy @kp = ecdsa_secp256k1_keygen()\n```",
        "ecdsa_secp256k1_sign" | "secp256k1_sign" => "`ecdsa_secp256k1_sign` signs with ECDSA secp256k1.\n\n```perl\nmy $sig = ecdsa_secp256k1_sign($priv, $msg)\n```",
        "ecdsa_secp256k1_verify" | "secp256k1_verify" => "`ecdsa_secp256k1_verify` verifies an ECDSA secp256k1 signature.\n\n```perl\np ecdsa_secp256k1_verify($pub, $msg, $sig)\n```",
        // ── ECDH ──
        "ecdh_p256" | "p256_dh" => "`ecdh_p256` (alias `p256_dh`) performs ECDH key exchange on P-256. Takes my_private_hex and their_public_hex, returns shared_secret_hex. Use HKDF to derive encryption keys from the shared secret.\n\n```perl\nmy @alice = ecdsa_p256_keygen()\nmy @bob = ecdsa_p256_keygen()\nmy $shared_a = ecdh_p256($alice[0], $bob[1])\nmy $shared_b = ecdh_p256($bob[0], $alice[1])\np $shared_a eq $shared_b  # 1 — same secret\n```",
        "ecdh_p384" | "p384_dh" => "`ecdh_p384` performs ECDH key exchange on P-384.\n\n```perl\nmy $shared = ecdh_p384($my_priv, $their_pub)\n```",
        // ── Base32 / Base58 ──
        "base32_encode" | "b32e" => "`base32_encode` (alias `b32e`) encodes data as RFC 4648 Base32. Used in TOTP secrets, onion addresses, and Bech32. Returns uppercase with padding.\n\n```perl\np base32_encode(\"hello\")   # NBSWY3DP\nmy $secret = base32_encode(random_bytes(20))\n```",
        "base32_decode" | "b32d" => "`base32_decode` (alias `b32d`) decodes RFC 4648 Base32 back to bytes. Accepts with or without padding.\n\n```perl\np base32_decode(\"NBSWY3DP\")   # hello\n```",
        "base58_encode" | "b58e" => "`base58_encode` (alias `b58e`) encodes data using Bitcoin's Base58 alphabet (no 0, O, I, l to avoid confusion). Used in Bitcoin addresses, IPFS CIDs.\n\n```perl\np base58_encode(\"hello\")   # Cn8eVZg\n```",
        "base58_decode" | "b58d" => "`base58_decode` (alias `b58d`) decodes Base58 back to bytes.\n\n```perl\np base58_decode(\"Cn8eVZg\")   # hello\n```",
        // ── TOTP / HOTP ──
        "totp" | "totp_generate" => "`totp` (alias `totp_generate`) generates a TOTP code (RFC 6238) for 2FA. Takes base32-encoded secret, optional digits (default 6), optional period (default 30s). Compatible with Google Authenticator, Authy, etc.\n\n```perl\nmy $secret = base32_encode(random_bytes(20))\nmy $code = totp($secret)\np $code   # 6-digit code\n# Custom: 8 digits, 60s period\nmy $code8 = totp($secret, 8, 60)\n```",
        "totp_verify" => "`totp_verify` verifies a TOTP code with optional time window (default ±1 period). Returns 1 if valid.\n\n```perl\nif (totp_verify($secret, $user_code)) {\n    p \"2FA valid\"\n}\n# Wider window: ±2 periods\ntotp_verify($secret, $code, 2)\n```",
        "hotp" | "hotp_generate" => "`hotp` (alias `hotp_generate`) generates an HOTP code (RFC 4226) using a counter. Takes base32 secret, counter value, optional digits.\n\n```perl\nmy $code = hotp($secret, 42)  # counter=42\n```",
        // ── AES-CBC ──
        "aes_cbc_encrypt" | "aes_cbc_enc" => "`aes_cbc_encrypt` (alias `aes_cbc_enc`) encrypts with AES-256-CBC and PKCS7 padding. Takes 32-byte key, plaintext, optional 16-byte IV (auto-generated if omitted). Returns base64(iv || ciphertext). Legacy mode — prefer `aes_encrypt` (GCM) for new code.\n\n```perl\nmy $key = random_bytes(32)\nmy $ct = aes_cbc_encrypt($key, \"secret\")\nmy $pt = aes_cbc_decrypt($key, $ct)\n```",
        "aes_cbc_decrypt" | "aes_cbc_dec" => "`aes_cbc_decrypt` (alias `aes_cbc_dec`) decrypts AES-256-CBC. Takes 32-byte key and base64(iv || ciphertext).\n\n```perl\nmy $pt = aes_cbc_decrypt($key, $ciphertext)\n```",
        // ── Blowfish ──
        "blowfish_encrypt" | "bf_enc" => "`blowfish_encrypt` (alias `bf_enc`) encrypts with Blowfish-CBC. Key=4-56 bytes, optional 8-byte IV. Legacy cipher — use AES for new code.\n\n```perl\nmy $ct = blowfish_encrypt($key, \"secret\")\nmy $pt = blowfish_decrypt($key, $ct)\n```",
        "blowfish_decrypt" | "bf_dec" => "`blowfish_decrypt` (alias `bf_dec`) decrypts Blowfish-CBC.\n\n```perl\nmy $pt = blowfish_decrypt($key, $ciphertext)\n```",
        // ── Triple DES (3DES) ──
        "des3_encrypt" | "3des_enc" | "tdes_enc" => "`des3_encrypt` (aliases `3des_enc`, `tdes_enc`) encrypts with Triple DES (3DES) CBC. Key=24 bytes (three 8-byte DES keys). Legacy cipher for PCI-DSS compliance.\n\n```perl\nmy $key = random_bytes(24)\nmy $ct = des3_encrypt($key, \"secret\")\nmy $pt = des3_decrypt($key, $ct)\n```",
        "des3_decrypt" | "3des_dec" | "tdes_dec" => "`des3_decrypt` (aliases `3des_dec`, `tdes_dec`) decrypts Triple DES (3DES) CBC.\n\n```perl\nmy $pt = des3_decrypt($key, $ciphertext)\n```",
        // ── Twofish ──
        "twofish_encrypt" | "tf_enc" => "`twofish_encrypt` (alias `tf_enc`) encrypts with Twofish-CBC. Key=16/24/32 bytes. AES finalist, still secure.\n\n```perl\nmy $key = random_bytes(32)\nmy $ct = twofish_encrypt($key, \"secret\")\n```",
        "twofish_decrypt" | "tf_dec" => "`twofish_decrypt` (alias `tf_dec`) decrypts Twofish-CBC.\n\n```perl\nmy $pt = twofish_decrypt($key, $ct)\n```",
        // ── Camellia ──
        "camellia_encrypt" | "cam_enc" => "`camellia_encrypt` (alias `cam_enc`) encrypts with Camellia-CBC. Key=16/24/32 bytes. Japanese/EU standard, equivalent security to AES.\n\n```perl\nmy $ct = camellia_encrypt($key, \"secret\")\n```",
        "camellia_decrypt" | "cam_dec" => "`camellia_decrypt` (alias `cam_dec`) decrypts Camellia-CBC.\n\n```perl\nmy $pt = camellia_decrypt($key, $ct)\n```",
        // ── CAST5 ──
        "cast5_encrypt" | "cast5_enc" => "`cast5_encrypt` encrypts with CAST5-CBC. Key=5-16 bytes. Used in PGP.\n\n```perl\nmy $ct = cast5_encrypt($key, \"secret\")\n```",
        "cast5_decrypt" | "cast5_dec" => "`cast5_decrypt` decrypts CAST5-CBC.\n\n```perl\nmy $pt = cast5_decrypt($key, $ct)\n```",
        // ── Salsa20 / XSalsa20 ──
        "salsa20" | "salsa20_encrypt" => "`salsa20` encrypts with Salsa20 stream cipher. Key=32 bytes, nonce auto-generated. Fast, secure stream cipher.\n\n```perl\nmy $ct = salsa20($key, \"data\")\nmy $pt = salsa20_decrypt($key, $ct)\n```",
        "salsa20_decrypt" => "`salsa20_decrypt` decrypts Salsa20.\n\n```perl\nmy $pt = salsa20_decrypt($key, $ct)\n```",
        "xsalsa20" | "xsalsa20_encrypt" => "`xsalsa20` encrypts with XSalsa20 (extended 24-byte nonce). Safer for random nonces.\n\n```perl\nmy $ct = xsalsa20($key, \"data\")\n```",
        "xsalsa20_decrypt" => "`xsalsa20_decrypt` decrypts XSalsa20.\n\n```perl\nmy $pt = xsalsa20_decrypt($key, $ct)\n```",
        // ── NaCl secretbox / box ──
        "secretbox" | "secretbox_seal" => "`secretbox` (alias `secretbox_seal`) is NaCl's symmetric authenticated encryption (XSalsa20-Poly1305). Key=32 bytes. Simple, secure, fast.\n\n```perl\nmy $key = random_bytes(32)\nmy $ct = secretbox($key, \"message\")\nmy $pt = secretbox_open($key, $ct)\n```",
        "secretbox_open" => "`secretbox_open` decrypts and authenticates NaCl secretbox.\n\n```perl\nmy $pt = secretbox_open($key, $ct)\n```",
        "nacl_box_keygen" | "box_keygen" => "`nacl_box_keygen` generates a NaCl box keypair (X25519). Returns [secret_key_hex, public_key_hex].\n\n```perl\nmy @kp = nacl_box_keygen()\nmy ($sk, $pk) = @kp\n```",
        "nacl_box" | "nacl_box_seal" | "box_seal" => "`nacl_box` is NaCl's asymmetric authenticated encryption. Takes recipient's public key, sender's secret key, plaintext.\n\n```perl\nmy @alice = nacl_box_keygen()\nmy @bob = nacl_box_keygen()\nmy $ct = nacl_box($bob[1], $alice[0], \"hello\")  # to Bob from Alice\nmy $pt = nacl_box_open($alice[1], $bob[0], $ct)  # Bob decrypts\n```",
        "nacl_box_open" | "box_open" => "`nacl_box_open` decrypts NaCl box. Takes sender's public key, recipient's secret key, ciphertext.\n\n```perl\nmy $pt = nacl_box_open($sender_pk, $my_sk, $ct)\n```",
        // ── QR Code ──
        "qr_ascii" | "qr" => "`qr_ascii` (alias `qr`) generates a QR code as ASCII art. Perfect for terminal output.\n\n```perl\np qr(\"https://example.com\")\np qr(\"otpauth://totp/App:user?secret=$secret&issuer=App\")\n```",
        "qr_png" => "`qr_png` generates a QR code as PNG image data (base64 encoded). Optional size parameter. Save to file or embed in HTML.\n\n```perl\nmy $png = qr_png(\"https://example.com\")\nspurt(\"qr.png\", base64_decode($png))\n# Larger QR\nmy $big = qr_png($url, 16)\n```",
        "qr_svg" => "`qr_svg` generates a QR code as SVG string. Scalable vector graphics, ideal for web.\n\n```perl\nmy $svg = qr_svg(\"https://example.com\")\nspurt(\"qr.svg\", $svg)\n```",
        // ── Barcode ──
        "barcode_code128" | "code128" => "`barcode_code128` (alias `code128`) generates a Code 128 barcode as ASCII. Code 128 supports alphanumeric data and is widely used in shipping labels.\n\n```perl\np code128(\"ABC-123\")\n```",
        "barcode_code39" | "code39" => "`barcode_code39` (alias `code39`) generates a Code 39 barcode. Supports uppercase, digits, and some symbols. Used in automotive and defense.\n\n```perl\np code39(\"HELLO123\")\n```",
        "barcode_ean13" | "ean13" => "`barcode_ean13` (alias `ean13`) generates an EAN-13 barcode (European Article Number). Standard retail barcode — requires exactly 12-13 digits.\n\n```perl\np ean13(\"5901234123457\")\n```",
        "barcode_svg" => "`barcode_svg` generates a barcode as SVG. Second argument specifies type: code128, code39, ean13, upca.\n\n```perl\nmy $svg = barcode_svg(\"ABC-123\", \"code128\")\nspurt(\"barcode.svg\", $svg)\nmy $retail = barcode_svg(\"012345678905\", \"upca\")\n```",
        // ── Compression Algorithms ──
        "brotli" | "br" => "`brotli` (alias `br`) compresses data using the Brotli algorithm (RFC 7932). Excellent compression ratio, used in HTTP compression. Returns compressed bytes.\n\n```perl\nmy $compressed = brotli($data)\np length($data) . \" -> \" . length($compressed)\n```",
        "brotli_decode" | "ubr" => "`brotli_decode` (alias `ubr`) decompresses Brotli data.\n\n```perl\nmy $original = brotli_decode($compressed)\n```",
        "xz" | "lzma" => "`xz` (alias `lzma`) compresses data using XZ/LZMA2. Best compression ratio, slower. Returns compressed bytes.\n\n```perl\nmy $compressed = xz($data)\nspurt(\"file.xz\", $compressed)\n```",
        "xz_decode" | "unxz" | "unlzma" => "`xz_decode` (aliases `unxz`, `unlzma`) decompresses XZ/LZMA data.\n\n```perl\nmy $original = xz_decode(slurp(\"file.xz\"))\n```",
        "bzip2" | "bz2" => "`bzip2` (alias `bz2`) compresses data using bzip2. Good compression, moderate speed. Returns compressed bytes.\n\n```perl\nmy $compressed = bzip2($data)\n```",
        "bzip2_decode" | "bunzip2" | "ubz2" => "`bzip2_decode` (aliases `bunzip2`, `ubz2`) decompresses bzip2 data.\n\n```perl\nmy $original = bunzip2($compressed)\n```",
        "lz4" => "`lz4` compresses data using LZ4. Very fast compression/decompression, moderate ratio. Ideal for real-time compression.\n\n```perl\nmy $compressed = lz4($data)\n```",
        "lz4_decode" | "unlz4" => "`lz4_decode` (alias `unlz4`) decompresses LZ4 data.\n\n```perl\nmy $original = lz4_decode($compressed)\n```",
        "snappy" | "snp" => "`snappy` (alias `snp`) compresses data using Snappy. Fastest compression, used in databases and RPC. Returns compressed bytes.\n\n```perl\nmy $compressed = snappy($data)\n```",
        "snappy_decode" | "unsnappy" => "`snappy_decode` (alias `unsnappy`) decompresses Snappy data.\n\n```perl\nmy $original = snappy_decode($compressed)\n```",
        "lzw" => "`lzw` compresses data using LZW (GIF/TIFF style). Classic algorithm.\n\n```perl\nmy $compressed = lzw($data)\n```",
        "lzw_decode" | "unlzw" => "`lzw_decode` (alias `unlzw`) decompresses LZW data.\n\n```perl\nmy $original = lzw_decode($compressed)\n```",
        // ── Archive Formats ──
        "tar_create" | "tar" => "`tar_create` (alias `tar`) creates a tar archive from a directory. Returns tar bytes.\n\n```perl\nmy $archive = tar_create(\"./src\")\nspurt(\"backup.tar\", $archive)\n```",
        "tar_extract" | "untar" => "`tar_extract` (alias `untar`) extracts a tar archive to a directory.\n\n```perl\ntar_extract(slurp(\"backup.tar\"), \"./restored\")\n```",
        "tar_list" => "`tar_list` lists files in a tar archive. Returns array of paths.\n\n```perl\nmy @files = @{tar_list(slurp(\"backup.tar\"))}\n@files |> e p\n```",
        "tar_gz_create" | "tgz" => "`tar_gz_create` (alias `tgz`) creates a gzipped tar archive. Convenience for tar + gzip.\n\n```perl\nmy $tgz = tgz(\"./project\")\nspurt(\"project.tar.gz\", $tgz)\n```",
        "tar_gz_extract" | "untgz" => "`tar_gz_extract` (alias `untgz`) extracts a .tar.gz archive.\n\n```perl\nuntgz(slurp(\"project.tar.gz\"), \"./extracted\")\n```",
        "zip_create" => "`zip_create` creates a ZIP archive from a directory. Returns zip bytes.\n\n```perl\nmy $archive = zip_create(\"./docs\")\nspurt(\"docs.zip\", $archive)\n```",
        "zip_extract" => "`zip_extract` extracts a ZIP archive to a directory.\n\n```perl\nzip_extract(slurp(\"docs.zip\"), \"./extracted\")\n```",
        "zip_list" => "`zip_list` lists files in a ZIP archive. Returns array of paths.\n\n```perl\nmy @files = @{zip_list(slurp(\"archive.zip\"))}\n```",
        "base64_encode" => "`base64_encode` (alias `b64e`) encodes a string or byte buffer as a Base64 string using the standard alphabet (A-Z, a-z, 0-9, +, /). Base64 is the standard way to embed binary data in text-based formats like JSON, XML, email (MIME), and data URIs. The output length is always a multiple of 4, padded with `=` as needed. Use `base64_decode` to reverse the encoding.\n\n```perl\nmy $encoded = base64_encode \"hello world\"\np $encoded   # aGVsbG8gd29ybGQ=\nmy $img_data = slurp \"photo.png\"\nmy $data_uri = \"data:image/png;base64,\" . base64_encode($img_data)\np base64_decode(base64_encode(\"round trip\"))  # round trip\n```",
        "base64_decode" => "`base64_decode` (alias `b64d`) decodes a Base64-encoded string back to its original bytes. It accepts standard Base64 with padding and is tolerant of line breaks within the input. This is essential for processing email attachments, decoding JWT payloads, extracting embedded images from data URIs, or reading any Base64-encoded field from an API response. Raises an exception on invalid Base64 input.\n\n```perl\nmy $decoded = base64_decode \"aGVsbG8gd29ybGQ=\"\np $decoded   # hello world\nmy $img = base64_decode($api_response->{avatar_b64})\nspurt \"avatar.png\", $img\nmy $json = base64_decode($jwt_parts[1])\ndd json_decode $json\n```",
        "hex_encode" => "`hex_encode` (alias `hxe`) converts a string or byte buffer into its lowercase hexadecimal representation, with two hex characters per input byte. This is useful for displaying binary data in a human-readable format, generating hex-encoded keys or IDs, logging raw bytes, or preparing data for protocols that use hex encoding. The output is always an even number of characters.\n\n```perl\np hex_encode \"hello\"   # 68656c6c6f\nmy $raw = slurp \"key.bin\"\np hex_encode $raw\nmy $color = hex_encode chr(255) . chr(128) . chr(0)\np \"#$color\"   # #ff8000\n```",
        "hex_decode" => "`hex_decode` (alias `hxd`) converts a hexadecimal string back to its original bytes, interpreting every two hex characters as one byte. This is the inverse of `hex_encode` and is useful for parsing hex-encoded binary data from config files, network protocols, or cryptographic outputs. The input must have an even number of valid hex characters (0-9, a-f, A-F) or an exception is raised.\n\n```perl\nmy $bytes = hex_decode \"68656c6c6f\"\np $bytes   # hello\nmy $key = hex_decode $env_hex_key\nmy $mac = hmac_sha256($data, hex_decode($secret_hex))\np hex_decode(hex_encode(\"round trip\"))  # round trip\n```",
        "uuid" => "`uuid` generates a cryptographically random UUID version 4 string in the standard 8-4-4-4-12 hyphenated format. Each call produces a unique identifier suitable for database primary keys, correlation IDs, session tokens, temporary file names, or any situation requiring a globally unique identifier without coordination. The randomness comes from the OS CSPRNG.\n\n```perl\nmy $id = uuid()\np $id   # e.g., 550e8400-e29b-41d4-a716-446655440000\nmy %record = (id => uuid(), name => \"Alice\", created => time)\nmy @ids = map { uuid() } 1..10\n```",
        "jwt_encode" => "`jwt_encode` creates a signed JSON Web Token from a payload hashref and a secret key. The default algorithm is HS256 (HMAC-SHA256), but you can specify an alternative as the third argument. JWTs are the standard for stateless authentication tokens, API authorization, and secure inter-service communication. The returned string contains the base64url-encoded header, payload, and signature separated by dots.\n\n```perl\nmy $token = jwt_encode({sub => \"user123\", exp => time + 3600}, \"my-secret\")\np $token\nmy $admin = jwt_encode({role => \"admin\", iat => time}, $secret, \"HS512\")\n# send as Authorization header\nhttp_request(method => \"GET\", url => $api_url,\n    headers => {Authorization => \"Bearer $token\"})\n```",
        "jwt_decode" => "`jwt_decode` verifies the signature of a JSON Web Token using the provided secret key and returns the decoded payload as a hashref. If the signature is invalid, the token has been tampered with, or it has expired (when an `exp` claim is present), the function raises an exception. This is the secure way to validate incoming JWTs from clients or other services — always use this over `jwt_decode_unsafe` in production.\n\n```perl\nmy $payload = jwt_decode($token, \"my-secret\")\np $payload->{sub}   # user123\nmy $claims = jwt_decode($bearer_token, $secret)\nif ($claims->{role} eq \"admin\") {\n    p \"admin access granted\"\n}\n```\n\nNote: raises an exception on expired tokens, invalid signatures, or malformed input.",
        "jwt_decode_unsafe" => "`jwt_decode_unsafe` decodes a JSON Web Token and returns the payload as a hashref without verifying the signature. This is intentionally insecure and should only be used for debugging, logging, or inspecting token contents in development environments. Never use this to make authorization decisions in production — an attacker can stryke arbitrary payloads. The function still parses the JWT structure and base64-decodes the payload, but skips all cryptographic checks.\n\n```perl\n# debugging only — never use for auth\nmy $claims = jwt_decode_unsafe($token)\ndd $claims\np $claims->{sub}   # inspect without needing the secret\nmy $exp = $claims->{exp}\np \"Expires: \" . datetime_from_epoch($exp)$1\n\nNote: this function exists for debugging. Use `jwt_decode` with a secret for any security-relevant validation.",

        // ── File I/O helpers ──
        "read_lines" | "rl" => "Read a file and return its contents as a list of lines with trailing newlines stripped. This is the idiomatic way to slurp a file line-by-line in stryke without manually opening a filehandle. The short alias `rl` keeps one-liners concise. If the file does not exist, the program dies with an error message.\n\n```perl\nmy @lines = rl(\"data.txt\")\np scalar @lines               # line count\n@lines |> grep /ERROR/ |> e p # print error lines\nmy $first = (rl \"config.ini\")[0]$1\n\nNote: returns an empty list for an empty file.",
        "append_file" | "af" => "Append a string to the end of a file, creating it if it does not exist. This is the safe way to add content without overwriting — useful for log files, CSV accumulation, or incremental output. The short alias `af` is convenient in pipelines. The file is opened, written, and closed atomically per call.\n\n```perl\naf(\"log.txt\", \"started at \" . datetime_utc() . \"\\n\")\n1..5 |> e { af(\"nums.txt\", \"$_\\n\") }\nmy @data = (\"a\",\"b\",\"c\")\n@data |> e { af \"out.txt\", \"$_\\n\" }\n```",
        "to_file" => "Write a string to a file, truncating any existing content. Unlike `append_file`, this replaces the file entirely. Returns the written content so it can be used in a pipeline — write to disk and continue processing in one expression. Creates the file if it does not exist.\n\n```perl\nmy $csv = \"name,age\\nAlice,30\\nBob,25\"\n$csv |> to_file(\"people.csv\") |> p\nto_file(\"empty.txt\", \"\")  # truncate a file\n```\n\nNote: the return-value-for-piping behavior distinguishes this from a plain write.",
        "tempfile" | "tf" => "Create a temporary file in the system temp directory and return its absolute path as a string. The file is created with a unique name and exists on disk immediately. Use `tf` as a short alias for quick scratch files in one-liners. The caller is responsible for cleanup, though OS temp-directory reaping will eventually reclaim it.\n\n```perl\nmy $tmp = tf()\nto_file($tmp, \"scratch data\\n\")\np rl($tmp)           # scratch data\nmy @all = map { tf() } 1..3  # three temp files\n```",
        "tempdir" | "tdr" => "Create a temporary directory in the system temp directory and return its absolute path. The directory is created with a unique name and is ready for use immediately. The short alias `tdr` mirrors `tf` for files. Useful for isolating multi-file operations like test fixtures, build artifacts, or staged output.\n\n```perl\nmy $dir = tdr()\nto_file(\"$dir/a.txt\", \"hello\")\nto_file(\"$dir/b.txt\", \"world\")\nmy @files = glob(\"$dir/*.txt\")\np scalar @files   # 2\n```",
        "read_json" | "rj" => "Read a JSON file from disk and parse it into a stryke data structure (hash ref or array ref). The short alias `rj` keeps JSON-config one-liners terse. Dies if the file does not exist or contains malformed JSON. This is the complement of `write_json`/`wj`.\n\n```perl\nmy $cfg = rj(\"config.json\")\np $cfg->{database}{host}\nmy @items = @{ rj(\"list.json\") }\n@items |> e { p $_->{name} }$1\n\nNote: numeric strings remain strings; use `+0` to coerce if needed.",
        "write_json" | "wj" => "Serialize a stryke data structure (hash ref or array ref) as pretty-printed JSON and write it to a file. Creates or overwrites the target file. The short alias `wj` pairs with `rj` for round-trip JSON workflows. Useful for persisting configuration, caching API responses, or generating fixture data.\n\n```perl\nmy %data = (name => \"Alice\", scores => [98, 87, 95])\nwj(\"out.json\", \\%data)\nmy $back = rj(\"out.json\")\np $back->{name}   # Alice\n```",

        // ── Compression ──
        "gzip" => "Compress a string or byte buffer using the gzip (RFC 1952) format and return the compressed bytes. Useful for shrinking data before writing to disk or sending over the network. Pairs with `gunzip` for decompression. The compression level is chosen automatically for a good speed/size tradeoff.\n\n```perl\nmy $raw = \"hello world\" x 1000\nmy $gz = gzip($raw)\nto_file(\"data.gz\", $gz)\np length($gz)       # much smaller than original\np gunzip($gz) eq $raw  # 1\n```",
        "gunzip" => "Decompress gzip-compressed data (RFC 1952) and return the original bytes. Dies if the input is not valid gzip. Use this to read `.gz` files or decompress data received from HTTP responses with `Content-Encoding: gzip`. Always the inverse of `gzip`.\n\n```perl\nmy $compressed = rl(\"archive.gz\")\nmy $text = gunzip($compressed)\np $text\n# round-trip in a pipeline\n\"payload\" |> gzip |> gunzip |> p  # payload\n```",
        "zstd" => "Compress a string or byte buffer using the Zstandard algorithm and return the compressed bytes. Zstandard offers significantly better compression ratios and speed compared to gzip, making it ideal for large datasets, IPC buffers, and caching. Pairs with `zstd_decode` for decompression.\n\n```perl\nmy $big = \"x]\" x 100_000\nmy $compressed = zstd($big)\np length($compressed)  # fraction of original\nto_file(\"data.zst\", $compressed)\np zstd_decode($compressed) eq $big  # 1\n```",
        "zstd_decode" => "Decompress Zstandard-compressed data and return the original bytes. Dies if the input is not valid Zstandard. This is the inverse of `zstd`. Use it to read `.zst` files or decompress cached buffers that were compressed with `zstd`.\n\n```perl\nmy $packed = zstd(\"important data\\n\" x 500)\nmy $original = zstd_decode($packed)\np $original\n# file round-trip\nto_file(\"cache.zst\", zstd($payload))\np zstd_decode(rl(\"cache.zst\"))\n```",

        // ── URL encoding ──
        "url_encode" | "uri_escape" => "Percent-encode a string so it is safe to embed in a URL query parameter or path segment. Unreserved characters (alphanumeric, `-`, `_`, `.`, `~`) are left as-is; everything else becomes `%XX`. The alias `uri_escape` matches the classic `URI::Escape` name for Perl muscle-memory.\n\n```perl\nmy $q = \"hello world & friends\"\nmy $safe = url_encode($q)\np $safe   # hello%20world%20%26%20friends\nmy $url = \"https://example.com/search?q=\" . url_encode($q)\np $url$1\n\nNote: does not encode the full URL structure — encode individual components, not the whole URL.",
        "url_decode" | "uri_unescape" => "Decode a percent-encoded string back to its original form, converting `%XX` sequences to the corresponding bytes and `+` to space. The alias `uri_unescape` matches `URI::Escape` conventions. Use this when parsing query strings from incoming URLs or reading URL-encoded form data.\n\n```perl\nmy $encoded = \"hello%20world%20%26%20friends\"\np url_decode($encoded)   # hello world & friends\n# round-trip\nmy $orig = \"café ☕\"\np url_decode(url_encode($orig)) eq $orig  # 1\n```",

        // ── Logging ──
        "log_info" => "Log a message at INFO level to stderr with a timestamp prefix. INFO is the default visible level and is appropriate for normal operational messages — startup notices, progress milestones, summary statistics. Messages are suppressed if the current log level is set higher than INFO.\n\n```perl\nlog_info(\"server started on port $port\")\nmy @rows = rl(\"data.csv\")\nlog_info(\"loaded \" . scalar(@rows) . \" rows\")\n1..5 |> e { log_info(\"processing item $_\") }\n```",
        "log_warn" => "Log a message at WARN level to stderr. Warnings indicate unexpected but recoverable situations — missing optional config, deprecated usage, slow operations. WARN messages appear at the default log level and are visually distinct from INFO in structured log output.\n\n```perl\nlog_warn(\"config file not found, using defaults\")\nlog_warn(\"query took ${elapsed}s, exceeds threshold\")\nunless (-e $path) {\n    log_warn(\"$path missing, skipping\")\n}\n```",
        "log_error" => "Log a message at ERROR level to stderr. Use this for failures that prevent an operation from completing but do not necessarily terminate the program — failed network requests, invalid input, permission errors. ERROR is always visible regardless of log level.\n\n```perl\nlog_error(\"failed to connect to $host: $!\")\neval { rj(\"bad.json\") }\nlog_error(\"parse failed: $@\") if $@\nlog_error(\"missing required field 'name'\")\n```",
        "log_debug" => "Log a message at DEBUG level to stderr. Debug messages are hidden by default and only appear when the log level is lowered to DEBUG or TRACE via `log_level`. Use for detailed internal state that helps during development — variable dumps, branch decisions, intermediate values.\n\n```perl\nlog_level(\"debug\")\nlog_debug(\"cache key: $key\")\nmy $result = compute($x)\nlog_debug(\"compute($x) => $result\")\n@items |> e { log_debug(\"item: $_\") }\n```",
        "log_trace" => "Log a message at TRACE level to stderr. This is the most verbose level, producing very fine-grained output — loop iterations, function entry/exit, raw payloads. Only visible when `log_level(\"trace\")` is set. Use sparingly in production code; primarily for deep debugging sessions.\n\n```perl\nlog_level(\"trace\")\nfn process($x) {\n    log_trace(\"entering process($x)\")\n    my $r = $x * 2\n    log_trace(\"leaving process => $r\")\n    $r\n}\n1..3 |> map process |> e p\n```",
        "log_json" => "Emit a structured JSON log line to stderr containing the message plus any additional key-value metadata. This is designed for machine-parseable logging pipelines — centralized log collectors, JSON-based monitoring, or `jq`-friendly output. Each call emits exactly one JSON object per line.\n\n```perl\nlog_json(\"request\", method => \"GET\", path => \"/api\")\nlog_json(\"metric\", name => \"latency_ms\", value => 42)\nlog_json(\"error\", msg => $@, file => $0)$1\n\nNote: all values are serialized as JSON strings.",
        "log_level" => "Get or set the current minimum log level. When called with no arguments, returns the current level as a string. When called with a level name, sets it for all subsequent log calls. Valid levels from most to least verbose: `trace`, `debug`, `info`, `warn`, `error`. The default level is `info`.\n\n```perl\np log_level()         # info\nlog_level(\"debug\")    # enable debug output\nlog_debug(\"now visible\")\nlog_level(\"error\")    # suppress everything below error\nlog_info(\"hidden\")    # not printed\n```",

        // ── Datetime ──
        "datetime_utc" => "Return the current UTC date and time as an ISO 8601 string (e.g. `2026-04-15T12:30:00Z`). This is the simplest way to get a portable, unambiguous timestamp for logging, file naming, or serialization. The returned string always ends with `Z` indicating UTC, so there is no timezone ambiguity.\n\n```perl\nmy $now = datetime_utc()\np $now                          # 2026-04-15T12:30:00Z\naf(\"audit.log\", \"$now: started\\n\")\nmy %event = (ts => datetime_utc(), action => \"deploy\")\nwj(\"event.json\", \\%event)\n```",
        "datetime_from_epoch" => "Convert a Unix epoch timestamp (seconds since 1970-01-01 00:00:00 UTC) into an ISO 8601 datetime string. This is useful when you have raw epoch values from `time()`, file modification times, or external APIs and need a human-readable representation. Fractional seconds are truncated.\n\n```perl\nmy $ts = 1700000000\np datetime_from_epoch($ts)       # 2023-11-14T22:13:20Z\nmy $born = datetime_from_epoch(0)\np $born                          # 1970-01-01T00:00:00Z\nmy @epochs = (1e9, 1.5e9, 2e9)\n@epochs |> e { p datetime_from_epoch }\n```",
        "datetime_strftime" => "Format an epoch timestamp using a `strftime`-style format string, giving full control over the output representation. The first argument is the format pattern and the second is the epoch value. Supports all standard specifiers: `%Y` (4-digit year), `%m` (month), `%d` (day), `%H` (hour), `%M` (minute), `%S` (second), `%A` (weekday name), and more.\n\n```perl\nmy $t = time()\np datetime_strftime(\"%Y-%m-%d\", $t)        # 2026-04-15\np datetime_strftime(\"%H:%M:%S\", $t)        # 14:23:07\np datetime_strftime(\"%A, %B %d\", $t)       # Wednesday, April 15\nmy $log_ts = datetime_strftime(\"%Y%m%d_%H%M%S\", $t)\nto_file(\"backup_$log_ts.sql\", $data)\n```",
        "datetime_now_tz" => "Return the current date and time in a specified IANA timezone as a formatted string. Pass a timezone name like `America/New_York`, `Europe/London`, or `Asia/Tokyo`. This avoids manual UTC-offset arithmetic and handles daylight saving transitions correctly. Dies if the timezone name is not recognized.\n\n```perl\np datetime_now_tz(\"America/New_York\")    # 2026-04-15 08:30:00 EDT\np datetime_now_tz(\"Asia/Tokyo\")           # 2026-04-15 21:30:00 JST\np datetime_now_tz(\"UTC\")                  # same as datetime_utc\nmy @offices = (\"US/Pacific\", \"Europe/Berlin\", \"Asia/Kolkata\")\n@offices |> e { p \"$_: \" . datetime_now_tz }\n```",
        "datetime_format_tz" => "Format an epoch timestamp in a specific IANA timezone, combining the capabilities of `datetime_strftime` and `datetime_now_tz`. This lets you render a historical or future timestamp as it would appear on the wall clock in any timezone. Handles DST transitions automatically.\n\n```perl\nmy $epoch = 1700000000\np datetime_format_tz($epoch, \"America/Chicago\")\n# 2023-11-14 16:13:20 CST\np datetime_format_tz($epoch, \"Europe/London\")\n# 2023-11-14 22:13:20 GMT\np datetime_format_tz(time(), \"Australia/Sydney\")\n```",
        "datetime_parse_local" => "Parse a local datetime string (without timezone info) into a Unix epoch timestamp, interpreting it in the system's local timezone. Accepts common formats like `2026-04-15 14:30:00` or `2026-04-15T14:30:00`. Dies if the string cannot be parsed. This is the inverse of formatting with `localtime`.\n\n```perl\nmy $epoch = datetime_parse_local(\"2026-04-15 14:30:00\")\np $epoch                          # Unix timestamp\np datetime_from_epoch($epoch)     # back to ISO 8601\nmy $midnight = datetime_parse_local(\"2026-01-01 00:00:00\")\np time() - $midnight              # seconds since New Year\n```",
        "datetime_parse_rfc3339" => "Parse an RFC 3339 / ISO 8601 datetime string (with timezone offset or `Z` suffix) into a Unix epoch timestamp. This is the standard format used by JSON APIs, RSS feeds, and git timestamps. Accepts strings like `2026-04-15T14:30:00Z` or `2026-04-15T14:30:00+05:30`. Dies on malformed input.\n\n```perl\nmy $epoch = datetime_parse_rfc3339(\"2026-04-15T12:00:00Z\")\np $epoch\nmy $with_tz = datetime_parse_rfc3339(\"2026-04-15T08:00:00-04:00\")\np $epoch == $with_tz              # 1 (same instant)\n# parse API response timestamps\nmy $created = $response->{created_at}\nmy $age = time() - datetime_parse_rfc3339($created)\np \"created ${age}s ago\"\n```",
        "datetime_add_seconds" => "Add (or subtract) a number of seconds to an ISO 8601 datetime string and return the resulting ISO 8601 string. This performs calendar-aware arithmetic, correctly crossing day, month, and year boundaries. Pass a negative number to subtract time. Useful for computing deadlines, expiration times, or time windows.\n\n```perl\nmy $now = datetime_utc()\nmy $later = datetime_add_seconds($now, 3600)     # +1 hour\np $later\nmy $yesterday = datetime_add_seconds($now, -86400) # -1 day\np $yesterday\nmy $deadline = datetime_add_seconds($now, 7 * 86400) # +1 week\np \"due by $deadline\"\n```",
        "elapsed" | "el" => "Return the number of seconds elapsed since the stryke process started, using a monotonic clock that is immune to system clock adjustments. The short alias `el` keeps benchmarking one-liners terse. Returns a floating-point value with sub-millisecond precision. Useful for timing operations, profiling hot loops, or adding relative timestamps to log output.\n\n```perl\nmy $t0 = el()\nmy @sorted = sort @big_array\nmy $dur = el() - $t0\np \"sort took ${dur}s\"\n# progress logging\n1..100 |> e { do_work\n    log_info(\"step $_ at \" . el() . \"s\") }\n```",
        "time" => "Return the current Unix epoch as an integer — the number of seconds since 1970-01-01 00:00:00 UTC. This is the standard wall-clock timestamp used for file times, database records, and interop with external systems. For monotonic timing of code sections, prefer `elapsed`/`el` instead since `time` can jump if the system clock is adjusted.\n\n```perl\nmy $start = time()\nsleep(2)\np time() - $start   # ~2\nmy $ts = time()\nwj(\"stamp.json\", { created => $ts })\np datetime_from_epoch($ts)  # human-readable form\n```",
        "times" => "Return the accumulated CPU times for the process as a four-element list: `($user, $system, $child_user, $child_system)`. User time is CPU spent executing your code; system time is CPU spent in kernel calls on your behalf. Child times cover subprocesses. Values are in seconds (floating point). Useful for profiling whether a script is CPU-bound or I/O-bound.\n\n```perl\nmy ($u, $s, $cu, $cs) = times()\np \"user=${u}s sys=${s}s\"\n# after heavy computation\nmy ($u2, $s2) = times()\np \"used \" . ($u2 - $u) . \"s of CPU\"\np \"total CPU: \" . ($u2 + $s2) . \"s\"\n```",
        "localtime" => "Convert a Unix epoch timestamp to a nine-element list of broken-down local time components: `($sec, $min, $hour, $mday, $mon, $year, $wday, $yday, $isdst)`. Follows the Perl convention where `$mon` is 0-based (January=0) and `$year` is years since 1900. When called without arguments, uses the current time. Use `gmtime` for the UTC equivalent.\n\n```perl\nmy @t = localtime(time())\np \"$t[2]:$t[1]:$t[0]\"                 # HH:MM:SS\nmy $year = $t[5] + 1900\nmy $mon  = $t[4] + 1\np \"$year-$mon-$t[3]\"                   # YYYY-M-D\nmy @days = qw(Sun Mon Tue Wed Thu Fri Sat)\np $days[$t[6]]                          # day of week\n```",
        "gmtime" => "Convert a Unix epoch timestamp to a nine-element list of broken-down UTC time components, identical in structure to `localtime` but always in the UTC timezone. The fields are `($sec, $min, $hour, $mday, $mon, $year, $wday, $yday, $isdst)` where `$isdst` is always 0. When called without arguments, uses the current time.\n\n```perl\nmy @utc = gmtime(time())\nmy $year = $utc[5] + 1900\nmy $mon  = $utc[4] + 1\np sprintf(\"%04d-%02d-%02dT%02d:%02d:%02dZ\",\n    $year, $mon, @utc[3,2,1,0])\n# compare local vs UTC\nmy @loc = localtime(time())\np \"UTC hour=$utc[2] local hour=$loc[2]\"\n```",
        "sleep" => "Pause execution for the specified number of seconds. Accepts both integer and fractional values for sub-second sleeps (e.g. `sleep 0.1` for 100ms). The process yields the CPU during the sleep, so it is safe to use in polling loops without burning cycles. Returns the unslept time (always 0 unless interrupted by a signal).\n\n```perl\np \"waiting...\"\nsleep(2)\np \"done\"\n# polling loop\nwhile (!-e \"done.flag\") {\n    sleep(0.5)\n}\n# rate limiting\nmy @urls = @targets\n@urls |> e { fetch\n    sleep(0.1) }\n```",
        "alarm" => "Schedule a `SIGALRM` signal to be delivered to the process after the specified number of seconds. Calling `alarm(0)` cancels any pending alarm. Only one alarm can be active at a time — setting a new alarm replaces the previous one. Returns the number of seconds remaining on the previous alarm (or 0 if none was set). Combine with `eval` and `$SIG{ALRM}` to implement timeouts around potentially hanging operations.\n\n```perl\neval {\n    local $SIG{ALRM} = fn { die \"timeout\\n\" }\n    alarm(5)           # 5 second deadline\n    my $data = slow_network_call()\n    alarm(0)           # cancel on success\n}\nif ($@ =~ /timeout/) {\n    log_error(\"operation timed out\")\n}\n```",

        // ── File / path utilities ──
        "basename" | "bn" => "Extract the filename component from a path, stripping all leading directory segments. The short alias `bn` keeps one-liner pipelines terse. If an optional suffix argument is provided, that suffix is also stripped from the result, which is handy for removing extensions.\n\n```perl\np basename(\"/usr/local/bin/stryke\")        # stryke\np bn(\"/tmp/data.csv\", \".csv\")           # data\n\"/etc/nginx/nginx.conf\" |> bn |> p      # nginx.conf\n```",
        "dirname" | "dn" => "Return the directory portion of a path, stripping the final filename component. The short alias `dn` mirrors `bn`. This is a pure string operation — it does not touch the filesystem, so it works on paths that do not exist yet. Useful for deriving output directories from input file paths.\n\n```perl\np dirname(\"/usr/local/bin/stryke\")          # /usr/local/bin\np dn(\"/tmp/data.csv\")                   # /tmp\nmy $dir = dn($0)                        # directory of current script\n```",
        "fileparse" => "Split a path into its three logical components: the filename, the directory prefix, and a suffix that matches one of the supplied patterns. This mirrors Perl's `File::Basename::fileparse` and is the most flexible path decomposition available. When no suffix patterns are given the suffix is empty.\n\n```perl\nmy ($name, $dir, $sfx) = fileparse(\"/home/user/report.txt\", qr/\\.txt/)\np \"$dir | $name | $sfx\"  # /home/user/ | report | .txt\nmy ($n, $d) = fileparse(\"./lib/Foo/Bar.pm\")\np \"$d$n\"                 # ./lib/Foo/Bar.pm\n```",
        "canonpath" => "Clean up a file path by collapsing redundant separators, resolving `.` and `..` segments, and normalizing trailing slashes — all without touching the filesystem. Unlike `realpath`, this is a purely lexical operation so it works on paths that do not exist. Use it to normalize user-supplied paths before comparison or storage.\n\n```perl\np canonpath(\"/usr/./local/../local/bin/\")   # /usr/local/bin\np canonpath(\"a/b/../c\")                     # a/c\nmy $clean = canonpath($ENV{HOME} . \"/./docs/../docs\")\np $clean\n```",
        "realpath" | "rp" => "Resolve a path to its absolute canonical form by following all symbolic links and eliminating `.` and `..` segments. Unlike `canonpath`, this hits the filesystem and will die if any component does not exist. The short alias `rp` is convenient in pipelines. Use this when you need a guaranteed unique path for deduplication or comparison.\n\n```perl\np realpath(\".\")                      # /home/user/project\np rp(\"../sibling\")                   # /home/user/sibling\nmy $canon = rp($0)                  # absolute path of current script\n\".\" |> rp |> p\n```",
        "getcwd" | "pwd" => "Return the current working directory as an absolute path string. This calls the underlying OS `getcwd` function, so it always reflects the real directory even if the process changed directories via `chdir`. The alias `pwd` matches the familiar shell command. Often used to save and restore directory context around `chdir` calls.\n\n```perl\nmy $orig = pwd()\nchdir(\"/tmp\")\np pwd()      # /tmp\nchdir($orig)\np getcwd()   # back to original\n```",
        "gethostname" | "hn" => "Return the hostname of the current machine as a string. This calls the POSIX `gethostname` system call. The short alias `hn` is useful in log prefixes, temp-file naming, or distributed-system identifiers where you need to tag output by machine.\n\n```perl\np gethostname()                          # myhost.local\nmy $log_prefix = hn() . \":\" . $$        # myhost.local:12345\nlog_info(\"running on \" . hn())\n```",
        "which" => "Search the `PATH` environment variable for the first executable matching the given command name and return its absolute path, or `undef` if not found. This is the programmatic equivalent of the shell `which` command. Useful for checking tool availability before calling `system` or `exec`.\n\n```perl\nmy $gcc = which(\"gcc\") // die \"gcc not found\"\np $gcc                       # /usr/bin/gcc\nif (which(\"rg\")) {\n    system(\"rg pattern file\")\n} else {\n    system(\"grep pattern file\")\n}\n```",
        "which_all" | "wha" => "Return a list of all absolute paths matching the given command name across every directory in `PATH`, not just the first match. The short alias `wha` keeps things concise. This is useful for detecting shadowed executables or auditing which versions of a tool are installed.\n\n```perl\nmy @all = which_all(\"python3\")\n@all |> e p                # /usr/local/bin/python3\n                             # /usr/bin/python3\np scalar wha(\"perl\")        # number of perls on PATH\n```",
        "glob_match" => "Test whether a filename or path matches a shell-style glob pattern. Returns true (1) on match, false (empty string) otherwise. Supports `*`, `?`, `[abc]`, and `{a,b}` patterns. This is a pure string match — it does not read the filesystem, so it works for filtering lists of paths you already have.\n\n```perl\np glob_match(\"*.pl\", \"script.pl\")        # 1\np glob_match(\"*.pl\", \"script.py\")        # (empty)\nmy @perl = grep { glob_match(\"*.{pl,pm}\", $_) } @files\n@perl |> e p\n```",
        "copy" => "Copy a file from a source path to a destination path. The destination can be a file path or a directory (in which case the source filename is preserved). Dies on failure. This is the programmatic equivalent of `cp` and avoids shelling out. Metadata such as permissions is preserved where possible.\n\n```perl\ncopy(\"src/config.yaml\", \"/tmp/config.yaml\")\ncopy(\"report.pdf\", \"/backup/\")\nmy $tmp = tf()\ncopy($0, $tmp)  # back up the current script\np slurp($tmp)\n```",
        "move" | "mv" => "Move or rename a file from source to destination. If the source and destination are on the same filesystem this is an atomic rename; otherwise it falls back to copy-then-delete. Dies on failure. The short alias `mv` mirrors the shell command.\n\n```perl\nmove(\"draft.txt\", \"final.txt\")         # rename in place\nmv(\"output.csv\", \"/archive/output.csv\") # move across dirs\nmy $tmp = tf()\nspurt($tmp, \"data\")\nmv($tmp, \"data.txt\")\n```",
        "read_bytes" | "slurp_raw" => "Read an entire file into memory as raw bytes without any encoding interpretation. Unlike `slurp`, which returns a decoded UTF-8 string, `read_bytes` preserves the exact byte content — useful for binary files like images, compressed archives, or protocol buffers. The alias `slurp_raw` emphasizes the raw nature.\n\n```perl\nmy $png = read_bytes(\"logo.png\")\np length($png)                        # byte count\nmy $gz = slurp_raw(\"data.gz\")\nmy $text = gunzip($gz)\np $text\n```",
        "spurt" | "write_file" | "wf" => "Write a string to a file, creating it if it does not exist or truncating it if it does. This is the complement of `slurp` — together they form a read/write pair for whole-file operations. The short alias `wf` is convenient for one-liners. The file is opened, written, and closed in a single call.\n\n```perl\nspurt(\"hello.txt\", \"Hello, world!\\n\")\nwf(\"nums.txt\", join(\"\\n\", 1..10))\n\"generated content\" |> wf(\"out.txt\")\nmy $data = slurp(\"in.txt\")\nwf(\"copy.txt\", $data)\n```",
        "mkdir" => "Create a directory at the given path. An optional second argument specifies the permission mode as an octal number (default `0777`, modified by the current `umask`). Dies if the directory cannot be created. Only creates one level — use `make_path` or shell out for recursive creation.\n\n```perl\nmkdir(\"output\")\nmkdir(\"/tmp/secure\", 0700)\nmy $dir = tdr() . \"/sub\"\nmkdir($dir)\np -d $dir    # 1\n```",
        "rmdir" => "Remove an empty directory. Dies if the directory does not exist, is not empty, or cannot be removed due to permissions. This only removes a single directory — it will not recursively delete contents. Remove files with `unlink` first, then call `rmdir`.\n\n```perl\nmkdir(\"scratch\")\nrmdir(\"scratch\")\np -d \"scratch\"   # (empty, dir is gone)\nunlink(\"tmp/file.txt\")\nrmdir(\"tmp\")\n```",
        "unlink" => "Delete one or more files from the filesystem. Returns the number of files successfully removed. Does not remove directories — use `rmdir` for those. Dies on permission errors. Accepts a list of paths, making it convenient for batch cleanup.\n\n```perl\nunlink(\"temp.log\")\nmy $n = unlink(\"a.tmp\", \"b.tmp\", \"c.tmp\")\np \"removed $n files\"\nmy @old = glob(\"*.bak\")\nunlink(@old)\n```",
        "rename" => "Rename a file or directory from an old name to a new name. This is an atomic operation on the same filesystem. If the destination already exists it is silently replaced. Dies on failure. Unlike `move`/`mv`, this does not fall back to copy-then-delete across filesystems.\n\n```perl\nrename(\"draft.md\", \"final.md\")\nrename(\"output\", \"output_v2\")          # works on dirs too\nmy $bak = \"config.yaml.bak\"\nrename(\"config.yaml\", $bak)\nspurt(\"config.yaml\", $new_config)\n```",
        "link" => "Create a hard link — a new directory entry pointing to the same underlying inode as the original file. Both names are indistinguishable and share the same data; removing one does not affect the other. Hard links cannot cross filesystem boundaries and typically cannot link directories.\n\n```perl\nlink(\"data.csv\", \"data_backup.csv\")\nmy @st1 = stat(\"data.csv\")\nmy @st2 = stat(\"data_backup.csv\")\np $st1[1] == $st2[1]   # 1 — same inode\n```",
        "symlink" => "Create a symbolic (soft) link that points to a target path. Unlike hard links, symlinks can cross filesystems and can point to directories. The link stores the target as a string, so it can dangle if the target is later removed. Use `readlink` to inspect where a symlink points.\n\n```perl\nsymlink(\"/usr/local/bin/stryke\", \"pe_link\")\np readlink(\"pe_link\")       # /usr/local/bin/stryke\np -l \"pe_link\"              # 1 (is a symlink)\nsymlink(\"../lib\", \"lib_link\")  # relative target\n```",
        "readlink" => "Return the target path that a symbolic link points to, without following the link further. Returns `undef` if the path is not a symlink. This is useful for inspecting symlink chains, verifying link targets, or resolving one level of indirection at a time.\n\n```perl\nsymlink(\"real.conf\", \"link.conf\")\nmy $target = readlink(\"link.conf\")\np $target                    # real.conf\nif (defined readlink($path)) {\n    p \"$path is a symlink\"\n}\n```",
        "stat" => "Return a 13-element list of file status information for a path or filehandle: `($dev, $ino, $mode, $nlink, $uid, $gid, $rdev, $size, $atime, $mtime, $ctime, $blksize, $blocks)`. This calls the POSIX `stat` system call. Use it to check file size, modification time, permissions, and other metadata without reading the file.\n\n```perl\nmy @st = stat(\"data.bin\")\np \"size: $st[7] bytes\"\np \"modified: \" . datetime_from_epoch($st[9])\nmy ($mode) = (stat($0))[2]\np sprintf(\"perms: %04o\", $mode & 07777)\n```",
        "chmod" => "Change the permission bits of one or more files. The mode is specified as an octal number. Returns the number of files successfully changed. Does not follow symlinks on some platforms. Use `stat` to read the current mode before modifying.\n\n```perl\nchmod(0755, \"script.pl\")\nchmod(0644, \"config.yaml\", \"data.json\")\nmy $n = chmod(0600, glob(\"*.key\"))\np \"secured $n key files\"\n```",
        "chown" => "Change the owner and group of one or more files, specified as numeric UID and GID. Pass `-1` for either to leave it unchanged. Returns the number of files successfully changed. Typically requires root privileges.\n\n```perl\nchown(1000, 1000, \"app.log\")\nchown(-1, 100, \"shared.txt\")     # change group only\nmy $uid = (getpwnam(\"deploy\"))[2]\nchown($uid, -1, \"release.tar\")\n```",
        "chdir" => "Change the current working directory of the process. Dies if the directory does not exist or is not accessible. This affects all subsequent relative path operations. Pair with `getcwd`/`pwd` to save and restore the original directory.\n\n```perl\nmy $orig = pwd()\nchdir(\"/tmp\")\nspurt(\"scratch.txt\", \"hello\")\nchdir($orig)                  # return to original\n```",
        "glob" => "Expand a shell-style glob pattern against the filesystem and return a list of matching paths. Supports `*`, `?`, `[abc]`, `{a,b}` patterns, and `**` for recursive matching. This actually reads the filesystem, unlike `glob_match` which is a pure string test.\n\n```perl\nmy @scripts = glob(\"*.pl\")\n@scripts |> e p\nmy @all_rs = glob(\"src/**/*.rs\")\np scalar @all_rs              # count of Rust files\nmy @cfg = glob(\"/etc/{nginx,apache2}/*.conf\")\n```",
        "opendir" => "Open a directory handle for reading its entries. Returns a directory handle that can be passed to `readdir`, `seekdir`, `telldir`, `rewinddir`, and `closedir`. Dies if the directory does not exist or cannot be opened. For most use cases `glob` or `readdir` with a path is simpler.\n\n```perl\nopendir(my $dh, \"/tmp\") or die \"cannot open: $!\"\nmy @entries = readdir($dh)\nclosedir($dh)\n@entries |> grep { $_ !~ /^\\./ } |> e p  # skip dotfiles\n```",
        "readdir" => "Read entries from a directory handle opened with `opendir`. In list context, returns all remaining entries. In scalar context, returns the next single entry or `undef` when exhausted. Entries include `.` and `..` so you typically filter them out.\n\n```perl\nopendir(my $dh, \".\") or die $!\nwhile (my $entry = readdir($dh)) {\n    next if $entry =~ /^\\./\n    p $entry\n}\nclosedir($dh)\n```",
        "closedir" => "Close a directory handle previously opened with `opendir`, releasing the underlying OS resource. While directory handles are closed automatically when they go out of scope, explicit `closedir` is good practice in long-running programs or loops that open many directories.\n\n```perl\nopendir(my $dh, \"/var/log\") or die $!\nmy @logs = readdir($dh)\nclosedir($dh)\n@logs |> grep { /\\.log$/ } |> e p\n```",
        "seekdir" => "Set the current position in a directory handle to a location previously obtained from `telldir`. This allows you to revisit directory entries without closing and reopening the handle. Rarely needed in practice, but useful for multi-pass directory scanning.\n\n```perl\nopendir(my $dh, \".\") or die $!\nmy $pos = telldir($dh)\nmy @first_pass = readdir($dh)\nseekdir($dh, $pos)              # rewind to saved position\nmy @second_pass = readdir($dh)\nclosedir($dh)\n```",
        "telldir" => "Return the current read position within a directory handle as an opaque integer. The returned value can be passed to `seekdir` to return to that position later. This is the directory-handle equivalent of `tell` for file handles.\n\n```perl\nopendir(my $dh, \"/tmp\") or die $!\nreaddir($dh)                  # skip .\nreaddir($dh)                  # skip ..\nmy $pos = telldir($dh)        # save position after . and ..\nmy @real = readdir($dh)\nseekdir($dh, $pos)            # go back\n```",
        "rewinddir" => "Reset a directory handle back to the beginning so that the next `readdir` returns the first entry again. This is equivalent to closing and reopening the directory but more efficient. Useful when you need to iterate a directory multiple times.\n\n```perl\nopendir(my $dh, \"src\") or die $!\nmy $count = scalar readdir($dh)\nrewinddir($dh)\nmy @entries = readdir($dh)\nclosedir($dh)\np \"$count entries\"\n```",
        "utime" => "Set the access time and modification time of one or more files. Times are specified as epoch seconds. Pass `undef` for either time to set it to the current time. Returns the number of files successfully updated. Useful for cache invalidation, build systems, or preserving timestamps after transformations.\n\n```perl\nutime(time(), time(), \"output.txt\")     # touch to now\nmy $epoch = 1700000000\nutime($epoch, $epoch, @files)           # backdate files\nutime(undef, undef, \"marker.flag\")      # equivalent of touch\n```",
        "umask" => "Get or set the file creation mask, which controls the default permissions for newly created files and directories. When called with an argument, sets the new mask and returns the previous one. When called without arguments, returns the current mask. The mask is subtracted from the requested permissions in `mkdir`, `open`, etc.\n\n```perl\nmy $old = umask(0077)         # restrict: owner-only\nmkdir(\"private\")               # created with 0700\numask($old)                    # restore previous mask\np sprintf(\"%04o\", umask())     # print current mask\n```",
        "uname" => "Return system identification as a five-element list: `($sysname, $nodename, $release, $version, $machine)`. This calls the POSIX `uname` system call and is useful for platform-specific logic, logging system info, or generating diagnostic reports without shelling out.\n\n```perl\nmy ($sys, $node, $rel, $ver, $arch) = uname()\np \"$sys $rel ($arch)\"           # Linux 6.1.0 (x86_64)\nif ($sys eq \"Darwin\") {\n    p \"running on macOS\"\n}\nlog_info(\"host: $node, kernel: $rel\")\n```",

        // ── Networking / sockets ──
        "socket" => "Create a network socket with the specified domain, type, and protocol. The socket handle is stored in the first argument and can then be used with `bind`, `connect`, `send`, `recv`, and other socket operations. Domain constants include `AF_INET` (IPv4) and `AF_INET6` (IPv6); type constants include `SOCK_STREAM` (TCP) and `SOCK_DGRAM` (UDP).\n\n```perl\nsocket(my $sock, AF_INET, SOCK_STREAM, 0)\nmy $addr = sockaddr_in(8080, inet_aton(\"127.0.0.1\"))\nconnect($sock, $addr)\nsend($sock, \"GET / HTTP/1.0\\r\\n\\r\\n\", 0)\n```",
        "bind" => "Bind a socket to a local address so it can accept connections or receive datagrams on that address. The address is a packed `sockaddr_in` or `sockaddr_in6` structure. Binding is required before calling `listen` on a server socket. Dies if the address is already in use unless `SO_REUSEADDR` is set.\n\n```perl\nsocket(my $srv, AF_INET, SOCK_STREAM, 0)\nsetsockopt($srv, SOL_SOCKET, SO_REUSEADDR, 1)\nbind($srv, sockaddr_in(8080, INADDR_ANY)) or die \"bind: $!\"\nlisten($srv, 5)\n```",
        "listen" => "Mark a bound socket as passive, ready to accept incoming connections. The backlog argument specifies the maximum number of pending connections the OS will queue before refusing new ones. This is only meaningful for stream (TCP) sockets. Call `accept` in a loop after `listen` to handle clients.\n\n```perl\nsocket(my $srv, AF_INET, SOCK_STREAM, 0)\nbind($srv, sockaddr_in(9000, INADDR_ANY))\nlisten($srv, 128) or die \"listen: $!\"\nwhile (accept(my $client, $srv)) {\n    send($client, \"hello\\n\", 0)\n}\n```",
        "accept" => "Accept a pending connection on a listening socket and return a new connected socket handle. The new handle is used for communication with that specific client while the original listening socket continues accepting others. Returns the packed remote address on success, false on failure.\n\n```perl\nlisten($srv, 5)\nwhile (my $remote = accept(my $client, $srv)) {\n    my ($port, $ip) = sockaddr_in($remote)\n    p \"connection from \" . inet_ntoa($ip) . \":$port\"\n    send($client, \"welcome\\n\", 0)\n}\n```",
        "connect" => "Initiate a connection from a socket to a remote address. For TCP sockets this performs the three-way handshake; for UDP it sets the default destination so subsequent `send` calls do not need an address. Dies or returns false if the connection is refused or times out.\n\n```perl\nsocket(my $sock, AF_INET, SOCK_STREAM, 0)\nmy $addr = sockaddr_in(80, inet_aton(\"example.com\"))\nconnect($sock, $addr) or die \"connect: $!\"\nsend($sock, \"GET / HTTP/1.0\\r\\n\\r\\n\", 0)\nrecv($sock, my $buf, 4096, 0)\np $buf\n```",
        "send" => "Send data through a connected socket. The flags argument controls behavior — use `0` for normal sends. For UDP sockets you can supply a destination address as a fourth argument to send to a specific peer without calling `connect` first. Returns the number of bytes sent, or `undef` on error.\n\n```perl\nsend($sock, \"hello world\\n\", 0)\nmy $n = send($sock, $payload, 0)\np \"sent $n bytes\"\n# UDP to specific peer\nsend($udp, $msg, 0, sockaddr_in(5000, inet_aton(\"10.0.0.1\")))\n```",
        "recv" => "Receive data from a socket into a buffer. The length argument specifies the maximum number of bytes to read. For stream sockets a short read is normal — loop until you have all expected data. For datagram sockets each call returns exactly one datagram. Returns the sender address for UDP, or empty string for TCP.\n\n```perl\nrecv($sock, my $buf, 4096, 0) or die \"recv: $!\"\np $buf\nmy $data = \"\"\nwhile (recv($sock, my $chunk, 8192, 0) && length($chunk)) {\n    $data .= $chunk\n}\n```",
        "shutdown" => "Shut down part or all of a socket connection without closing the file descriptor. The `how` argument controls direction: `0` stops reading, `1` stops writing (sends FIN to peer), `2` stops both. This is useful for signaling end-of-data to the remote side while still reading its response.\n\n```perl\nsend($sock, $request, 0)\nshutdown($sock, 1)           # done writing\nrecv($sock, my $resp, 65536, 0)  # still read response\nshutdown($sock, 2)           # fully close\n```",
        "setsockopt" => "Set an option on a socket at the specified protocol level. Common uses include enabling `SO_REUSEADDR` to allow immediate rebinding after a server restart, setting `TCP_NODELAY` to disable Nagle's algorithm, or adjusting buffer sizes. The value is typically a packed integer.\n\n```perl\nsetsockopt($srv, SOL_SOCKET, SO_REUSEADDR, 1)\nsetsockopt($sock, IPPROTO_TCP, TCP_NODELAY, 1)\nsetsockopt($sock, SOL_SOCKET, SO_RCVBUF, pack(\"I\", 262144))\n```",
        "getsockopt" => "Retrieve the current value of a socket option at the specified protocol level. Returns the option value as a packed binary string — use `unpack` to interpret it. Useful for inspecting buffer sizes, checking whether `SO_REUSEADDR` is set, or reading OS-assigned values.\n\n```perl\nmy $val = getsockopt($sock, SOL_SOCKET, SO_RCVBUF)\np unpack(\"I\", $val)                 # e.g. 131072\nmy $reuse = getsockopt($srv, SOL_SOCKET, SO_REUSEADDR)\np unpack(\"I\", $reuse)               # 1 or 0\n```",
        "getpeername" => "Return the packed socket address of the remote end of a connected socket. Use `sockaddr_in` or `sockaddr_in6` to unpack it into a port and IP address. This is how a server discovers which client it is talking to after `accept`, or how a client confirms the peer address after `connect`.\n\n```perl\nmy $packed = getpeername($client)\nmy ($port, $ip) = sockaddr_in($packed)\np \"peer: \" . inet_ntoa($ip) . \":$port\"\n```",
        "getsockname" => "Return the packed socket address of the local end of a socket. This is useful when the socket was bound to `INADDR_ANY` or port `0` (OS-assigned) and you need to discover the actual address and port the OS chose. Unpack the result with `sockaddr_in`.\n\n```perl\nbind($sock, sockaddr_in(0, INADDR_ANY))  # OS picks port\nmy $local = getsockname($sock)\nmy ($port, $ip) = sockaddr_in($local)\np \"listening on port $port\"\n```",
        "gethostbyname" => "Resolve a hostname to its network addresses using the system resolver. Returns `($name, $aliases, $addrtype, $length, @addrs)`. The addresses are packed binary — pass them through `inet_ntoa` to get dotted-quad strings. This is the classic DNS forward-lookup function.\n\n```perl\nmy @info = gethostbyname(\"example.com\")\nmy @addrs = @info[4..$ #info]\n@addrs |> map inet_ntoa |> e p\nmy $ip = inet_ntoa((gethostbyname(\"localhost\"))[4])\np $ip   # 127.0.0.1\n```",
        "gethostbyaddr" => "Perform a reverse DNS lookup — given a packed binary IP address and address family, return the hostname associated with that address. Returns `($name, $aliases, $addrtype, $length, @addrs)` on success, or empty list if no PTR record exists.\n\n```perl\nmy $packed = inet_aton(\"8.8.8.8\")\nmy $name = (gethostbyaddr($packed, AF_INET))[0]\np $name   # dns.google\n```",
        "getpwent" => "Read the next entry from the system password database, iterating through all user accounts. Returns `($name, $passwd, $uid, $gid, $quota, $comment, $gcos, $dir, $shell)` for each user, or empty list when exhausted. Call `setpwent` to rewind and `endpwent` to close.\n\n```perl\nwhile (my @pw = getpwent()) {\n    p \"$pw[0]: uid=$pw[2] home=$pw[7]\"\n}\nendpwent()\n```",
        "getgrent" => "Read the next entry from the system group database, iterating through all groups. Returns `($name, $passwd, $gid, $members)` for each group, or empty list when exhausted. The members field is a space-separated string of usernames. Call `endgrent` when done.\n\n```perl\nwhile (my @gr = getgrent()) {\n    p \"$gr[0]: gid=$gr[2] members=$gr[3]\"\n}\nendgrent()\n```",
        "getprotobyname" => "Look up a network protocol by its name and return protocol information. Returns `($name, $aliases, $proto_number)`. The protocol number is what you pass to `socket` as the protocol argument. Common names include `tcp`, `udp`, and `icmp`.\n\n```perl\nmy ($name, $aliases, $proto) = getprotobyname(\"tcp\")\np \"$name = protocol $proto\"   # tcp = protocol 6\nsocket(my $raw, AF_INET, SOCK_RAW, (getprotobyname(\"icmp\"))[2])\n```",
        "getservbyname" => "Look up a network service by its name and protocol, returning the port number and related information. Returns `($name, $aliases, $port, $proto)`. The port is in host byte order. This resolves well-known service names like `http`, `ssh`, or `smtp` to their port numbers portably.\n\n```perl\nmy ($name, $aliases, $port) = getservbyname(\"http\", \"tcp\")\np \"$name => port $port\"         # http => port 80\nmy $ssh_port = (getservbyname(\"ssh\", \"tcp\"))[2]\np $ssh_port                     # 22\n```",

        // ── Process ──
        "fork" => "Fork the current process, creating a child that is an exact copy of the parent. Returns the child's PID to the parent process and `0` to the child, allowing each side to branch. Returns `undef` on failure. Always pair with `wait`/`waitpid` to reap the child and avoid zombies.\n\n```perl\nmy $pid = fork()\nif ($pid == 0) {\n    p \"child $$\"\n    exit(0)\n}\np \"parent $$, child is $pid\"\nwaitpid($pid, 0)\n```",
        "exec" => "Replace the current process image entirely with a new command. This never returns on success — the new program takes over. If `exec` fails (command not found, permission denied) it returns false and execution continues. Use `system` instead if you want to run a command and keep the current process alive.\n\n```perl\nexec(\"ls\", \"-la\", \"/tmp\") or die \"exec failed: $!\"\n# code here only runs if exec fails\n\n# common fork+exec pattern\nif (fork() == 0) {\n    exec(\"worker\", \"--daemon\")\n}\n```",
        "system" => "Execute a command in a subshell and wait for it to complete, returning the exit status. A return value of `0` means success; non-zero indicates failure. The exit status is encoded as `$? >> 8` for the actual exit code. Use backticks or `capture` if you need the command's output.\n\n```perl\nmy $rc = system(\"make\", \"test\")\np \"exit code: \" . ($rc >> 8)\nsystem(\"cp data.csv /backup/\") == 0 or die \"copy failed\"\nif (system(\"which rg >/dev/null 2>&1\") == 0) {\n    p \"ripgrep is installed\"\n}\n```",
        "wait" => "Wait for any child process to terminate and return its PID. The exit status is stored in `$?`. If there are no child processes, returns `-1`. This is the simplest reaping function — use `waitpid` when you need to wait for a specific child or use non-blocking flags.\n\n```perl\nfor (1..3) {\n    fork() or do { sleep(1)\n    exit(0) }\n}\nwhile ((my $pid = wait()) != -1) {\n    p \"child $pid exited with \" . ($? >> 8)\n}\n```",
        "waitpid" => "Wait for a specific child process identified by PID to change state. The flags argument controls behavior — use `0` for blocking wait, or `WNOHANG` for non-blocking (returns `0` if child is still running). Returns the PID on success, `-1` if the child does not exist. Exit status is in `$?`.\n\n```perl\nmy $pid = fork() // die \"fork: $!\"\nif ($pid == 0) { sleep(2)\n    exit(42) }\nwaitpid($pid, 0)\np \"child exited: \" . ($? >> 8)   # 42\n\n# non-blocking poll\nwhile (waitpid($pid, WNOHANG) == 0) {\n    p \"still running...\"\n    sleep(1)\n}\n```",
        "kill" => "Send a signal to one or more processes by PID. The signal can be specified as a number (`9`) or a name (`\"TERM\"`). Sending signal `0` tests whether the process exists without actually sending anything. Returns the number of processes successfully signaled.\n\n```perl\nkill(\"TERM\", $child_pid)\nkill(9, @worker_pids)                # SIGKILL\nif (kill(0, $pid)) {\n    p \"process $pid is alive\"\n}\nmy $n = kill(\"HUP\", @daemons)\np \"signaled $n processes\"\n```",
        "exit" => "Terminate the program immediately with the given exit status code. An exit code of `0` conventionally means success; any non-zero value indicates an error. `END` blocks and object destructors are still run. Use `POSIX::_exit` to skip cleanup entirely.\n\n```perl\nexit(0)                 # success\nexit(1) if $error       # failure\n\n# conditional exit in a pipeline\nmy $ok = system(\"make test\")\nexit($ok >> 8) if $ok\n```",
        "getlogin" => "Return the login name of the user who owns the current terminal session. This reads from the system's utmp/utmpx database and may return `undef` for processes without a controlling terminal (cron jobs, daemons). For a more reliable alternative, use `getpwuid($<)` which looks up the effective UID.\n\n```perl\nmy $user = getlogin() // (getpwuid($<))[0]\np \"running as $user\"\nlog_info(\"session started by \" . getlogin())\n```",
        "getpwuid" => "Look up user account information by numeric UID. Returns `($name, $passwd, $uid, $gid, $quota, $comment, $gcos, $dir, $shell)` on success, or empty list if the UID does not exist. This is the reliable way to map a UID to a username and home directory.\n\n```perl\nmy ($name, undef, undef, undef, undef, undef, undef, $home) = getpwuid($<)\np \"user: $name, home: $home\"\nmy $root_shell = (getpwuid(0))[8]\np $root_shell   # /bin/bash or /bin/zsh\n```",
        "getpwnam" => "Look up user account information by username string. Returns the same 9-element list as `getpwuid`: `($name, $passwd, $uid, $gid, $quota, $comment, $gcos, $dir, $shell)`. Returns empty list if the user does not exist. Useful for resolving a username to a UID before calling `chown`.\n\n```perl\nmy @info = getpwnam(\"deploy\")\np \"uid=$info[2] home=$info[7]\"\nmy $uid = (getpwnam(\"www-data\"))[2]\nchown($uid, -1, \"public/index.html\")\n```",
        "getgrgid" => "Look up group information by numeric GID. Returns `($name, $passwd, $gid, $members)` where members is a space-separated string of usernames belonging to the group. Returns empty list if the GID does not exist.\n\n```perl\nmy ($name, undef, undef, $members) = getgrgid(0)\np \"group $name: $members\"\nmy $gname = (getgrgid((stat(\"file.txt\"))[5]))[0]\np \"file group: $gname\"\n```",
        "getgrnam" => "Look up group information by group name string. Returns `($name, $passwd, $gid, $members)`. Useful for resolving a group name to a GID before calling `chown`, or for checking group membership.\n\n```perl\nmy ($name, undef, $gid, $members) = getgrnam(\"staff\")\np \"gid=$gid members=$members\"\nchown(-1, $gid, \"shared_dir\")\nif ((getgrnam(\"admin\"))[3] =~ /\\b$user\\b/) {\n    p \"$user is an admin\"\n}\n```",
        "getppid" => "Return the process ID of the parent process. This is useful for detecting whether the process has been orphaned (parent PID becomes 1 on Unix when the original parent exits), or for logging the process hierarchy. Always returns a valid PID.\n\n```perl\np \"my pid: $$, parent: \" . getppid()\nif (getppid() == 1) {\n    log_warn(\"parent process has exited, we are orphaned\")\n}\n```",
        "getpgrp" => "Return the process group ID of the current process (or of the specified PID). Processes in the same group receive signals together — for example, Ctrl-C sends SIGINT to the entire foreground process group. Use `setpgrp` to move a process into a different group.\n\n```perl\np \"process group: \" . getpgrp()\nmy $pg = getpgrp($$)\np $pg == $$ ? \"group leader\" : \"group member\"\n```",
        "setpgrp" => "Set the process group ID of a process. Call `setpgrp(0, 0)` to make the current process a new group leader, which is useful for daemonization or isolating a subprocess from the terminal's signal group. Takes `(PID, PGID)` — use `0` for the current process.\n\n```perl\nsetpgrp(0, 0)   # become process group leader\nif (fork() == 0) {\n    setpgrp(0, 0)  # child gets its own group\n    exec(\"worker\")\n}\n```",
        "getpriority" => "Get the scheduling priority (nice value) of a process, process group, or user. The `which` argument selects the target type: `PRIO_PROCESS`, `PRIO_PGRP`, or `PRIO_USER`. Lower values mean higher priority. The default nice value is `0`; range is typically `-20` to `19`.\n\n```perl\nmy $nice = getpriority(0, $$)    # PRIO_PROCESS, current PID\np \"nice value: $nice\"\nmy $user_prio = getpriority(2, $<)  # PRIO_USER, current user\np \"user priority: $user_prio\"\n```",
        "setpriority" => "Set the scheduling priority (nice value) of a process, process group, or user. Lowering the nice value (higher priority) typically requires root privileges. Raising it (lower priority) is always allowed. Use this to deprioritize background batch work or boost latency-sensitive tasks.\n\n```perl\nsetpriority(0, $$, 10)   # lower priority for batch work\nif (fork() == 0) {\n    setpriority(0, 0, 19)  # lowest priority for child\n    exec(\"batch-job\")\n}\n```",

        // ── Misc builtins ──
        "pack" => "Convert a list of values into a binary string according to a template. Each template character specifies how one value is encoded: `N` for 32-bit big-endian unsigned, `n` for 16-bit, `a` for raw bytes, `Z` for null-terminated string, etc. This is essential for constructing binary protocols, file formats, and `sockaddr` structures.\n\n```perl\nmy $bin = pack(\"NnA4\", 0xDEADBEEF, 8080, \"test\")\np length($bin)                # 10 bytes\nmy $header = pack(\"A8 N N\", \"MAGIC01\\0\", 1, 42)\nspurt(\"data.bin\", $header)\n```",
        "unpack" => "Decode a binary string into a list of values according to a template, performing the inverse of `pack`. The template characters must match how the data was packed. Use this for parsing binary file formats, network protocol headers, or any structured binary data.\n\n```perl\nmy ($magic, $version, $count) = unpack(\"A8 N N\", $header)\np \"v$version, $count records\"\nmy ($port, $addr) = unpack(\"n a4\", $sockaddr)\np inet_ntoa($addr) . \":$port\"\n```",
        "vec" => "Treat a string as a bit vector and get or set individual elements at a specified bit width. The first argument is the string, the second is the element offset, and the third is the bit width (1, 2, 4, 8, 16, or 32). As an lvalue, `vec` modifies the string in place. Useful for compact boolean arrays and bitmap manipulation.\n\n```perl\nmy $bits = \"\"\nvec($bits, 0, 1) = 1   # set bit 0\nvec($bits, 7, 1) = 1   # set bit 7\np vec($bits, 0, 1)     # 1\np vec($bits, 3, 1)     # 0\np unpack(\"B8\", $bits)  # 10000001\n```",
        "tie" => "Bind a variable to an implementing class so that all accesses (read, write, delete, etc.) are intercepted by methods on that class. This is Perl's mechanism for transparent object-backed variables — tied hashes can be backed by a database, tied scalars can validate on assignment, etc. Use `untie` to remove the binding.\n\n```perl\ntie my %db, 'DB_File', 'cache.db'\n$db{key} = \"value\"         # writes to disk\np $db{key}                  # reads from disk\nuntie %db\n```",
        "prototype" => "Return the prototype string of a named function, or `undef` if the function has no prototype. Prototypes control how arguments are parsed at compile time — they influence context and reference-passing behavior. Useful for introspection and metaprogramming.\n\n```perl\np prototype(\"CORE::push\")     # \\@@\np prototype(\"CORE::map\")      # &@\nfn greet($name) { p \"hi $name\" }\np prototype(\\&greet)          # undef (signatures, no proto)\n```",
        "bless" => "Associate a reference with a package name, turning it into an object of that class. The blessed reference can then have methods called on it via `->`. The second argument defaults to the current package. This is the foundation of Perl's object system.\n\n```perl\nfn new($class, %args) {\n    bless { %args }, $class\n}\nmy $obj = new(\"Dog\", name => \"Rex\", breed => \"Lab\")\np ref($obj)          # Dog\np $obj->{name}       # Rex\n```",
        "rand" => "Return a pseudo-random floating-point number in the range `[0, N)`. If N is omitted it defaults to `1`. The result is never exactly equal to N. For integer results, combine with `int`. Seed the generator with `srand` for reproducible sequences.\n\n```perl\np rand()                # e.g. 0.7342...\np int(rand(100))        # random int 0..99\nmy @deck = 1..52\nmy @shuffled = sort { rand() <=> rand() } @deck  # poor shuffle\nmy $coin = rand() < 0.5 ? \"heads\" : \"tails\"\np $coin\n```",
        "srand" => "Seed the pseudo-random number generator used by `rand`. Calling `srand` with a specific value produces a reproducible sequence, which is useful for testing. Without arguments, Perl seeds from a platform-specific entropy source. You rarely need to call this explicitly — Perl auto-seeds on first use of `rand`.\n\n```perl\nsrand(42)                   # reproducible sequence\np int(rand(100))            # always the same value\nsrand(42)\np int(rand(100))            # same value again\nsrand()                     # re-seed from entropy\n```",
        "int" => "Truncate a floating-point number toward zero, discarding the fractional part. This is not rounding — `int(1.9)` is `1` and `int(-1.9)` is `-1`. Use `sprintf(\"%.0f\", $n)` or `POSIX::round` for proper rounding. Commonly paired with `rand` to generate random integers.\n\n```perl\np int(3.7)       # 3\np int(-3.7)      # -3\np int(rand(6)) + 1  # dice roll 1..6\n1..10 |> map { $_ / 3 } |> map int |> e p\n```",
        "abs" => "Return the absolute value of a number, stripping any negative sign. Returns the argument unchanged if it is already non-negative. Works on both integers and floating-point numbers.\n\n```perl\np abs(-42)       # 42\np abs(3.14)      # 3.14\nmy $diff = abs($a - $b)\np \"distance: $diff\"\n1..5 |> map { $_ - 3 } |> map abs |> e p  # 2 1 0 1 2\n```",
        "sqrt" => "Return the square root of a non-negative number. Dies if the argument is negative — use `abs` first or check the sign. For the inverse operation, use `squared`/`sq` or the `**` operator.\n\n```perl\np sqrt(144)         # 12\np sqrt(2)           # 1.41421356...\nmy $hyp = sqrt($a**2 + $b**2)  # Pythagorean theorem\n1..5 |> map sqrt |> e { p sprintf(\"%.3f\", $_) }\n```",
        "squared" | "sq" | "square" => "Return the square of a number (`N * N`). This is a stryke convenience function — clearer than writing `$n ** 2` or `$n * $n` in pipelines. The aliases `sq` and `square` are interchangeable.\n\n```perl\np squared(5)        # 25\np sq(12)            # 144\n1..5 |> map sq |> e p    # 1 4 9 16 25\nmy $hyp = sqrt(sq($a) + sq($b))  # Pythagorean theorem\n```",
        "cubed" | "cb" | "cube" => "Return the cube of a number (`N * N * N`). This is a stryke convenience function for the common `$n ** 3` operation, useful in math-heavy pipelines. The aliases `cb` and `cube` are interchangeable.\n\n```perl\np cubed(3)          # 27\np cb(10)            # 1000\n1..4 |> map cb |> e p  # 1 8 27 64\nmy $vol = cb($side)             # volume of a cube\n```",
        "expt" | "pow" | "pw" => "Raise a base to an arbitrary exponent and return the result. This is the function form of the `**` operator. Accepts integer and floating-point exponents, including negative values for reciprocals and fractional values for roots.\n\n```perl\np expt(2, 10)       # 1024\np expt(27, 1/3)     # 3.0 (cube root)\np expt(10, -2)      # 0.01\n1..8 |> map { expt(2, $_) } |> e p  # 2 4 8 16 32 64 128 256\n```",
        "exp" => "Return Euler's number *e* raised to the given power. `exp(0)` is `1`, `exp(1)` is approximately `2.71828`. This is the inverse of `log`. Useful for exponential growth/decay calculations, probability distributions, and converting between logarithmic and linear scales.\n\n```perl\np exp(1)            # 2.71828182845905\np exp(0)            # 1\nmy $growth = $initial * exp($rate * $time)\n1..5 |> map exp |> e { p sprintf(\"%.4f\", $_) }\n```",
        "log" => "Return the natural (base-*e*) logarithm of a positive number. Dies if the argument is zero or negative. For base-10 logarithms, divide by `log(10)`. For base-2, divide by `log(2)`. This is the inverse of `exp`.\n\n```perl\np log(exp(1))       # 1.0\np log(100) / log(10)  # 2.0 (log base 10)\nmy $bits = log($n) / log(2)  # log base 2\n1..5 |> map log |> e { p sprintf(\"%.3f\", $_) }\n```",
        "sin" => "Return the sine of an angle given in radians. The result ranges from `-1` to `1`. For degrees, convert first: `sin($deg * 3.14159265 / 180)`. Use `atan2` to go in the reverse direction.\n\n```perl\np sin(0)               # 0\np sin(3.14159265 / 2)  # 1.0\nmy @wave = map { sin($_ * 0.1) } 0..62\n@wave |> e { p sprintf(\"%6.3f\", $_) }\n```",
        "cos" => "Return the cosine of an angle given in radians. The result ranges from `-1` to `1`. `cos(0)` is `1`. Like `sin`, convert degrees to radians before calling.\n\n```perl\np cos(0)                 # 1\np cos(3.14159265)        # -1.0\nmy $x = $radius * cos($theta)\nmy $y = $radius * sin($theta)\np \"($x, $y)\"\n```",
        "atan2" => "Return the arctangent of `Y/X` in radians, using the signs of both arguments to determine the correct quadrant. The result ranges from `-pi` to `pi`. This is the standard way to compute angles from Cartesian coordinates and is more robust than `atan(Y/X)` because it handles `X=0` correctly.\n\n```perl\nmy $pi = atan2(0, -1)       # 3.14159265...\np atan2(1, 1)               # 0.785... (pi/4)\nmy $angle = atan2($dy, $dx)\np sprintf(\"%.1f degrees\", $angle * 180 / $pi)\n```",
        "formline" => "Format a line of output according to a picture template, appending the result to the `$^A` (format accumulator) variable. This is the low-level engine behind Perl's `format`/`write` report-generation system. Template characters like `@<<<` (left-justify), `@>>>` (right-justify), and `@###.##` (numeric) control field placement.\n\n```perl\n$^A = \"\"\nformline(\"@<<<< @>>>>>\\n\", \"Name\", \"Score\")\nformline(\"@<<<< @>>>>>\\n\", \"Alice\", 98)\nformline(\"@<<<< @>>>>>\\n\", \"Bob\", 85)\np $^A\n```",
        "not" => "Low-precedence logical negation — returns true if the expression is false, and false if it is true. Functionally identical to `!` but binds looser than almost everything, so `not $a == $b` is `not($a == $b)` rather than `(!$a) == $b`. Useful for readable boolean conditions.\n\n```perl\nif (not defined $val) {\n    p \"val is undef\"\n}\nmy @missing = grep { not -e $_ } @files\n@missing |> e p\np not 0    # 1\np not 1    # (empty string)\n```",
        "syscall" => "Invoke a raw system call by its numeric identifier, passing arguments directly to the kernel. This is an escape hatch for system calls that have no Perl wrapper. The call number is platform-specific and the arguments must be correctly typed (integers or string buffers). Use with caution — incorrect arguments can crash the process.\n\n```perl\n# SYS_getpid on Linux x86_64 is 39\nmy $pid = syscall(39)\np $pid                     # same as $$\n# SYS_sync on Linux is 162\nsyscall(162)               # flush filesystem caches\n```",

        // ── stryke extensions (syntax / macros) ──
        "thread" | "t" | "~>" | "->>" => "Clojure-inspired threading macro — chain stages without repeating `|>`.\n\n```perl\n~> @data grep { $_ > 5 } map { $_ * 2 } sort { $_0 <=> $_1 } |> join \",\" |> p\n~> \" hello \" tm uc rv lc ufc sc cc kc tj p  # short aliases\nsub add2 { $_0 + $_1 }\n~> 10 add2($_, 5) p                          # add2(10, 5) = 15\n~> 10 add2(5, $_) p                          # add2(5, 10) = 15  (any position)\n~> 10 add2($_, 5) add2($_, 100) p            # 115 (chained)\n```\n\nFour equivalent spellings: `~>`, `thread`, `t`, `->>`. Stages: bare function (`uc`, `tm`, …), function with block (`map { … }`, `grep { … }`), `name(args)` call where `$_` is the threaded-value placeholder (must appear at least once in args), or `>{}` anonymous block.\n`|>` terminates the `~>` macro.",
        "fn" => "Alias for `sub` — define a function with optional typed parameters and default values.\n\n```perl\nfn double($x) { $x * 2 }\nmy $f = fn { $_ * 2 }\nmy $add = fn ($a: Int, $b: Int) { $a + $b }\n\n# Default parameter values (stryke extension):\nfn greet($name = \"world\") { p \"hello $name\" }\ngreet()           # hello world\ngreet(\"Alice\")    # hello Alice\n\nfn range_check($x, $min = 0, $max = 100) { $min <= $x <= $max }\nfn with_list(@items = (1, 2, 3)) { join \"-\", @items }\nfn with_hash(%opts = (debug => 0)) { $opts{debug} }\n```\n\nDefault values are evaluated at call time if the argument is not provided.",
        "mysync" => "Declare shared variables for parallel blocks (`Arc<Mutex>`).\n\n```perl\nmysync $counter = 0\nfan 10000 { $counter++ }   # always exactly 10000\nmysync @results\nmysync %histogram$1\n\nCompound ops (`++`, `+=`, `.=`, `|=`, `&=`) are fully atomic.",
        "frozen" | "const" => "Declare an immutable lexical variable. `const my` and `frozen my` are interchangeable spellings; `const` reads more naturally for engineers coming from other languages.\n\n```perl\nconst my $pi = 3.14159\n# $pi = 3  # ERROR: cannot assign to frozen variable\n\nfrozen my @primes = (2, 3, 5, 7, 11)\n```",
        "match" => "Algebraic pattern matching (stryke extension).\n\n```perl\nmatch ($val) {\n    /^\\d+$/ => p \"number: $val\",\n    [1, 2, _] => p \"array starting with 1,2\",\n    { name => $n } => p \"name is $n\",\n    _ => p \"default\",\n}\n```\n\nPatterns: regex, array, hash, literal, wildcard `_`. Optional `if` guard per arm.",
        "|>" => "Pipe-forward operator — threads LHS as first argument of RHS call.\n\n```perl\n\"hello\" |> uc |> rev |> p              # OLLEH\n1..10 |> grep $_ > 5 |> map $_ * 2 |> e p\n$url |> fetch_json |> json_jq '.name' |> p\n\"hello world\" |> s/world/perl/ |> p     # hello perl\n```\n\nZero runtime cost (parse-time desugaring). Binds looser than `||`, tighter than `?:`.",
        "chained_comparison" | "chained comparisons" => "Raku-style chained comparisons — `1 < $x < 10` works without explicit `&&`.\n\nStryke desugars chained numeric and string comparisons at parse time:\n- `a < b < c` becomes `(a < b) && (b < c)`\n- `a <= b < c <= d` becomes `(a <= b) && (b < c) && (c <= d)`\n- Works with all comparison operators: `<`, `<=`, `>`, `>=`, `lt`, `le`, `gt`, `ge`\n\n```perl\nmy $x = 5\nif (1 < $x < 10) { p \"in range\" }      # true\nif (0 < $x <= 5) { p \"0 < x <= 5\" }    # true\nif (10 > $x > 0) { p \"descending\" }    # true\nif (\"a\" lt $s lt \"z\") { p \"lowercase\" } # string comparisons too\n\n# Chains of 3+ work:\nif (1 < 2 < 3 < 4) { p \"all true\" }\n```\n\nShort-circuit evaluation applies: if any comparison is false, the rest are not evaluated.",
        "pipe" | "CORE::pipe" => "Create a unidirectional pipe, returning a pair of connected filehandles: one for reading and one for writing. Data written to the write end can be read from the read end, making pipes the fundamental building block for inter-process communication. Commonly used with `fork` so the parent and child can exchange data.\n\n```perl\npipe(my $rd, my $wr) or die \"pipe: $!\"\nif (fork() == 0) {\n    close($rd)\n    print $wr \"hello from child\\n\"\n    exit(0)\n}\nclose($wr)\nmy $msg = <$rd>\np $msg   # hello from child\n```",
        "gen" => "Create a generator — lazy `yield` values on demand.\n\n```perl\nmy $g = gen { yield $_ for 1..5 }\nmy ($val, $more) = @{$g->next}\n```",
        "yield" => "Yield a value from inside a `gen { }` generator block, suspending the generator until the consumer calls `->next` again. Each `yield` produces one element in the lazy sequence. When the block finishes without yielding, the generator signals exhaustion. This is the stryke equivalent of Python's `yield` or Rust's `Iterator::next`.\n\n```perl\nmy $fib = gen {\n    my ($a, $b) = (0, 1)\n    while (1) {\n        yield $a\n        ($a, $b) = ($b, $a + $b)\n    }\n}\nfor (1..10) {\n    my ($val) = @{$fib->next}\n    p $val\n}\n```",
        "trace" => "Trace `mysync` mutations to stderr (tagged with worker index under `fan`).\n\n```perl\ntrace { fan 10 { $counter++ } }\n```",
        "timer" => "Measure wall-clock milliseconds for a block.\n\n```perl\nmy $ms = timer { heavy_work() }\n```",
        "bench" => "Benchmark a block N times; returns `\"min/mean/p99\"`.\n\n```perl\nmy $report = bench { work() } 1000\n```",
        "eval_timeout" => "Run a block with a wall-clock timeout (seconds).\n\n```perl\neval_timeout 5 { slow_operation() }\n```",
        "retry" => "Retry a block on failure.\n\n```perl\nretry { http_call() } times => 3, backoff => 'exponential'\n```",
        "rate_limit" => "Limit invocations per time window.\n\n```perl\nrate_limit(10, \"1s\") { hit_api() }\n```",
        "every" => "Run a block at a fixed interval.\n\n```perl\nevery \"500ms\" { tick() }\n```",
        "fore" | "e" => "Side-effect-only list iterator (like `map` but void, returns item count).\n\n```perl\nqw(a b c) |> e p           # prints a, b, c; returns 3\n1..5 |> map $_ * 2 |> e p  # prints 2,4,6,8,10\n```",
        "ep" => "`ep` — shorthand for `e { p }` (foreach + print). Iterates the list and prints each element.\n\n```perl\nqw(a b c) |> ep            # prints a, b, c (one per line)\nfilef |> sorted |> ep      # print sorted file list\n1..5 |> map $_ * 2 |> ep   # prints 2,4,6,8,10\n```",
        "p" => "`p` — alias for `say` (print with newline).\n\n```perl\np \"hello\"       # hello\\n\np 42            # 42\\n\n1..5 |> e p     # prints each on its own line\n```",
        "watch" => "Watch a single file for changes (non-parallel).\n\n```perl\nwatch \"/tmp/x\", fn { process }\n```",
        "glob_par" => "Perform a parallel recursive file-system glob, using multiple threads to walk directory trees concurrently. This is significantly faster than `glob` for deep directory hierarchies with thousands of files. Accepts the same glob syntax (`*`, `**`, `{a,b}`) but returns results as they are discovered across threads. Ideal for large codebases or log directories.\n\n```perl\nmy @logs = glob_par(\"**/*.log\")\np scalar @logs               # count of log files\n\"**/*.rs\" |> glob_par |> e p  # print all Rust files\nmy @imgs = glob_par(\"assets/**/*.{png,jpg,webp}\")\n@imgs |> e { p bn($_) }\n```",
        "par_find_files" => "Recursively search a directory tree in parallel for files matching a glob pattern. Unlike `glob_par` which takes a single pattern string, `par_find_files` separates the root directory from the pattern, making it convenient when the search root is a variable. Returns a list of absolute paths to matching files.\n\n```perl\nmy @src = par_find_files(\"src\", \"*.rs\")\np scalar @src                    # count of Rust files under src/\nmy @tests = par_find_files(\".\", \"*_test.pl\")\n@tests |> e p\nmy @configs = par_find_files(\"/etc\", \"*.conf\")\n```",
        "par_line_count" => "Count lines across multiple files in parallel, returning the total line count. Each file is read and counted by a separate thread, making this dramatically faster than sequential `wc -l` for large file sets. Useful for codebase metrics, log analysis, or validating data pipeline output.\n\n```perl\nmy @files = glob(\"src/**/*.rs\")\nmy $total = par_line_count(@files)\np \"$total lines of Rust\"\nmy $logs = par_line_count(glob(\"/var/log/*.log\"))\np \"$logs log lines\"\n```",
        "capture" => "Run a command and capture structured output. Returns an object with `->stdout`, `->stderr`, `->exitcode`, and `->failed` methods. When piped to `lines` or `words`, stdout is auto-extracted so you can chain directly.\n\n```perl\nmy $r = capture(\"ls -la\")\np $r->stdout, $r->stderr, $r->exitcode\ncapture(\"ps aux\") |> lines |> map { columns } |> tbl |> p\n```",

        "pager" | "pg" | "less" => "`LIST |> pager` / `pager LIST` — pipe each element (one per line) into the user's `$PAGER` (default `less -R`; falls back to `more`, then plain stdout). Bypasses the pager when stdout isn't a TTY so pipelines like `stryke '... |> pager' | grep` still compose.\n\nBlocks until the user quits the pager; returns `undef`.\n\nAliases: `pager`, `pg`, `less`.\n\n```perl\n# browse every callable spelling interactively\nkeys %all |> sort |> pager\n\n# filter the reference for parallel ops\nkeys %b |> grep { $b{$_} eq \"parallel\" } |> pager\n\n# whole file, one screen at a time\nslurp(\"README.md\") |> pager\n```",
        "input" => "Slurp all of stdin (or a filehandle) as one string.\n\n```perl\nmy $all = input          # slurp stdin\nmy $fh_data = input($fh) # slurp filehandle\n```",
        "slurp" | "sl" => "Read an entire file into memory as a single UTF-8 string. The short alias `sl` is convenient in pipelines. Dies if the file does not exist or cannot be read. This is the complement of `spurt`/`wf` — together they form a simple read/write pair for whole-file operations. For binary data, use `read_bytes`/`slurp_raw` instead.\n\n```perl\nmy $text = slurp(\"config.yaml\")\np $text\nmy $json = sl(\"data.json\")\nmy $data = decode_json($json)\n\"README.md\" |> sl |> length |> p  # character count\n```",

        // ── Language reflection (populated at interpreter init from `build.rs` tables) ──
        "stryke::builtins" => "`%stryke::builtins` (short: `%b`) — every **primary** callable name → its category. Primaries-only, so `scalar keys %b` is a clean unique-operation count. For the \"everything you can type\" view (primaries + aliases), use `%stryke::all` / `%all`.\n\n```perl\np $b{pmap}               # \"parallel\"\np $b{to_json}            # \"serialization\"\np scalar keys %b         # unique-op count\n```",

        "stryke::all" => "`%stryke::all` (short: `%all`) — every callable *spelling* (primaries **and** aliases) → category. Aliases inherit their primary's category.\n\nUse `%all` when you want \"how many names can I type?\" or want to look up an alias's category without hopping through `%aliases`. Use `%builtins` when you want unique operations.\n\n```perl\np scalar keys %all   # total callable-spellings count\np $all{tj}           # \"serialization\"  (alias resolves via inheritance)\np $all{to_json}      # \"serialization\"\nkeys %all |> pager   # browse every spelling\n```",

        "stryke::perl_compats" => "`%stryke::perl_compats` (short: `%pc`) — Perl 5 core names only, name → category.\n\nSubset of `%builtins` restricted to names from `is_perl5_core`. Direct O(1) access for the \"show me just Perl core\" query.\n\n```perl\np $pc{map}                    # \"array / list\"\np scalar keys %pc             # core-only count\nkeys %pc |> sort |> p         # enumerate every Perl core name\n```",

        "stryke::extensions" => "`%stryke::extensions` (short: `%e`) — stryke-only names, name → category.\n\nDisjoint from `%perl_compats`. Everything `--compat` mode rejects at parse time, plus dispatch primaries like `basename`/`ddump` that are extensions at runtime even without a parser entry.\n\n```perl\np $e{pmap}                                # \"parallel\"\nkeys %e |> grep /^p/ |> sort |> p         # every p* parallel op\n```",

        "stryke::aliases" => "`%stryke::aliases` (short: `%a`) — alias spelling → canonical primary.\n\nKeys are the 2nd-and-later names in each `try_builtin` match arm. For O(1) *reverse* lookup (primary → all its aliases), use `%stryke::primaries` / `%p`.\n\n```perl\np $a{tj}                                  # \"to_json\"\np $a{bn}                                  # \"basename\"\np scalar keys %a                          # total alias count\n```",

        "stryke::descriptions" => "`%stryke::descriptions` (short: `%d`) — name → one-line summary.\n\nFirst sentence of each LSP hover doc (`doc_for_label_text`), harvested at build time. Sparse — only names that have a hover doc appear, so `exists $d{$name}` doubles as \"is this documented?\".\n\n```perl\np $d{pmap}                                # one-line summary\np $d{to_json}                             # \"Serialize a PerlValue to a JSON string.\"\np scalar keys %d                          # count of documented ops\nkeys %d |> grep { $d{$_} =~ /parallel/i } |> sort |> p\n```",

        "stryke::categories" => "`%stryke::categories` (short: `%c`) — category string → arrayref of names in that category.\n\nInverted index on `%builtins`. Gives O(1) reverse-lookup for \"list every op of kind X\" queries that would otherwise be O(n) `grep`s. Name lists are alphabetized.\n\n```perl\n$c{parallel} |> e p                  # every parallel op\np scalar @{ $c{parallel} }           # how many?\np join \", \", @{ $c{\"array / list\"} } # joined roster\nkeys %c |> sort |> p                 # all category names\n```",

        "stryke::primaries" => "`%stryke::primaries` (short: `%p`) — primary dispatcher name → arrayref of its aliases.\n\nInverted `%aliases`. Primaries with no aliases still have an entry (empty arrayref), so `exists $p{foo}` reliably answers \"is foo a dispatch primary?\" O(1).\n\n```perl\n$p{to_json} |> e p              # [\"tj\"]\np scalar @{ $p{basename} }      # how many aliases does basename have?\n# find every primary that has at least one alias:\nkeys %p |> grep { scalar @{$p{$_}} } |> sort |> p\n```",

        // ── Higher-order function wrappers ──
        "compose" | "comp" => "`compose` (alias `comp`) creates a right-to-left function composition. Given `compose(\\&f, \\&g)`, calling the result with `x` computes `f(g(x))`. Chain any number of functions — they apply from right to left (last argument first). This is the standard mathematical function composition found in Haskell, Clojure, and Ramda. The returned code ref can be stored, passed around, or used in pipelines.\n\n```perl\nmy $double = fn { $_[0] * 2 }\nmy $inc    = fn { $_[0] + 1 }\nmy $f = compose($inc, $double)\np $f->(5)   # 11  (double 5 → 10, inc 10 → 11)\n\nmy $pipeline = compose(\n    fn { join \",\", @{$_[0]} },\n    fn { [sort @{$_[0]}] },\n    fn { [grep { $_ > 2 } @{$_[0]}] },\n)\np $pipeline->([3,1,4,1,5])  # 3,4,5\n```",
        "partial" => "`partial` returns a partially applied function — the bound arguments are prepended to any arguments supplied at call time. `partial(\\&f, @bound)->(x)` is equivalent to `f(@bound, x)`. This is the standard partial application from functional programming, useful for creating specialized versions of general functions without closures.\n\n```perl\nmy $add = fn { $_[0] + $_[1] }\nmy $add5 = partial($add, 5)\np $add5->(3)   # 8\n\nmy $log = fn { p \"[$_[0]] $_[1]\" }\nmy $warn_log = partial($log, \"WARN\")\n$warn_log->(\"disk full\")   # [WARN] disk full\n```",
        "curry" => "`curry` auto-curries a function with a given arity. The curried function accumulates arguments across calls and invokes the original only when enough have been collected. `curry(\\&f, N)->(a)->(b)` calls `f(a, b)` when N=2. If all arguments are supplied at once, it calls immediately.\n\n```perl\nmy $add = curry(fn { $_[0] + $_[1] }, 2)\nmy $add5 = $add->(5)\np $add5->(3)       # 8\np $add->(10, 20)   # 30  (enough args — calls immediately)\n```",
        "memoize" | "memo" => "`memoize` (alias `memo`) wraps a function so that repeated calls with the same arguments return a cached result instead of re-executing the function. Arguments are stringified and joined as the cache key. This is essential for expensive pure functions like recursive algorithms, API lookups with stable results, or any computation where the same inputs always produce the same output.\n\n```perl\nmy $fib = memoize(fn {\n    my $n = $_[0]\n    $n < 2 ? $n : $fib->($n-1) + $fib->($n-2)\n})\np $fib->(30)   # instant (without memoize: ~1B calls)\n\nmy $fetch_user = memo(fn { fetch_json(\"https://api/users/$_[0]\") })\n$fetch_user->(42)   # hits API\n$fetch_user->(42)   # returns cached\n```",
        "once" => "`once` wraps a function so it is called at most once. The first invocation executes the function and caches the result; all subsequent calls return the cached value without re-executing. This is ideal for lazy initialization, one-time setup, or singleton patterns.\n\n```perl\nmy $init = once(fn { p \"initializing...\"\n    42 })\np $init->()   # prints \"initializing...\" → 42\np $init->()   # 42 (no print — cached)\np $init->()   # 42 (still cached)\n```",
        "constantly" => "`constantly` (alias `const`) returns a function that ignores all arguments and always returns the given value. Useful as a default callback, a stub in higher-order function pipelines, or anywhere a function is required but a fixed value suffices.\n\n```perl\nmy $zero = constantly(0)\np $zero->(\"anything\")   # 0\nmy @defaults = map { constantly(0)->() } 1..5   # [0,0,0,0,0]\n```",
        "complement" | "compl" => "`complement` (alias `compl`) wraps a predicate function and returns a new function that negates its boolean result. `complement(\\&even?)->(3)` returns true. This is the functional equivalent of `!f(x)` without creating a closure.\n\n```perl\nmy $even = fn { $_[0] % 2 == 0 }\nmy $odd = complement($even)\np $odd->(3)   # 1\np $odd->(4)   # 0\n1..10 |> grep { complement($even)->($_) } |> e p  # 1 3 5 7 9\n```",
        "juxt" => "`juxt` (juxtapose) takes multiple functions and returns a new function that calls each one with the same arguments and collects the results into an array. This is useful for computing multiple derived values from the same input in a single pass.\n\n```perl\nmy $stats = juxt(fn { min @_ }, fn { max @_ }, fn { avg @_ })\nmy @r = $stats->(3, 1, 4, 1, 5)\np \"@r\"   # 1 5 2.8\n```",
        "fnil" => "`fnil` wraps a function so that any `undef` arguments are replaced with the given defaults before the function is called. This eliminates repetitive `// $default` patterns inside function bodies.\n\n```perl\nmy $greet = fnil(fn { \"Hello, $_[0]!\" }, \"World\")\np $greet->(undef)    # Hello, World!\np $greet->(\"Alice\")  # Hello, Alice!\n```",

        // ── Deep structure utilities ──
        "deep_clone" | "dclone" => "`deep_clone` (alias `dclone`) performs a recursive deep copy of a nested data structure. Array refs, hash refs, and scalar refs are cloned recursively so that the result shares no references with the original. Modifications to the clone never affect the source. This is the stryke equivalent of JavaScript's `structuredClone` or Perl's `Storable::dclone`.\n\n```perl\nmy $orig = {users => [{name => \"Alice\"}], meta => {v => 1}}\nmy $copy = deep_clone($orig)\n$copy->{users}[0]{name} = \"Bob\"\np $orig->{users}[0]{name}   # Alice (unchanged)\n```",
        "deep_merge" | "dmerge" => "`deep_merge` (alias `dmerge`) recursively merges two hash references. When both sides have a hash ref for the same key, they are merged recursively; otherwise the right-hand value wins. This is the standard deep merge from Lodash, Ruby's `deep_merge`, and config-file overlay patterns. Returns a new hash ref — neither input is modified.\n\n```perl\nmy $defaults = {db => {host => \"localhost\", port => 5432}, debug => 0}\nmy $overrides = {db => {port => 3306}, debug => 1}\nmy $cfg = deep_merge($defaults, $overrides)\np $cfg->{db}{host}   # localhost (from defaults)\np $cfg->{db}{port}   # 3306 (overridden)\np $cfg->{debug}      # 1 (overridden)\n```",
        "deep_equal" | "deq" => "`deep_equal` (alias `deq`) performs structural equality comparison of two values, recursively descending into array refs, hash refs, and scalar refs. Returns 1 if the structures are identical, 0 otherwise. This is the stryke equivalent of Node's `assert.deepStrictEqual`, Lodash `isEqual`, or Python's `==` on nested dicts/lists.\n\n```perl\np deep_equal([1, {a => 2}], [1, {a => 2}])   # 1\np deep_equal([1, {a => 2}], [1, {a => 3}])   # 0\np deq({x => [1,2]}, {x => [1,2]})            # 1\n```",
        "tally" => "`tally` counts how many times each distinct element appears in a list and returns a hash ref mapping element → count. This is the same as Ruby's `Enumerable#tally` or Python's `Counter`. Similar to `frequencies` but follows the Ruby naming convention.\n\n```perl\nmy $t = tally(\"a\", \"b\", \"a\", \"c\", \"a\", \"b\")\np $t->{a}   # 3\np $t->{b}   # 2\nqw(red blue red green blue red) |> tally |> dd\n```",

        // ── System stats ──
        "mem_total" => "`mem_total` — total physical RAM in bytes. Uses `/proc/meminfo` on Linux, `sysctlbyname(\"hw.memsize\")` on macOS. Returns `undef` on unsupported platforms.\n\n```perl\np mem_total                              # 68719476736\np format_bytes(mem_total)                # 64.0 GiB\n```",
        "mem_free" => "`mem_free` — free/available physical RAM in bytes. On Linux reads `MemAvailable` from `/proc/meminfo` (falls back to `MemFree`). On macOS uses `vm.page_free_count` via sysctl. Returns `undef` on unsupported platforms.\n\n```perl\np mem_free                               # 3821666304\np format_bytes(mem_free)                 # 3.6 GiB\nmy $pct = int(mem_free / mem_total * 100)\np \"$pct% free\"\n```",
        "mem_used" => "`mem_used` — used physical RAM in bytes (total - free). Returns `undef` if either `mem_total` or `mem_free` is unavailable.\n\n```perl\np format_bytes(mem_used)                 # 60.4 GiB\ngauge(mem_used / mem_total) |> p         # memory usage gauge\n```",
        "swap_total" => "`swap_total` — total swap space in bytes. Linux: `/proc/meminfo SwapTotal`. macOS: `vm.swapusage` sysctl. Returns 0 if swap is disabled, `undef` on unsupported platforms.\n\n```perl\np swap_total                             # 8589934592\np format_bytes(swap_total)               # 8.0 GiB\n```",
        "swap_free" => "`swap_free` — free swap space in bytes.\n\n```perl\np format_bytes(swap_free)\nmy $pct = swap_total > 0 ? int(swap_free / swap_total * 100) : 100\np \"$pct% swap free\"\n```",
        "swap_used" => "`swap_used` — used swap space in bytes (total - free).\n\n```perl\np format_bytes(swap_used)\np \"swap pressure\" if swap_used > swap_total * 0.8\n```",
        "disk_total" => "`disk_total PATH` — total disk space in bytes for the filesystem containing PATH (default `/`). Uses `statvfs` on unix. Returns `undef` on unsupported platforms.\n\n```perl\np format_bytes(disk_total)               # 1.8 TiB\np format_bytes(disk_total(\"/home\"))       # specific mount\n```",
        "disk_free" => "`disk_free PATH` — free disk space in bytes (superuser view). Uses `f_bfree` from `statvfs`.\n\n```perl\np format_bytes(disk_free)                # 76.3 GiB\np format_bytes(disk_free(\"/tmp\"))\n```",
        "disk_avail" => "`disk_avail PATH` — available disk space in bytes for non-root users. Uses `f_bavail` from `statvfs`, which excludes blocks reserved for the superuser.\n\n```perl\np format_bytes(disk_avail)               # 76.3 GiB\ngauge(1 - disk_avail / disk_total) |> p  # usage gauge\n```",
        "disk_used" => "`disk_used PATH` — used disk space in bytes (total - free).\n\n```perl\np format_bytes(disk_used)                # 1.7 TiB\nmy $pct = int(disk_used / disk_total * 100)\np \"$pct% disk used\"\n```",
        "load_avg" => "`load_avg` — system load averages as a 3-element arrayref `[1min, 5min, 15min]`. Uses `getloadavg()` on unix. Returns `undef` on unsupported platforms.\n\n```perl\nmy $la = load_avg\np \"1m=$la->[0] 5m=$la->[1] 15m=$la->[2]\"\nload_avg |> spark |> p                   # sparkline of load\n```",
        "sys_uptime" => "`sys_uptime` — system uptime in seconds (float). On Linux reads `/proc/uptime`. On macOS uses `kern.boottime` sysctl. This is the wall-clock uptime of the machine, not the stryke process (see `uptime_secs` for process uptime).\n\n```perl\np sys_uptime                             # 147951.5\nmy $days = int(sys_uptime / 86400)\np \"up $days days\"\n```",
        "page_size" => "`page_size` — memory page size in bytes. Uses `sysconf(_SC_PAGESIZE)` on unix.\n\n```perl\np page_size                              # 16384 (Apple Silicon)\np page_size                              # 4096 (x86_64)\n```",
        "os_version" => "`os_version` — OS kernel version/release string from `uname`. Returns `undef` on non-unix.\n\n```perl\np os_version                             # 25.4.0\np \"kernel: \" . os_version\n```",
        "os_family" => "`os_family` — OS family string: `\"unix\"` or `\"windows\"`. From `std::env::consts::FAMILY`.\n\n```perl\np os_family                              # unix\ndie \"unix only\" unless os_family eq \"unix\"\n```",
        "endianness" => "`endianness` — byte order of the platform: `\"little\"` or `\"big\"`. Compile-time constant.\n\n```perl\np endianness                             # little\n```",
        "pointer_width" => "`pointer_width` — pointer width in bits: `32` or `64`. Compile-time constant.\n\n```perl\np pointer_width                          # 64\n```",
        "proc_mem" | "rss" => "`proc_mem` (alias `rss`) — current process resident set size (RSS) in bytes. On Linux reads `VmRSS` from `/proc/self/status`. On macOS uses `getrusage(RUSAGE_SELF)`. Returns `undef` on unsupported platforms.\n\n```perl\np format_bytes(rss)                      # 18.5 MiB\nmy $before = rss\ndo_work()\np format_bytes(rss - $before) . \" allocated\"\n```",

        // ── Statistics (extended) ─────────────────────────────────────────────
        "z_score" => "`z_score($value, $mean, $stddev)` — computes the z-score (standard score) of a value given the population mean and standard deviation. Returns how many standard deviations the value is from the mean. Useful for standardizing data and detecting outliers.\n\n```perl\nmy $z = z_score(85, 75, 10)\np $z   # 1 (one stddev above mean)\np z_score(50, 60, 5)   # -2\n```",
        "z_scores" => "`z_scores(@data)` — computes z-scores for all values in a list, returning an arrayref of standardized values. Each z-score indicates how many standard deviations that value is from the mean of the dataset. Useful for comparing values across different scales.\n\n```perl\nmy @grades = (70, 80, 90)\nmy $zs = z_scores(@grades)\np @$zs   # (-1, 0, 1)\n@data |> z_scores |> e p\n```",
        "percentile_rank" => "`percentile_rank($value, @data)` — returns the percentile rank (0-100) of a value within a dataset. Indicates what percentage of the data falls below the given value. Useful for ranking and normalization.\n\n```perl\nmy @scores = 1..100\np percentile_rank(50, @scores)   # ~50\np percentile_rank(90, @scores)   # ~90\n```",
        "quartiles" => "`quartiles(@data)` — returns the three quartile values [Q1, Q2, Q3] of a dataset. Q1 is the 25th percentile, Q2 is the median (50th), Q3 is the 75th percentile. Useful for understanding data distribution and computing IQR.\n\n```perl\nmy @data = 1..100\nmy $q = quartiles(@data)\np \"Q1=$q->[0] Q2=$q->[1] Q3=$q->[2]\"\n```",
        "spearman_correlation" | "spearman" => "`spearman_correlation(\\@x, \\@y)` (alias `spearman`) — computes Spearman's rank correlation coefficient between two datasets. Measures monotonic relationships, robust to outliers. Returns a value from -1 to 1.\n\n```perl\nmy @x = (1, 2, 3, 4, 5)\nmy @y = (5, 6, 7, 8, 7)\np spearman(\\@x, \\@y)   # high positive correlation\n```",
        "t_test_one_sample" | "ttest1" => "`t_test_one_sample($mu, @sample)` (alias `ttest1`) — performs a one-sample t-test comparing a sample mean against a hypothesized population mean mu. Returns [t_statistic, degrees_of_freedom]. Use t-distribution tables for p-value lookup.\n\n```perl\nmy @sample = (5.1, 4.9, 5.2, 5.0, 4.8)\nmy $result = ttest1(5.0, @sample)\np \"t=$result->[0] df=$result->[1]\"\n```",
        "t_test_two_sample" | "ttest2" => "`t_test_two_sample(\\@a, \\@b)` (alias `ttest2`) — performs an independent two-sample t-test comparing means of two samples. Assumes equal variances. Returns [t_statistic, degrees_of_freedom].\n\n```perl\nmy @a = (5.1, 5.3, 4.9)\nmy @b = (4.2, 4.5, 4.1)\nmy $r = ttest2(\\@a, \\@b)\np \"t=$r->[0] df=$r->[1]\"\n```",
        "chi_square_stat" | "chi2" => "`chi_square_stat(\\@observed, \\@expected)` (alias `chi2`) — computes the chi-squared test statistic comparing observed vs expected frequencies. Useful for goodness-of-fit tests. Returns the chi-squared value.\n\n```perl\nmy @obs = (10, 20, 30)\nmy @exp = (15, 20, 25)\np chi2(\\@obs, \\@exp)   # chi-squared statistic\n```",
        "gini" | "gini_coefficient" => "`gini(@data)` (alias `gini_coefficient`) — computes the Gini coefficient measuring inequality in a distribution. Returns a value from 0 (perfect equality) to 1 (perfect inequality). Commonly used for income distribution analysis.\n\n```perl\np gini(1, 1, 1, 1)     # 0 (perfect equality)\np gini(0, 0, 0, 100)   # ~0.75 (high inequality)\n```",
        "lorenz_curve" | "lorenz" => "`lorenz_curve(@data)` (alias `lorenz`) — computes the Lorenz curve as an arrayref of [cumulative_share_of_population, cumulative_share_of_wealth] pairs. Used to visualize inequality. The diagonal represents perfect equality.\n\n```perl\nmy $curve = lorenz(10, 20, 30, 40)\nfor my $pt (@$curve) {\n    p \"$pt->[0], $pt->[1]\"\n}\n```",
        "outliers_iqr" => "`outliers_iqr(@data)` — identifies outliers using the IQR method (1.5 × IQR beyond Q1/Q3). Returns an arrayref of outlier values. Standard approach for detecting unusual data points.\n\n```perl\nmy @data = (1, 2, 3, 4, 5, 100)\nmy $out = outliers_iqr(@data)\np @$out   # (100)\n```",
        "five_number_summary" | "fivenum" => "`five_number_summary(@data)` (alias `fivenum`) — returns [min, Q1, median, Q3, max], the classic five-number summary used for box plots. Provides a quick view of data distribution.\n\n```perl\nmy @data = 1..100\nmy $f = fivenum(@data)\np \"min=$f->[0] Q1=$f->[1] med=$f->[2] Q3=$f->[3] max=$f->[4]\"\n```",
        "describe" => "`describe(@data)` — returns a hashref with comprehensive descriptive statistics: count, mean, std, min, 25%, 50%, 75%, max. Similar to pandas DataFrame.describe().\n\n```perl\nmy $stats = describe(1..100)\np $stats->{mean}    # 50.5\np $stats->{std}     # ~29.0\np $stats->{\"50%\"}   # median\n```",

        // ── Geometry (extended) ───────────────────────────────────────────────
        "scale_point" => "`scale_point($x, $y, $sx, $sy)` — scales a 2D point by factors sx and sy from the origin. Returns [$x*$sx, $y*$sy]. Use for geometric transformations.\n\n```perl\nmy $p = scale_point(2, 3, 2, 2)\np @$p   # (4, 6)\n```",
        "translate_point" => "`translate_point($x, $y, $dx, $dy)` — translates a 2D point by offset (dx, dy). Returns [$x+$dx, $y+$dy]. Basic vector addition.\n\n```perl\nmy $p = translate_point(1, 2, 3, 4)\np @$p   # (4, 6)\n```",
        "reflect_point" => "`reflect_point($x, $y, $axis)` — reflects a point across an axis. Axis can be 'x', 'y', or 'origin'. Returns the reflected coordinates.\n\n```perl\np reflect_point(3, 4, 'x')       # [3, -4]\np reflect_point(3, 4, 'y')       # [-3, 4]\np reflect_point(3, 4, 'origin')  # [-3, -4]\n```",
        "angle_between" => "`angle_between($x1, $y1, $x2, $y2)` — computes the angle in radians between two 2D vectors from origin. Returns the angle using atan2.\n\n```perl\nmy $angle = angle_between(1, 0, 0, 1)\np $angle   # ~1.5708 (π/2 radians = 90°)\n```",
        "line_intersection" => "`line_intersection($x1, $y1, $x2, $y2, $x3, $y3, $x4, $y4)` — finds the intersection point of two lines defined by points (x1,y1)-(x2,y2) and (x3,y3)-(x4,y4). Returns [$x, $y] or undef if parallel.\n\n```perl\nmy $pt = line_intersection(0, 0, 1, 1, 0, 1, 1, 0)\np @$pt   # (0.5, 0.5)\n```",
        "point_in_polygon" | "pip" => "`point_in_polygon($x, $y, @polygon)` (alias `pip`) — tests if a point lies inside a polygon. Polygon is given as flat list [x1,y1,x2,y2,...]. Returns 1 if inside, 0 if outside. Uses ray-casting algorithm.\n\n```perl\nmy @square = (0,0, 4,0, 4,4, 0,4)\np pip(2, 2, @square)   # 1 (inside)\np pip(5, 5, @square)   # 0 (outside)\n```",
        "convex_hull" | "chull" => "`convex_hull(@points)` (alias `chull`) — computes the convex hull of a set of 2D points. Points given as flat list [x1,y1,x2,y2,...]. Returns the hull vertices in counterclockwise order. Uses Graham scan algorithm.\n\n```perl\nmy @pts = (0,0, 1,1, 2,0, 1,0.5)\nmy $hull = chull(@pts)\np @$hull\n```",
        "bounding_box" | "bbox" => "`bounding_box(@points)` (alias `bbox`) — computes the axis-aligned bounding box of 2D points. Returns [min_x, min_y, max_x, max_y].\n\n```perl\nmy @pts = (1,2, 3,4, 0,1, 5,3)\nmy $bb = bbox(@pts)\np @$bb   # (0, 1, 5, 4)\n```",
        "centroid" => "`centroid(@points)` — computes the centroid (geometric center) of 2D points. Points as flat list [x1,y1,x2,y2,...]. Returns [$cx, $cy].\n\n```perl\nmy @triangle = (0,0, 3,0, 0,3)\nmy $c = centroid(@triangle)\np @$c   # (1, 1)\n```",
        "polygon_perimeter" | "polyper" => "`polygon_perimeter(@points)` (alias `polyper`) — computes the perimeter of a polygon. Points as flat list [x1,y1,x2,y2,...]. Assumes closed polygon.\n\n```perl\nmy @square = (0,0, 1,0, 1,1, 0,1)\np polyper(@square)   # 4\n```",
        "circle_from_three_points" | "circ3" => "`circle_from_three_points($x1, $y1, $x2, $y2, $x3, $y3)` (alias `circ3`) — finds the unique circle passing through three non-collinear points. Returns [$cx, $cy, $radius] or undef if collinear.\n\n```perl\nmy $c = circ3(0, 0, 1, 0, 0.5, 0.866)\np \"center=($c->[0], $c->[1]) r=$c->[2]\"\n```",
        "arc_length" => "`arc_length($radius, $theta)` — computes the arc length of a circular arc. Theta is the central angle in radians. Returns `radius * theta`.\n\n```perl\np arc_length(10, 3.14159)   # ~31.4 (half circle)\np arc_length(5, 1.57)       # ~7.85 (quarter circle)\n```",
        "sector_area" => "`sector_area($radius, $theta)` — computes the area of a circular sector. Theta is the central angle in radians. Returns `0.5 * r² * theta`.\n\n```perl\np sector_area(10, 3.14159)   # ~157 (half circle)\np sector_area(5, 1.57)       # ~19.6\n```",
        "torus_volume" => "`torus_volume($major_r, $minor_r)` — computes the volume of a torus with major radius R (center to tube center) and minor radius r (tube radius). Returns `2π²Rr²`.\n\n```perl\np torus_volume(10, 2)   # ~789.6\n```",
        "torus_surface" => "`torus_surface($major_r, $minor_r)` — computes the surface area of a torus. Returns `4π²Rr`.\n\n```perl\np torus_surface(10, 2)   # ~789.6\n```",
        "pyramid_volume" => "`pyramid_volume($base_area, $height)` — computes the volume of a pyramid. Returns `(1/3) * base_area * height`.\n\n```perl\np pyramid_volume(100, 15)   # 500\n```",
        "frustum_volume" => "`frustum_volume($r1, $r2, $h)` — computes the volume of a conical frustum (truncated cone) with base radii r1 and r2 and height h.\n\n```perl\np frustum_volume(5, 3, 10)   # ~513\n```",
        "ellipse_perimeter" | "ellper" => "`ellipse_perimeter($a, $b)` (alias `ellper`) — approximates the perimeter of an ellipse with semi-axes a and b. Uses Ramanujan's approximation.\n\n```perl\np ellper(5, 3)   # ~25.5\np ellper(10, 10)   # ~62.8 (circle)\n```",
        "haversine_distance" | "haversine" => "`haversine_distance($lat1, $lon1, $lat2, $lon2)` (alias `haversine`) — computes the great-circle distance between two points on Earth given latitude/longitude in degrees. Returns distance in kilometers.\n\n```perl\n# NYC to LA\np haversine(40.7128, -74.0060, 34.0522, -118.2437)\n# ~3940 km\n```",
        "vector_dot" | "vdot" => "`vector_dot(\\@a, \\@b)` (alias `vdot`) — computes the dot product of two vectors. Returns the scalar sum of element-wise products.\n\n```perl\np vdot([1,2,3], [4,5,6])   # 32\n```",
        "vector_cross" | "vcross" => "`vector_cross(\\@a, \\@b)` (alias `vcross`) — computes the cross product of two 3D vectors. Returns a 3-element arrayref perpendicular to both inputs.\n\n```perl\nmy $c = vcross([1,0,0], [0,1,0])\np @$c   # (0, 0, 1)\n```",
        "vector_magnitude" | "vmag" => "`vector_magnitude(\\@v)` (alias `vmag`) — computes the Euclidean length (L2 norm) of a vector.\n\n```perl\np vmag([3, 4])   # 5\np vmag([1, 2, 2])   # 3\n```",
        "vector_normalize" | "vnorm" => "`vector_normalize(\\@v)` (alias `vnorm`) — returns a unit vector in the same direction. Divides by magnitude.\n\n```perl\nmy $u = vnorm([3, 4])\np @$u   # (0.6, 0.8)\n```",
        "vector_angle" | "vangle" => "`vector_angle(\\@a, \\@b)` (alias `vangle`) — computes the angle in radians between two vectors using the dot product formula.\n\n```perl\np vangle([1,0], [0,1])   # ~1.5708 (π/2)\np vangle([1,0], [1,0])   # 0\n```",

        // ── Financial (extended) ──────────────────────────────────────────────
        "irr" => "`irr(@cashflows)` — computes the Internal Rate of Return for a series of cash flows. First value is typically negative (initial investment). Uses Newton-Raphson iteration.\n\n```perl\nmy @cf = (-1000, 300, 420, 680)\np irr(@cf)   # ~0.166 (16.6%)\n```",
        "xirr" => "`xirr(\\@cashflows, \\@dates)` — computes IRR for irregularly-spaced cash flows. Dates can be epoch timestamps or date strings. More accurate than `irr` for real-world investments.\n\n```perl\nmy @cf = (-1000, 500, 600)\nmy @dates = ('2020-01-01', '2020-07-01', '2021-01-01')\np xirr(\\@cf, \\@dates)\n```",
        "payback_period" | "payback" => "`payback_period($initial, @cashflows)` (alias `payback`) — computes the number of periods to recover initial investment. Returns fractional periods.\n\n```perl\np payback(1000, 300, 400, 500)   # 2.6 periods\n```",
        "discounted_payback" => "`discounted_payback($initial, $rate, @cashflows)` — computes payback period using discounted cash flows. Accounts for time value of money.\n\n```perl\np discounted_payback(1000, 0.1, 400, 400, 400, 400)\n```",
        "pmt" => "`pmt($rate, $nper, $pv)` — computes the periodic payment for a loan given interest rate, number of periods, and present value. Standard amortization formula.\n\n```perl\n# $100k loan, 5% annual rate, 30 years\np pmt(0.05/12, 360, 100000)   # ~$537/month\n```",
        "nper" | "num_periods" => "`nper($rate, $pmt, $pv)` (alias `num_periods`) — computes the number of periods needed to pay off a loan given rate, payment, and principal.\n\n```perl\np nper(0.05/12, 500, 50000)   # ~127 months\n```",
        "amortization_schedule" | "amort" => "`amortization_schedule($principal, $rate, $periods)` (alias `amort`) — generates a full amortization schedule. Returns arrayref of hashrefs with period, payment, principal, interest, balance.\n\n```perl\nmy $sched = amort(10000, 0.05, 12)\nfor my $row (@$sched) {\n    p \"Period $row->{period}: bal=$row->{balance}\"\n}\n```",
        "bond_price" => "`bond_price($face, $coupon_rate, $ytm, $periods)` — computes the present value (price) of a bond given face value, coupon rate, yield to maturity, and number of periods.\n\n```perl\np bond_price(1000, 0.05, 0.04, 10)   # ~1081 (premium)\np bond_price(1000, 0.05, 0.06, 10)   # ~926 (discount)\n```",
        "bond_yield" => "`bond_yield($price, $face, $coupon_rate, $periods)` — computes the yield to maturity of a bond given its current price. Uses Newton-Raphson iteration.\n\n```perl\np bond_yield(950, 1000, 0.05, 10)   # ~5.7%\n```",
        "duration" | "bond_duration" => "`duration($face, $coupon_rate, $ytm, $periods)` (alias `bond_duration`) — computes the Macaulay duration of a bond, measuring interest rate sensitivity as weighted average time to receive cash flows.\n\n```perl\np duration(1000, 0.05, 0.05, 10)   # ~7.7 years\n```",
        "modified_duration" | "mod_dur" => "`modified_duration($face, $coupon_rate, $ytm, $periods)` (alias `mod_dur`) — computes modified duration, which directly measures price sensitivity to yield changes. Equal to Macaulay duration / (1 + ytm).\n\n```perl\np mod_dur(1000, 0.05, 0.05, 10)   # ~7.3\n```",
        "sharpe_ratio" | "sharpe" => "`sharpe_ratio(\\@returns, $risk_free_rate)` (alias `sharpe`) — computes the Sharpe ratio measuring risk-adjusted returns. Higher is better. Returns (mean_return - rf) / stddev.\n\n```perl\nmy @returns = (0.05, -0.02, 0.08, 0.03)\np sharpe(\\@returns, 0.02)   # risk-adjusted performance\n```",
        "sortino_ratio" | "sortino" => "`sortino_ratio(\\@returns, $target)` (alias `sortino`) — computes the Sortino ratio, similar to Sharpe but only penalizes downside volatility. Better for asymmetric return distributions.\n\n```perl\nmy @returns = (0.05, -0.02, 0.08, -0.01)\np sortino(\\@returns, 0.0)   # penalizes only negative returns\n```",
        "max_drawdown" | "mdd" => "`max_drawdown(@equity_curve)` (alias `mdd`) — computes the maximum peak-to-trough decline in a series of values. Returns as a decimal (0.2 = 20% drawdown). Key risk metric.\n\n```perl\nmy @equity = (100, 120, 90, 110, 85, 130)\np mdd(@equity)   # ~0.29 (29% max drawdown)\n```",
        "continuous_compound" | "ccomp" => "`continuous_compound($principal, $rate, $time)` (alias `ccomp`) — computes future value with continuous compounding: P × e^(rt).\n\n```perl\np ccomp(1000, 0.05, 10)   # ~1648.72\n```",
        "rule_of_72" | "r72" => "`rule_of_72($rate)` (alias `r72`) — estimates years to double investment using the Rule of 72: 72 / (rate × 100). Quick mental math approximation.\n\n```perl\np r72(0.06)   # 12 years to double at 6%\np r72(0.10)   # 7.2 years at 10%\n```",
        "wacc" => "`wacc($equity, $debt, $cost_equity, $cost_debt, $tax_rate)` — computes Weighted Average Cost of Capital. Returns the blended cost of capital accounting for the tax shield on debt.\n\n```perl\np wacc(60, 40, 0.10, 0.05, 0.25)   # ~7.5%\n```",
        "capm" => "`capm($risk_free, $beta, $market_return)` — computes expected return using the Capital Asset Pricing Model: rf + β(rm - rf).\n\n```perl\np capm(0.02, 1.2, 0.08)   # 9.2% expected return\n```",
        "black_scholes_call" | "bscall" => "`black_scholes_call($S, $K, $r, $T, $sigma)` (alias `bscall`) — computes Black-Scholes price for a European call option. S=spot, K=strike, r=rate, T=time to expiry, sigma=volatility.\n\n```perl\np bscall(100, 100, 0.05, 1, 0.2)   # ~10.45\n```",
        "black_scholes_put" | "bsput" => "`black_scholes_put($S, $K, $r, $T, $sigma)` (alias `bsput`) — computes Black-Scholes price for a European put option.\n\n```perl\np bsput(100, 100, 0.05, 1, 0.2)   # ~5.57\n```",

        // ── DSP / Signal (extended) ───────────────────────────────────────────
        "lowpass_filter" | "lpf" => "`lowpass_filter(\\@signal, $cutoff)` (alias `lpf`) — applies a simple low-pass filter to remove high-frequency components. Cutoff is normalized (0-1). Uses exponential moving average.\n\n```perl\nmy @noisy = map { sin($_/10) + rand(0.2) } 1..100\nmy $smooth = lpf(\\@noisy, 0.1)\n```",
        "highpass_filter" | "hpf" => "`highpass_filter(\\@signal, $cutoff)` (alias `hpf`) — applies a simple high-pass filter to remove low-frequency components (DC offset, drift). Signal = original - lowpass.\n\n```perl\nmy @with_dc = map { 5 + sin($_/10) } 1..100\nmy $ac_only = hpf(\\@with_dc, 0.1)\n```",
        "bandpass_filter" | "bpf" => "`bandpass_filter(\\@signal, $low, $high)` (alias `bpf`) — applies a band-pass filter passing frequencies between low and high cutoffs. Combines low-pass and high-pass.\n\n```perl\nmy $filtered = bpf(\\@signal, 0.1, 0.3)\n```",
        "median_filter" | "medfilt" => "`median_filter(\\@signal, $window_size)` (alias `medfilt`) — applies a median filter for noise removal. Each output is the median of the surrounding window. Excellent for removing impulse noise while preserving edges.\n\n```perl\nmy @spiky = (1, 2, 100, 3, 4)   # 100 is spike\nmy $clean = medfilt(\\@spiky, 3)\np @$clean   # spike removed\n```",
        "window_hann" | "hann" => "`window_hann($n)` (alias `hann`) — generates a Hann (raised cosine) window of length n. Common window function for spectral analysis that reduces spectral leakage.\n\n```perl\nmy $w = hann(1024)\n```",
        "window_hamming" | "hamming" => "`window_hamming($n)` (alias `hamming`) — generates a Hamming window of length n. Similar to Hann but with slightly different coefficients, optimized for speech processing.\n\n```perl\nmy $w = hamming(1024)\n```",
        "window_blackman" | "blackman" => "`window_blackman($n)` (alias `blackman`) — generates a Blackman window of length n. Provides excellent sidelobe suppression at the cost of main lobe width.\n\n```perl\nmy $w = blackman(1024)\n```",
        "window_kaiser" | "kaiser" => "`window_kaiser($n, $beta)` (alias `kaiser`) — generates a Kaiser window with parameter beta controlling the tradeoff between main lobe width and sidelobe level. Beta=0 is rectangular, beta~5 is similar to Hamming.\n\n```perl\nmy $w = kaiser(1024, 5.0)\n```",
        "apply_window" => "`apply_window(\\@signal, \\@window)` — element-wise multiplies signal by window function. Use before FFT to reduce spectral leakage.\n\n```perl\nmy $w = hann(scalar @signal)\nmy $windowed = apply_window(\\@signal, $w)\nmy $spectrum = dft($windowed)\n```",
        "dft" => "`dft(\\@signal)` — computes the Discrete Fourier Transform. Returns arrayref of complex numbers as [re, im] pairs. O(n²) reference implementation; for large n, use FFT libraries.\n\n```perl\nmy @signal = map { sin(2 * 3.14159 * $_ / 64) } 0..63\nmy $spectrum = dft(\\@signal)\n```",
        "idft" => "`idft(\\@spectrum)` — computes the Inverse Discrete Fourier Transform. Takes arrayref of [re, im] pairs and returns the time-domain signal.\n\n```perl\nmy $spectrum = dft(\\@signal)\nmy $reconstructed = idft($spectrum)\n```",
        "power_spectrum" | "psd" => "`power_spectrum(\\@signal)` (alias `psd`) — computes the power spectral density (magnitude squared of DFT). Returns real values representing power at each frequency bin.\n\n```perl\nmy $psd = psd(\\@signal)\n@$psd |> e p\n```",
        "phase_spectrum" => "`phase_spectrum(\\@signal)` — computes the phase angle (in radians) at each frequency bin from the DFT.\n\n```perl\nmy $phases = phase_spectrum(\\@signal)\n```",
        "spectrogram" | "stft" => "`spectrogram(\\@signal, $window_size, $hop_size)` (alias `stft`) — computes Short-Time Fourier Transform. Returns a 2D arrayref where each row is the spectrum of a windowed segment. Useful for time-frequency analysis.\n\n```perl\nmy $spec = stft(\\@audio, 1024, 512)\n# $spec->[t][f] is magnitude at time t, frequency f\n```",
        "resample" => "`resample(\\@signal, $factor)` — resamples signal by a rational factor. Factor > 1 upsamples, factor < 1 downsamples. Uses linear interpolation.\n\n```perl\nmy $up = resample(\\@signal, 2)     # double sample rate\nmy $down = resample(\\@signal, 0.5) # halve sample rate\n```",
        "downsample" | "decimate" => "`downsample(\\@signal, $factor)` (alias `decimate`) — reduces sample rate by keeping every nth sample. Apply anti-aliasing filter first to avoid aliasing.\n\n```perl\nmy $decimated = decimate(\\@signal, 4)  # keep every 4th sample\n```",
        "upsample" | "interpolate" => "`upsample(\\@signal, $factor)` (alias `interpolate`) — increases sample rate by inserting zeros and filtering. Factor must be positive integer.\n\n```perl\nmy $upsampled = interpolate(\\@signal, 4)  # 4x sample rate\n```",
        "normalize_signal" | "normsig" => "`normalize_signal(\\@signal)` (alias `normsig`) — scales signal to range [-1, 1] based on its peak absolute value. Useful for audio processing.\n\n```perl\nmy $norm = normsig(\\@audio)\np max(map { abs } @$norm)   # 1\n```",
        "energy" => "`energy(\\@signal)` — computes the total energy of a signal as the sum of squared samples. Useful for audio analysis and feature extraction.\n\n```perl\np energy(\\@signal)   # total signal energy\n```",
        "spectral_centroid" | "scentroid" => "`spectral_centroid(\\@spectrum)` (alias `scentroid`) — computes the spectral centroid (center of mass) of a spectrum. Indicates the 'brightness' of a sound. Returns frequency bin index.\n\n```perl\nmy $psd = psd(\\@signal)\np scentroid($psd)   # brightness measure\n```",
        "envelope" | "hilbert_env" => "`envelope(\\@signal)` (alias `hilbert_env`) — computes the amplitude envelope using Hilbert transform magnitude. Useful for amplitude modulation analysis.\n\n```perl\nmy $env = envelope(\\@signal)\n# $env traces the amplitude of oscillations\n```",
        "cross_correlation" | "xcorr" => "`cross_correlation(\\@a, \\@b)` (alias `xcorr`) — computes the cross-correlation of two signals. Measures similarity as a function of time-lag. Peak location indicates time delay between signals.\n\n```perl\nmy $xcor = xcorr(\\@signal1, \\@signal2)\nmy $lag = max_index(@$xcor)   # find peak lag\n```",

        // ── Math Formulas ───────────────────────────────────────────────────────
        "quadratic_roots" | "qroots" => "`quadratic_roots($a, $b, $c)` (alias `qroots`) — solves ax² + bx + c = 0. Returns [x1, x2] for real roots, undef if no real solutions (discriminant < 0).\n\n```perl\nmy @roots = quadratic_roots(1, -5, 6)  # [3, 2] (x² - 5x + 6 = 0)\np qroots(1, 0, -4)                      # [2, -2]\np qroots(1, 2, 5)                       # undef (complex roots)\n```",
        "quadratic_discriminant" | "qdisc" => "`quadratic_discriminant($a, $b, $c)` (alias `qdisc`) — computes b² - 4ac. Positive = two real roots, zero = one repeated root, negative = complex roots.\n\n```perl\np qdisc(1, -5, 6)   # 1 (two distinct real roots)\np qdisc(1, 2, 1)    # 0 (one repeated root)\np qdisc(1, 2, 5)    # -16 (complex roots)\n```",
        "arithmetic_series" | "arithser" => "`arithmetic_series($a1, $d, $n)` (alias `arithser`) — sum of n terms of arithmetic sequence starting at a1 with common difference d. Formula: n/2 × (2a1 + (n-1)d).\n\n```perl\np arithser(1, 1, 100)    # 5050 (1+2+...+100)\np arithser(2, 3, 10)     # 155 (2+5+8+...+29)\n```",
        "geometric_series" | "geomser" => "`geometric_series($a1, $r, $n)` (alias `geomser`) — sum of n terms of geometric sequence. Formula: a1×(1-rⁿ)/(1-r). If r=1, returns a1×n.\n\n```perl\np geomser(1, 2, 10)      # 1023 (1+2+4+...+512)\np geomser(1, 0.5, 10)    # ~1.998 (converging to 2)\n```",
        "permutations" | "perm" => "`permutations($n, $r)` (alias `perm`) — number of ways to arrange r items from n. Formula: n!/(n-r)!. Order matters.\n\n```perl\np perm(5, 3)    # 60 (ways to arrange 3 items from 5)\np perm(10, 2)   # 90\n```",
        "combinations" | "comb" => "`combinations($n, $r)` (alias `comb`) — number of ways to choose r items from n. Formula: n!/(r!(n-r)!). Order doesn't matter.\n\n```perl\np comb(5, 3)    # 10 (ways to choose 3 items from 5)\np comb(52, 5)   # 2598960 (poker hands)\n```",
        "lerp" => "`lerp($a, $b, $t)` — linear interpolation between a and b. When t=0 returns a, t=1 returns b, t=0.5 returns midpoint.\n\n```perl\np lerp(0, 100, 0.5)   # 50\np lerp(10, 20, 0.25)  # 12.5\n# Animation: smoothly transition between values\nfor my $t (0..10) { p lerp($start, $end, $t/10) }\n```",
        "smoothstep" => "`smoothstep($edge0, $edge1, $x)` — smooth Hermite interpolation. Returns 0 if x ≤ edge0, 1 if x ≥ edge1, otherwise smooth S-curve. Great for animations.\n\n```perl\np smoothstep(0, 1, 0.5)   # 0.5 (but with smooth acceleration/deceleration)\np smoothstep(10, 20, 15)  # 0.5\n```",
        "map_range" | "remap" => "`map_range($value, $in_min, $in_max, $out_min, $out_max)` (alias `remap`) — remap a value from one range to another.\n\n```perl\np remap(50, 0, 100, 0, 1)     # 0.5\np remap(75, 0, 100, 0, 255)   # 191.25 (percent to byte)\n```",
        "normal_pdf" | "normpdf" => "`normal_pdf($x, $mu, $sigma)` (alias `normpdf`) — normal/Gaussian distribution probability density function. Default: μ=0, σ=1 (standard normal).\n\n```perl\np normpdf(0)              # ~0.399 (peak of standard normal)\np normpdf(0, 100, 15)     # PDF at IQ=100, mean=100, stddev=15\n```",
        "normal_cdf" | "normcdf" => "`normal_cdf($x, $mu, $sigma)` (alias `normcdf`) — cumulative distribution function. Returns probability that a value is ≤ x.\n\n```perl\np normcdf(0)              # 0.5 (half below mean)\np normcdf(1.96)           # ~0.975 (95% confidence bound)\n```",
        "poisson_pmf" | "poisson" => "`poisson_pmf($k, $lambda)` (alias `poisson`) — Poisson probability mass function. P(X=k) for events with rate λ.\n\n```perl\np poisson(3, 5)   # P(X=3) when avg rate is 5\n```",
        "gamma_approx" => "`gamma_approx($z)` — Gamma function Γ(z) using Lanczos approximation. Extends factorial: Γ(n) = (n-1)!\n\n```perl\np gamma(5)    # 24 (same as 4!)\np gamma(0.5)  # ~1.772 (√π)\n```",
        "erf_approx" => "`erf_approx($x)` — error function. Used in probability, statistics, and partial differential equations.\n\n```perl\np erf(1)    # ~0.843\np erf(2)    # ~0.995\n```",

        // ── Physics Formulas ────────────────────────────────────────────────────
        "momentum" => "`momentum($mass, $velocity)` — compute momentum p = mv (kg⋅m/s).\n\n```perl\np momentum(10, 5)   # 50 kg⋅m/s\n```",
        "impulse" => "`impulse($force, $time)` — compute impulse J = F×t (N⋅s). Change in momentum.\n\n```perl\np impulse(100, 0.5)   # 50 N⋅s\n```",
        "work" => "`work($force, $distance, $angle)` — compute work W = F×d×cos(θ) (Joules). Angle in degrees, default 0.\n\n```perl\np work(100, 10)       # 1000 J (force parallel to motion)\np work(100, 10, 60)   # 500 J (force at 60°)\n```",
        "torque" => "`torque($force, $lever_arm, $angle)` — compute torque τ = r×F×sin(θ) (N⋅m). Angle in degrees, default 90.\n\n```perl\np torque(50, 0.5)     # 25 N⋅m (perpendicular force)\np torque(50, 0.5, 30) # 12.5 N⋅m\n```",
        "centripetal_force" | "centrip" => "`centripetal_force($mass, $velocity, $radius)` (alias `centrip`) — F = mv²/r for circular motion.\n\n```perl\np centrip(1000, 20, 50)   # 8000 N (car turning)\n```",
        "escape_velocity" | "escvel" => "`escape_velocity($mass, $radius)` (alias `escvel`) — minimum velocity to escape gravitational field: v = √(2GM/r). Defaults to Earth values.\n\n```perl\np escvel                        # ~11186 m/s (Earth)\np escvel(1.989e30, 6.96e8)      # ~617 km/s (Sun)\n```",
        "orbital_velocity" | "orbvel" => "`orbital_velocity($mass, $radius)` (alias `orbvel`) — velocity for circular orbit: v = √(GM/r). Defaults to LEO around Earth.\n\n```perl\np orbvel    # ~7672 m/s (ISS orbital speed)\n```",
        "orbital_period" | "orbper" => "`orbital_period($mass, $radius)` (alias `orbper`) — period for circular orbit: T = 2π√(r³/GM). Returns seconds.\n\n```perl\nmy $t = orbper       # ~5560 s (ISS orbital period)\np $t / 60            # ~93 minutes\n```",
        "gravitational_force" | "gforce" => "`gravitational_force($m1, $m2, $r)` (alias `gforce`) — Newton's law of gravitation: F = Gm₁m₂/r².\n\n```perl\np gforce(5.972e24, 1000, 6.371e6)   # ~9820 N (1 ton at Earth surface)\n```",
        "coulomb_force" | "coulomb" => "`coulomb_force($q1, $q2, $r)` (alias `coulomb`) — Coulomb's law: F = kq₁q₂/r². Charges in Coulombs, distance in meters.\n\n```perl\np coulomb(1e-6, 1e-6, 0.1)   # ~0.9 N (two 1μC charges 10cm apart)\n```",
        "lorentz_factor" | "lorentz" => "`lorentz_factor($velocity)` (alias `lorentz`) — relativistic gamma factor γ = 1/√(1-v²/c²). Returns infinity at v ≥ c.\n\n```perl\np lorentz(0)           # 1 (at rest)\np lorentz(0.9 * 3e8)   # ~2.29 (at 90% speed of light)\n```",
        "time_dilation" | "tdilate" => "`time_dilation($proper_time, $velocity)` (alias `tdilate`) — dilated time Δt = Δt₀ × γ. Time appears to slow for moving objects.\n\n```perl\np tdilate(1, 0.99 * 3e8)   # ~7.09 s (1 second at 99% c)\n```",
        "length_contraction" | "lcontract" => "`length_contraction($proper_length, $velocity)` (alias `lcontract`) — contracted length L = L₀ × √(1-v²/c²).\n\n```perl\np lcontract(1, 0.99 * 3e8)   # ~0.14 m (1m at 99% c)\n```",
        "de_broglie_wavelength" | "debroglie" => "`de_broglie_wavelength($mass, $velocity)` (alias `debroglie`) — quantum wavelength λ = h/(mv). Defaults to electron mass.\n\n```perl\np debroglie(9.109e-31, 1e6)   # ~7.3e-10 m (electron at 1 km/s)\n```",
        "photon_energy" | "photonenergy" => "`photon_energy($frequency)` (alias `photonenergy`) — energy E = hf (Joules) of a photon.\n\n```perl\np photonenergy(5e14)   # ~3.3e-19 J (visible light)\n```",
        "schwarzschild_radius" | "schwarz" => "`schwarzschild_radius($mass)` (alias `schwarz`) — event horizon radius rs = 2GM/c² of a black hole.\n\n```perl\np schwarz(1.989e30)   # ~2954 m (Sun as black hole)\np schwarz(5.972e24)   # ~0.009 m (Earth as black hole)\n```",
        "ideal_gas_pressure" | "gasp" => "`ideal_gas_pressure($n, $V, $T)` (alias `gasp`) — pressure P = nRT/V. n moles, V volume (m³), T temperature (K).\n\n```perl\np gasp(1, 0.0224, 273.15)   # ~101325 Pa (1 mol at STP)\n```",
        "projectile_range" | "projrange" => "`projectile_range($velocity, $angle)` (alias `projrange`) — horizontal range R = v²sin(2θ)/g. Angle in degrees.\n\n```perl\np projrange(100, 45)   # ~1020 m (optimal angle for range)\np projrange(100, 30)   # ~884 m\n```",
        "projectile_max_height" | "projheight" => "`projectile_max_height($velocity, $angle)` (alias `projheight`) — maximum height H = v²sin²(θ)/(2g).\n\n```perl\np projheight(100, 45)   # ~255 m\np projheight(100, 90)   # ~510 m (straight up)\n```",
        "pendulum_period" | "pendper" => "`pendulum_period($length)` (alias `pendper`) — period T = 2π√(L/g) of a simple pendulum.\n\n```perl\np pendper(1)      # ~2.01 s (1 meter pendulum)\np pendper(0.25)   # ~1.00 s (grandfather clock)\n```",
        "doppler_frequency" | "doppler" => "`doppler_frequency($source_freq, $source_vel, $observer_vel)` (alias `doppler`) — observed frequency with Doppler effect. Uses speed of sound (343 m/s).\n\n```perl\np doppler(440, -30, 0)   # ~482 Hz (ambulance approaching at 30 m/s)\np doppler(440, 30, 0)    # ~405 Hz (ambulance receding)\n```",
        "snells_law" | "snell" => "`snells_law($n1, $n2, $theta1)` (alias `snell`) — refraction angle using Snell's law. Angles in degrees. Returns undef for total internal reflection.\n\n```perl\np snell(1, 1.5, 30)     # ~19.5° (air to glass)\np snell(1.5, 1, 45)     # undef (total internal reflection)\n```",
        "thin_lens" | "thinlens" => "`thin_lens($object_dist, $focal_length)` (alias `thinlens`) — image distance using 1/f = 1/do + 1/di.\n\n```perl\np thinlens(30, 20)   # 60 cm (real image)\np thinlens(10, 20)   # -20 cm (virtual image)\n```",

        // ── Math Constants ──────────────────────────────────────────────────────
        "euler_mascheroni" | "gamma_const" => "`euler_mascheroni` (alias `gamma_const`) — Euler-Mascheroni constant γ ≈ 0.5772. Appears in number theory and analysis.\n\n```perl\np euler_mascheroni   # 0.5772156649015329\n```",
        "catalan_constant" | "catalan" => "`catalan_constant` (alias `catalan`) — Catalan's constant G ≈ 0.9159. Appears in combinatorics and series.\n\n```perl\np catalan   # 0.9159655941772190\n```",
        "golden_ratio" | "phi" => "`golden_ratio` (alias `phi`) — φ = (1+√5)/2 ≈ 1.618. The golden ratio, appears throughout nature and art.\n\n```perl\np phi   # 1.618033988749895\n# Fibonacci limit: fib(n)/fib(n-1) → φ\n```",
        "silver_ratio" | "silver" => "`silver_ratio` (alias `silver`) — δS = 1 + √2 ≈ 2.414. Related to Pell numbers.\n\n```perl\np silver   # 2.414213562373095\n```",
        "feigenbaum_delta" | "feigd" => "`feigenbaum_delta` (alias `feigd`) — Feigenbaum constant δ ≈ 4.669. Universal constant in chaos theory.\n\n```perl\np feigd   # 4.669201609102990\n```",

        // ── Physics Constants ───────────────────────────────────────────────────
        "vacuum_permittivity" | "epsilon0" => "`vacuum_permittivity` (alias `epsilon0`) — ε₀ ≈ 8.854×10⁻¹² F/m. Electric constant, permittivity of free space.\n\n```perl\np epsilon0   # 8.8541878128e-12\n```",
        "fine_structure_constant" | "alpha_fs" => "`fine_structure_constant` (alias `alpha_fs`) — α ≈ 1/137 ≈ 0.00730. Fundamental constant of electromagnetism.\n\n```perl\np alpha_fs       # 0.0072973525693\np 1 / alpha_fs   # ~137.036\n```",
        "bohr_radius" | "a0" => "`bohr_radius` (alias `a0`) — a₀ ≈ 5.29×10⁻¹¹ m. Radius of hydrogen atom ground state.\n\n```perl\np a0   # 5.29177210903e-11\n```",
        "gas_constant" | "rgas" => "`gas_constant` (alias `rgas`) — R ≈ 8.314 J/(mol⋅K). Universal gas constant.\n\n```perl\np rgas   # 8.314462618\n```",
        "faraday_constant" | "faraday" => "`faraday_constant` (alias `faraday`) — F ≈ 96485 C/mol. Charge per mole of electrons.\n\n```perl\np faraday   # 96485.33212\n```",
        "astronomical_unit" | "au" => "`astronomical_unit` (alias `au`) — AU ≈ 1.496×10¹¹ m. Mean Earth-Sun distance.\n\n```perl\np au             # 1.495978707e11 m\np au / 1000      # ~149.6 million km\n```",
        "light_year" | "ly" => "`light_year` (alias `ly`) — ly ≈ 9.461×10¹⁵ m. Distance light travels in one year.\n\n```perl\np ly             # 9.4607304725808e15 m\np ly / au        # ~63241 AU\n```",
        "parsec" | "pc" => "`parsec` (alias `pc`) — pc ≈ 3.086×10¹⁶ m ≈ 3.26 ly. Common astronomical distance unit.\n\n```perl\np pc        # 3.0856775814913673e16 m\np pc / ly   # ~3.26 light years\n```",
        "planck_length" | "lplanck" => "`planck_length` (alias `lplanck`) — lP ≈ 1.616×10⁻³⁵ m. Smallest meaningful length in quantum mechanics.\n\n```perl\np lplanck   # 1.616255e-35 m\n```",
        "earth_mass" | "mearth" => "`earth_mass` (alias `mearth`) — M⊕ ≈ 5.972×10²⁴ kg. Mass of Earth.\n\n```perl\np mearth   # 5.972167867e24 kg\n```",
        "sun_mass" | "msun" => "`sun_mass` (alias `msun`) — M☉ ≈ 1.989×10³⁰ kg. Mass of Sun (solar mass).\n\n```perl\np msun   # 1.98892e30 kg\n```",

        // ── Linear Algebra ────────────────────────────────────────────────
        "matrix_solve" | "msolve" | "solve" => "`matrix_solve` (aliases `msolve`, `solve`) solves the linear system Ax=b via Gaussian elimination with partial pivoting. Returns the solution vector x.\n\n```perl\nmy $A = [[2,1],[-1,1]]\nmy $b = [5,2]\nmy $x = solve($A, $b)   # [1, 3]\n```",
        "matrix_lu" | "mlu" => "`matrix_lu` (alias `mlu`) computes the LU decomposition with partial pivoting. Returns [L, U, P] where PA = LU.\n\n```perl\nmy ($L, $U, $P) = @{mlu([[4,3],[6,3]])}\n```",
        "matrix_qr" | "mqr" => "`matrix_qr` (alias `mqr`) computes the QR decomposition via Gram-Schmidt orthogonalization. Returns [Q, R] where A = QR.\n\n```perl\nmy ($Q, $R) = @{mqr([[1,1],[1,-1]])}\n```",
        "matrix_eigenvalues" | "meig" | "eigenvalues" | "eig" => "`matrix_eigenvalues` (aliases `meig`, `eig`) computes eigenvalues of a square matrix via QR iteration. Returns an array of eigenvalues.\n\n```perl\nmy @eigs = @{eig([[2,1],[1,2]])}   # [3, 1]\n```",
        "matrix_norm" | "mnorm" => "`matrix_norm` (alias `mnorm`) computes a matrix norm. Default is Frobenius; pass 1 for max-column-sum, Inf for max-row-sum.\n\n```perl\np mnorm([[3,4]])           # 5 (Frobenius)\np mnorm([[1,2],[3,4]], 1)  # 6 (1-norm)\n```",
        "matrix_cond" | "mcond" | "cond" => "`matrix_cond` (aliases `mcond`, `cond`) estimates the condition number of a matrix (ratio of largest to smallest singular value). Large values indicate ill-conditioning.\n\n```perl\np cond([[1,0],[0,1]])    # 1 (perfect)\np cond([[1,2],[2,4]])    # Inf (singular)\n```",
        "matrix_pinv" | "mpinv" | "pinv" => "`matrix_pinv` (aliases `mpinv`, `pinv`) computes the Moore-Penrose pseudo-inverse via (A^T A)^{-1} A^T.\n\n```perl\nmy $A_plus = pinv([[1,2],[3,4],[5,6]])\n```",
        "matrix_cholesky" | "mchol" | "cholesky" => "`matrix_cholesky` (aliases `mchol`, `cholesky`) computes the Cholesky decomposition of a symmetric positive-definite matrix. Returns lower-triangular L where M = L·L^T.\n\n```perl\nmy $L = cholesky([[4,2],[2,3]])\n```",
        "matrix_det_general" | "mdetg" | "det" => "`matrix_det_general` (aliases `mdetg`, `det`) computes the determinant of any NxN matrix via LU decomposition.\n\n```perl\np det([[1,2,3],[4,5,6],[7,8,0]])  # 27\n```",

        // ── Statistics Tests ─────────────────────────────────────────────
        "welch_ttest" | "welcht" => "`welch_ttest` (alias `welcht`) performs Welch's t-test for two independent samples with unequal variances. Returns [t-statistic, degrees of freedom].\n\n```perl\nmy ($t, $df) = @{welcht([1,2,3,4,5], [3,4,5,6,7])}\np \"t=$t df=$df\"\n```",
        "paired_ttest" | "pairedt" => "`paired_ttest` (alias `pairedt`) performs a paired t-test on two matched samples. Returns [t-statistic, degrees of freedom].\n\n```perl\nmy ($t, $df) = @{pairedt([85,90,78], [88,92,80])}\n```",
        "cohen_d" | "cohend" => "`cohen_d` (alias `cohend`) computes Cohen's d effect size between two samples. Small=0.2, medium=0.5, large=0.8.\n\n```perl\np cohend([1,2,3], [4,5,6])  # large effect\n```",
        "anova_oneway" | "anova" | "anova1" => "`anova_oneway` (aliases `anova`, `anova1`) performs one-way ANOVA. Returns [F-statistic, df_between, df_within].\n\n```perl\nmy ($F, $dfb, $dfw) = @{anova([1,2,3], [4,5,6], [7,8,9])}\n```",
        "spearman_corr" | "rho" => "`spearman` (aliases `spearman_corr`, `rho`) computes Spearman's rank correlation coefficient between two samples.\n\n```perl\np rho([1,2,3,4,5], [5,6,7,8,7])  # ~0.82\n```",
        "kendall_tau" | "kendall" | "ktau" => "`kendall_tau` (aliases `kendall`, `ktau`) computes Kendall's rank correlation coefficient (tau-b).\n\n```perl\np ktau([1,2,3,4], [1,2,4,3])  # 0.67\n```",
        "confidence_interval" | "ci" => "`confidence_interval` (alias `ci`) computes a confidence interval for the mean. Default 95%. Returns [lower, upper].\n\n```perl\nmy ($lo, $hi) = @{ci([10,12,14,11,13])}\np \"95%% CI: $lo to $hi\"\nmy ($lo99, $hi99) = @{ci([10,12,14,11,13], 0.99)}\n```",

        // ── Distributions ────────────────────────────────────────────────
        "beta_pdf" | "betapdf" => "`beta_pdf` (alias `betapdf`) evaluates the Beta distribution PDF at x with shape parameters alpha and beta.\n\n```perl\np betapdf(0.5, 2, 5)  # Beta(2,5) at x=0.5\n```",
        "gamma_pdf" | "gammapdf" => "`gamma_pdf` (alias `gammapdf`) evaluates the Gamma distribution PDF at x with shape k and scale theta.\n\n```perl\np gammapdf(2.0, 2, 1)  # Gamma(2,1) at x=2\n```",
        "chi2_pdf" | "chi2pdf" | "chi_squared_pdf" => "`chi2_pdf` (alias `chi2pdf`) evaluates the chi-squared distribution PDF at x with k degrees of freedom.\n\n```perl\np chi2pdf(3.84, 1)  # p-value boundary for df=1\n```",
        "t_pdf" | "tpdf" | "student_pdf" => "`t_pdf` (alias `tpdf`) evaluates Student's t-distribution PDF at x with nu degrees of freedom.\n\n```perl\np tpdf(0, 10)    # peak of t(10)\np tpdf(2.228, 10)\n```",
        "f_pdf" | "fpdf" | "fisher_pdf" => "`f_pdf` (alias `fpdf`) evaluates the F-distribution PDF at x with d1 and d2 degrees of freedom.\n\n```perl\np fpdf(1.0, 5, 10)\n```",
        "lognormal_pdf" | "lnormpdf" => "`lognormal_pdf` (alias `lnormpdf`) evaluates the log-normal distribution PDF at x with parameters mu and sigma.\n\n```perl\np lnormpdf(1.0, 0, 1)  # LogN(0,1) at x=1\n```",
        "weibull_pdf" | "weibpdf" => "`weibull_pdf` (alias `weibpdf`) evaluates the Weibull distribution PDF at x with shape k and scale lambda.\n\n```perl\np weibpdf(1.0, 1.5, 1.0)\n```",
        "cauchy_pdf" | "cauchypdf" => "`cauchy_pdf` (alias `cauchypdf`) evaluates the Cauchy distribution PDF at x with location x0 and scale gamma.\n\n```perl\np cauchypdf(0, 0, 1)  # peak of standard Cauchy\n```",
        "laplace_pdf" | "laplacepdf" => "`laplace_pdf` (alias `laplacepdf`) evaluates the Laplace distribution PDF at x with location mu and scale b.\n\n```perl\np laplacepdf(0, 0, 1)  # 0.5 (peak)\n```",
        "pareto_pdf" | "paretopdf" => "`pareto_pdf` (alias `paretopdf`) evaluates the Pareto distribution PDF at x with minimum xm and shape alpha.\n\n```perl\np paretopdf(2, 1, 3)  # Pareto(1,3) at x=2\n```",

        // ── Interpolation ────────────────────────────────────────────────
        "lagrange_interp" | "lagrange" | "linterp" => "`lagrange_interp` (aliases `lagrange`, `linterp`) performs Lagrange polynomial interpolation. Takes xs, ys, and a query point x.\n\n```perl\np lagrange([0,1,2], [0,1,4], 1.5)  # 2.25\n```",
        "cubic_spline" | "cspline" | "spline" => "`cubic_spline` (aliases `cspline`, `spline`) performs natural cubic spline interpolation. Takes xs, ys, and a query point x.\n\n```perl\np spline([0,1,2,3], [0,1,0,1], 1.5)  # smooth interpolation\n```",
        "poly_eval" | "polyval" => "`poly_eval` (alias `polyval`) evaluates a polynomial c0 + c1·x + c2·x² + ... using Horner's method.\n\n```perl\np polyval([1, 0, 1], 3)  # 1 + 0*3 + 1*9 = 10\n```",
        "polynomial_fit" | "polyfit" => "`polynomial_fit` (alias `polyfit`) performs least-squares polynomial fitting. Returns coefficients [c0, c1, ..., cn].\n\n```perl\nmy $c = polyfit([0,1,2,3], [1,3,5,7], 1)  # linear fit\n```",

        // ── Numerical Methods ────────────────────────────────────────────
        "trapz" | "trapezoid" => "`trapz` (alias `trapezoid`) integrates evenly-spaced samples using the trapezoidal rule. Optional dx (default 1).\n\n```perl\nmy @y = map { $_ ** 2 } 0..100\np trapz(\\@y, 0.01)  # ≈ 0.3333\n```",
        "simpson" | "simps" => "`simpson` (alias `simps`) integrates evenly-spaced samples using Simpson's rule. More accurate than trapz for smooth functions.\n\n```perl\nmy @y = map { sin($_ * 0.01) } 0..314\np simps(\\@y, 0.01)  # ≈ 2.0\n```",
        "numerical_diff" | "numdiff" | "diff_array" => "`numerical_diff` (aliases `numdiff`, `diff_array`) computes the numerical first derivative via central differences. Returns an array.\n\n```perl\nmy @y = map { $_ ** 2 } 0..10\nmy @dy = @{numdiff(\\@y)}  # ≈ [0, 2, 4, 6, ...]\n```",
        "cumtrapz" | "cumulative_trapz" => "`cumtrapz` cumulative trapezoidal integration. Returns running integral array.\n\n```perl\nmy @y = (1, 2, 3, 4)\nmy @F = @{cumtrapz(\\@y)}  # [0, 1.5, 4.0, 7.5]\n```",

        // ── Optimization ─────────────────────────────────────────────────
        "bisection" | "bisect" => "`bisection` (alias `bisect`) finds a root of f(x)=0 in [a,b] via the bisection method. Takes a code ref, a, b, and optional tolerance.\n\n```perl\nmy $root = bisect(sub { $_[0]**2 - 2 }, 1, 2)  # √2 ≈ 1.4142\n```",
        "newton_method" | "newton" | "newton_raphson" => "`newton_method` (aliases `newton`, `newton_raphson`) finds a root via Newton-Raphson. Takes f, f', x0, and optional tolerance.\n\n```perl\nmy $root = newton(sub { $_[0]**2 - 2 }, sub { 2*$_[0] }, 1.5)  # √2\n```",
        "golden_section" | "golden" | "gss" => "`golden_section` (aliases `golden`, `gss`) finds the minimum of f on [a,b] via golden-section search.\n\n```perl\nmy $xmin = golden(sub { ($_[0]-3)**2 }, 0, 10)  # 3.0\n```",

        // ── ODE Solvers ──────────────────────────────────────────────────
        "rk4" | "runge_kutta" | "rk4_ode" => "`rk4` (aliases `runge_kutta`, `rk4_ode`) solves an ODE dy/dt = f(t,y) using 4th-order Runge-Kutta. Returns [[t,y], ...].\n\n```perl\n# dy/dt = -y, y(0) = 1 → y = e^(-t)\nmy $sol = rk4(sub { -$_[1] }, 0, 1, 0.1, 100)\n```",
        "euler_ode" | "euler_method" => "`euler_ode` (alias `euler_method`) solves an ODE dy/dt = f(t,y) using the Euler method. Returns [[t,y], ...].\n\n```perl\nmy $sol = euler_ode(sub { $_[1] }, 0, 1, 0.01, 100)  # exponential growth\n```",

        // ── Graph Algorithms ─────────────────────────────────────────────
        "dijkstra" | "shortest_path" => "`dijkstra` (alias `shortest_path`) computes shortest paths from a source node. Graph is a hash of {node => [[neighbor, weight], ...]}. Returns {node => distance}.\n\n```perl\nmy $g = {A => [[\"B\",1],[\"C\",4]], B => [[\"C\",2]], C => []}\nmy $d = dijkstra($g, \"A\")  # {A=>0, B=>1, C=>3}\n```",
        "bellman_ford" | "bellmanford" => "`bellman_ford` (alias `bellmanford`) computes shortest paths from a source in a graph with negative weights. Takes edges [[u,v,w],...], node count, source index.\n\n```perl\nmy $d = bellmanford([[0,1,4],[0,2,5],[1,2,-3]], 3, 0)\n```",
        "floyd_warshall" | "floydwarshall" | "apsp" => "`floyd_warshall` (aliases `floydwarshall`, `apsp`) computes all-pairs shortest paths. Takes a distance matrix (use Inf for no edge).\n\n```perl\nmy $d = apsp([[0,3,1e18],[1e18,0,1],[1e18,1e18,0]])\n```",
        "prim_mst" | "mst" | "prim" => "`prim_mst` (aliases `mst`, `prim`) computes the minimum spanning tree weight via Prim's algorithm. Takes an adjacency matrix.\n\n```perl\np mst([[0,2,0],[2,0,3],[0,3,0]])  # 5\n```",

        // ── Trig Extensions ──────────────────────────────────────────────
        "cot" => "`cot` returns the cotangent (1/tan) of an angle in radians.\n\n```perl\np cot(0.7854)  # ≈ 1.0 (cot 45°)\n```",
        "sec" => "`sec` returns the secant (1/cos) of an angle in radians.\n\n```perl\np sec(0)  # 1.0\n```",
        "csc" => "`csc` returns the cosecant (1/sin) of an angle in radians.\n\n```perl\np csc(1.5708)  # ≈ 1.0 (csc 90°)\n```",
        "sinc" => "`sinc` returns the unnormalized sinc function: sin(x)/x, with sinc(0)=1.\n\n```perl\np sinc(0)       # 1.0\np sinc(3.14159) # ≈ 0\n```",

        // ── ML Activation Functions ──────────────────────────────────────
        "leaky_relu" | "lrelu" => "`leaky_relu` (alias `lrelu`) applies Leaky ReLU: x if x≥0, alpha*x otherwise (default alpha=0.01).\n\n```perl\np lrelu(5)     # 5\np lrelu(-5)    # -0.05\np lrelu(-5, 0.1)  # -0.5\n```",
        "elu" => "`elu` applies the Exponential Linear Unit: x if x≥0, alpha*(e^x - 1) otherwise.\n\n```perl\np elu(1)     # 1\np elu(-1)    # -0.632\n```",
        "selu" => "`selu` applies the Scaled ELU with fixed lambda=1.0507, alpha=1.6733 for self-normalizing networks.\n\n```perl\np selu(1)    # 1.0507\np selu(-1)   # -1.1113\n```",
        "gelu" => "`gelu` applies the Gaussian Error Linear Unit, used in BERT/GPT transformers.\n\n```perl\np gelu(1)    # 0.8412\np gelu(-1)   # -0.1588\n```",
        "silu" | "swish" => "`silu` (alias `swish`) applies SiLU/Swish: x·sigmoid(x). Smooth approximation to ReLU.\n\n```perl\np silu(1)    # 0.7311\np swish(-2)  # -0.2384\n```",
        "mish" => "`mish` applies the Mish activation: x·tanh(softplus(x)). Often outperforms ReLU/Swish.\n\n```perl\np mish(1)    # 0.8651\np mish(-1)   # -0.3034\n```",
        "softplus" => "`softplus` applies the Softplus activation: ln(1 + e^x). Smooth approximation to ReLU.\n\n```perl\np softplus(0)   # 0.6931 (ln 2)\np softplus(10)  # ≈ 10\n```",

        // ── Special Functions ────────────────────────────────────────────
        "bessel_j0" | "j0" => "`bessel_j0` (alias `j0`) computes the Bessel function of the first kind, order 0.\n\n```perl\np j0(0)    # 1.0\np j0(2.4)  # ≈ 0 (first zero)\n```",
        "bessel_j1" | "j1" => "`bessel_j1` (alias `j1`) computes the Bessel function of the first kind, order 1.\n\n```perl\np j1(0)    # 0.0\np j1(1.84) # ≈ 0.582 (first max)\n```",
        "lambert_w" | "lambertw" | "productlog" => "`lambert_w` (aliases `lambertw`, `productlog`) computes the Lambert W function (principal branch): the inverse of f(w) = w·e^w.\n\n```perl\np lambertw(1)   # 0.5671 (Omega constant)\np lambertw(exp(1))  # 1.0\n```",

        // ── Number Theory ────────────────────────────────────────────────
        "mod_exp" | "modexp" | "powmod" => "`mod_exp` (aliases `modexp`, `powmod`) computes modular exponentiation: base^exp mod m, using fast binary exponentiation.\n\n```perl\np powmod(2, 10, 1000)  # 24 (2^10 mod 1000)\np powmod(3, 13, 50)    # 7\n```",
        "mod_inv" | "modinv" => "`mod_inv` (alias `modinv`) computes the modular multiplicative inverse via extended Euclidean algorithm. Errors if no inverse exists.\n\n```perl\np modinv(3, 7)   # 5 (3*5 ≡ 1 mod 7)\n```",
        "chinese_remainder" | "crt" => "`chinese_remainder` (alias `crt`) solves a system of simultaneous congruences via the Chinese Remainder Theorem.\n\n```perl\np crt([2,3,2], [3,5,7])  # 23 (x≡2 mod 3, x≡3 mod 5, x≡2 mod 7)\n```",
        "miller_rabin" | "millerrabin" | "is_probable_prime" => "`miller_rabin` (aliases `millerrabin`, `is_probable_prime`) performs a probabilistic primality test with k rounds (default 20).\n\n```perl\np millerrabin(104729)     # 1 (prime)\np millerrabin(104730)     # 0 (composite)\n```",

        // ── Combinatorics ────────────────────────────────────────────────
        "derangements" => "`derangements` counts the number of derangements (subfactorial !n) — permutations with no fixed points.\n\n```perl\np derangements(4)   # 9\np derangements(5)   # 44\n```",
        "stirling2" | "stirling_second" => "`stirling2` (alias `stirling_second`) computes the Stirling number of the second kind S(n,k) — the number of ways to partition n elements into k non-empty subsets.\n\n```perl\np stirling2(4, 2)   # 7\np stirling2(5, 3)   # 25\n```",
        "bernoulli_number" | "bernoulli" => "`bernoulli_number` (alias `bernoulli`) computes the nth Bernoulli number. B(0)=1, B(1)=-0.5, odd B(n>1)=0.\n\n```perl\np bernoulli(0)   # 1\np bernoulli(2)   # 0.1667\np bernoulli(4)   # -0.0333\n```",
        "harmonic_number" | "harmonic" => "`harmonic_number` (alias `harmonic`) computes the nth harmonic number H_n = 1 + 1/2 + 1/3 + ... + 1/n.\n\n```perl\np harmonic(1)    # 1.0\np harmonic(10)   # 2.9290\np harmonic(100)  # 5.1874\n```",

        // ── Physics ──────────────────────────────────────────────────────
        "drag_force" | "fdrag" => "`drag_force` (alias `fdrag`) computes aerodynamic drag: F = 0.5·Cd·ρ·A·v². Args: drag_coeff, air_density, area, velocity.\n\n```perl\np fdrag(0.47, 1.225, 0.01, 30)  # drag on a ball at 30 m/s\n```",
        "ideal_gas" | "pv_nrt" => "`ideal_gas` (alias `pv_nrt`) solves PV=nRT for the unknown (pass 0 for the value to solve). Args: P, V, n, T.\n\n```perl\np pv_nrt(0, 0.0224, 1, 273.15)  # pressure at STP\np pv_nrt(101325, 0, 1, 273.15)   # volume at STP\n```",

        // ── Financial Greeks ─────────────────────────────────────────────
        "bs_delta" | "bsdelta" | "option_delta" => "`bs_delta` (aliases `bsdelta`, `option_delta`) computes the Black-Scholes delta (∂C/∂S). Args: S, K, T, r, sigma.\n\n```perl\np bsdelta(100, 100, 1, 0.05, 0.2)  # ≈ 0.64\n```",
        "bs_gamma" | "bsgamma" | "option_gamma" => "`bs_gamma` computes the Black-Scholes gamma (∂²C/∂S²). Measures convexity of option value.\n\n```perl\np bsgamma(100, 100, 1, 0.05, 0.2)  # ≈ 0.019\n```",
        "bs_vega" | "bsvega" | "option_vega" => "`bs_vega` computes the Black-Scholes vega (∂C/∂σ). Sensitivity to volatility.\n\n```perl\np bsvega(100, 100, 1, 0.05, 0.2)  # ≈ 37.5\n```",
        "bs_theta" | "bstheta" | "option_theta" => "`bs_theta` computes the Black-Scholes theta (∂C/∂t). Time decay per unit time.\n\n```perl\np bstheta(100, 100, 1, 0.05, 0.2)  # negative (time decay)\n```",
        "bs_rho" | "bsrho" | "option_rho" => "`bs_rho` computes the Black-Scholes rho (∂C/∂r). Sensitivity to interest rate.\n\n```perl\np bsrho(100, 100, 1, 0.05, 0.2)  # ≈ 46\n```",
        "mac_duration" => "`bond_duration` (alias `mac_duration`) computes Macaulay duration — the weighted-average time to receive cash flows.\n\n```perl\nmy $dur = bond_duration([5,5,5,105], 0.05)  # ≈ 3.72 years\n```",

        // ── DSP ──────────────────────────────────────────────────────────
        "dct" => "`dct` computes the Type-II Discrete Cosine Transform of a signal. Used in JPEG, MP3, and speech processing.\n\n```perl\nmy @coeffs = @{dct([1,2,3,4])}\n```",
        "idct" => "`idct` computes the inverse DCT (Type-III). Reconstructs a signal from DCT coefficients.\n\n```perl\nmy @signal = @{idct(dct([1,2,3,4]))}\n```",
        "goertzel" => "`goertzel` computes the magnitude of a single DFT frequency bin using the Goertzel algorithm. Much faster than full FFT when you need one frequency.\n\n```perl\nmy $mag = goertzel(\\@signal, 440, 44100)  # 440 Hz component\n```",
        "chirp" | "chirp_signal" => "`chirp` generates a linear chirp signal sweeping from f0 to f1 Hz. Args: n_samples, f0, f1, sample_rate.\n\n```perl\nmy @sig = @{chirp(1000, 100, 1000, 8000)}\n```",

        // ── Encoding ─────────────────────────────────────────────────────
        "base85_encode" | "b85e" | "ascii85_encode" | "a85e" => "`base85_encode` (aliases `b85e`, `a85e`) encodes a string using Ascii85/Base85 encoding. More compact than Base64 (4:5 ratio vs 3:4).\n\n```perl\np b85e(\"Hello\")  # encoded string\n```",
        "base85_decode" | "b85d" | "ascii85_decode" | "a85d" => "`base85_decode` (aliases `b85d`, `a85d`) decodes an Ascii85/Base85 encoded string.\n\n```perl\np b85d(b85e(\"Hello\"))  # Hello\n```",

        // ── R base: distribution CDFs & quantiles ────────────────────────
        "pnorm" => "`pnorm` — normal CDF. P(X ≤ x) for N(mu, sigma). Args: x [, mu, sigma]. Default standard normal.\n\n```perl\np pnorm(0)          # 0.5\np pnorm(1.96)       # 0.975\np pnorm(100, 90, 15) # z-score for IQ 100\n```",
        "qnorm" => "`qnorm` — normal quantile (inverse CDF). Returns x such that P(X ≤ x) = p. Args: p [, mu, sigma].\n\n```perl\np qnorm(0.975)      # 1.96\np qnorm(0.5)        # 0.0\np qnorm(0.95, 100, 15)  # 90th percentile IQ\n```",
        "pbinom" => "`pbinom` — binomial CDF. P(X ≤ k) for Binom(n, p). Args: k, n, p.\n\n```perl\np pbinom(5, 10, 0.5)   # P(≤5 heads in 10 flips)\n```",
        "dbinom" => "`dbinom` — binomial PMF. P(X = k) for Binom(n, p). Args: k, n, p.\n\n```perl\np dbinom(5, 10, 0.5)   # P(exactly 5 heads in 10 flips)\n```",
        "ppois" => "`ppois` — Poisson CDF. P(X ≤ k) for Poisson(lambda). Args: k, lambda.\n\n```perl\np ppois(3, 2.5)  # P(≤3 events when avg is 2.5)\n```",
        "punif" => "`punif` — uniform CDF. P(X ≤ x) for Uniform(a, b). Args: x, a, b.\n\n```perl\np punif(0.5, 0, 1)  # 0.5\np punif(3, 0, 10)   # 0.3\n```",
        "pexp" => "`pexp` — exponential CDF. P(X ≤ x) for Exp(rate). Args: x, rate.\n\n```perl\np pexp(1, 1)     # 0.6321 (1 - e^-1)\np pexp(5, 0.5)   # P(wait ≤ 5 with avg wait 2)\n```",
        "pweibull" => "`pweibull` — Weibull CDF. P(X ≤ x) for Weibull(shape, scale). Args: x, shape, scale.\n\n```perl\np pweibull(1, 1, 1)  # same as exponential\n```",
        "plnorm" => "`plnorm` — log-normal CDF. P(X ≤ x) for LogN(mu, sigma). Args: x, mu, sigma.\n\n```perl\np plnorm(1, 0, 1)   # 0.5 (median of LogN(0,1))\n```",
        "pcauchy" => "`pcauchy` — Cauchy CDF. P(X ≤ x). Args: x [, location, scale].\n\n```perl\np pcauchy(0, 0, 1)  # 0.5\n```",

        // ── R base: matrix ops ───────────────────────────────────────────
        "rbind" => "`rbind` — bind matrices/vectors by rows (vertical stack). Like R's rbind().\n\n```perl\nmy $m = rbind([1,2,3], [4,5,6])  # [[1,2,3],[4,5,6]]\n```",
        "cbind" => "`cbind` — bind matrices by columns (horizontal join). Like R's cbind().\n\n```perl\nmy $m = cbind([[1],[2]], [[3],[4]])  # [[1,3],[2,4]]\n```",
        "row_sums" | "rowSums" => "`row_sums` (alias `rowSums`) — sum of each row in a matrix. Like R's rowSums().\n\n```perl\np row_sums([[1,2,3],[4,5,6]])  # [6, 15]\n```",
        "col_sums" | "colSums" => "`col_sums` (alias `colSums`) — sum of each column. Like R's colSums().\n\n```perl\np colSums([[1,2],[3,4]])  # [4, 6]\n```",
        "row_means" | "rowMeans" => "`row_means` (alias `rowMeans`) — mean of each row. Like R's rowMeans().\n\n```perl\np rowMeans([[2,4],[6,8]])  # [3, 7]\n```",
        "col_means" | "colMeans" => "`col_means` (alias `colMeans`) — mean of each column. Like R's colMeans().\n\n```perl\np colMeans([[2,4],[6,8]])  # [4, 6]\n```",
        "outer_product" | "outer" => "`outer` — outer product of two vectors. Returns matrix where M[i][j] = v1[i] * v2[j].\n\n```perl\nmy $m = outer([1,2,3], [10,20])  # [[10,20],[20,40],[30,60]]\n```",
        "crossprod" => "`crossprod` — cross product t(M) * M (R's crossprod). Efficiently computes M^T M without explicit transpose.\n\n```perl\nmy $ata = crossprod([[1,2],[3,4]])  # [[10,14],[14,20]]\n```",
        "tcrossprod" => "`tcrossprod` — M * t(M) (R's tcrossprod).\n\n```perl\nmy $aat = tcrossprod([[1,2],[3,4]])  # [[5,11],[11,25]]\n```",
        "nrow" => "`nrow` — number of rows in a matrix.\n\n```perl\np nrow([[1,2],[3,4],[5,6]])  # 3\n```",
        "ncol" => "`ncol` — number of columns in a matrix.\n\n```perl\np ncol([[1,2,3],[4,5,6]])  # 3\n```",
        "prop_table" | "proptable" => "`prop_table` — convert a count matrix to proportions (each cell / total). Like R's prop.table().\n\n```perl\np prop_table([[10,20],[30,40]])  # [[0.1,0.2],[0.3,0.4]]\n```",

        // ── R base: vector ops ───────────────────────────────────────────
        "cummax" => "`cummax` — cumulative maximum. Each element is the max of all elements up to that position.\n\n```perl\np cummax([3,1,4,1,5,9])  # [3,3,4,4,5,9]\n```",
        "cummin" => "`cummin` — cumulative minimum.\n\n```perl\np cummin([9,5,1,4,3])  # [9,5,1,1,1]\n```",
        "scale_vec" | "scale" => "`scale` — standardize a vector: (x - mean) / sd. Like R's scale().\n\n```perl\nmy @z = @{scale([10,20,30])}  # [-1, 0, 1]\n```",
        "which_fn" => "`which_fn` — return indices where a predicate is true. Like R's which().\n\n```perl\nmy @idx = @{which_fn([1,5,3,8,2], sub { $_[0] > 3 })}  # [1, 3]\n```",
        "tabulate" => "`tabulate` — frequency table of values. Returns hash of value => count. Like R's table().\n\n```perl\nmy %t = %{tabulate([qw(a b a c b a)])}\np $t{a}  # 3\n```",
        "duplicated" | "duped" => "`duplicated` — boolean array: 1 if element appeared earlier in the vector, 0 otherwise. Like R's duplicated().\n\n```perl\np duplicated([1,2,3,2,1])  # [0,0,0,1,1]\n```",
        "seq_fn" => "`seq_fn` — generate a numeric sequence from, to, by. Like R's seq().\n\n```perl\nmy @s = @{seq_fn(1, 10, 2)}  # [1,3,5,7,9]\nmy @r = @{seq_fn(5, 1, -1)}  # [5,4,3,2,1]\n```",
        "rep_fn" => "`rep_fn` — repeat a value N times. Like R's rep().\n\n```perl\nmy @r = @{rep_fn(0, 10)}   # ten zeros\nmy @s = @{rep_fn(\"x\", 5)}  # five \"x\"s\n```",
        "cut_bins" | "cut" => "`cut` — bin continuous values into intervals. Returns integer bin indices. Like R's cut().\n\n```perl\np cut([1.5, 3.2, 7.8], [0, 2, 5, 10])  # [1, 2, 3]\n```",
        "find_interval" | "findInterval" => "`find_interval` — for each x, find which interval in breaks it falls into. Like R's findInterval().\n\n```perl\np findInterval([1.5, 3.5, 7.5], [0, 2, 5, 10])  # [1, 2, 3]\n```",
        "ecdf_fn" | "ecdf" => "`ecdf` — empirical CDF: proportion of data ≤ x. Like R's ecdf().\n\n```perl\np ecdf([1,2,3,4,5], 3)  # 0.6 (3 of 5 values ≤ 3)\n```",
        "density_est" | "density" => "`density` — kernel density estimation with Gaussian kernel and Silverman bandwidth. Returns [[x_grid], [density_values]]. Like R's density().\n\n```perl\nmy ($x, $y) = @{density([1,2,2,3,3,3,4,4,5])}\n# $x is grid of 512 points, $y is estimated density\n```",
        "embed_ts" | "embed" => "`embed` — time-delay embedding. Converts a time series into a matrix of lagged values. Like R's embed().\n\n```perl\np embed([1,2,3,4,5], 3)  # [[3,2,1],[4,3,2],[5,4,3]]\n```",

        // ── R base: stats tests ──────────────────────────────────────────
        "shapiro_test" | "shapiro" => "`shapiro_test` (alias `shapiro`) — Shapiro-Wilk normality test. Returns W statistic (close to 1 = normal). Like R's shapiro.test().\n\n```perl\np shapiro([rnorm(100)])  # W ≈ 0.99 for normal data\np shapiro([1,1,1,2,10]) # W < 0.9 for non-normal\n```",
        "ks_test" | "ks" => "`ks_test` (alias `ks`) — two-sample Kolmogorov-Smirnov test. Returns D statistic (max CDF difference). Like R's ks.test().\n\n```perl\nmy $D = ks([1,2,3,4,5], [2,3,4,5,6])  # small D = similar\n```",
        "wilcox_test" | "wilcox" | "mann_whitney" => "`wilcox_test` (aliases `wilcox`, `mann_whitney`) — Wilcoxon rank-sum test (Mann-Whitney U). Returns U statistic. Like R's wilcox.test().\n\n```perl\nmy $U = wilcox([1,2,3], [4,5,6])  # 0 (no overlap)\n```",
        "prop_test" | "proptest" => "`prop_test` — one-sample proportion z-test. Returns [z-statistic, p-value]. Like R's prop.test().\n\n```perl\nmy ($z, $pval) = @{proptest(55, 100, 0.5)}  # test if 55/100 differs from 50%%\n```",
        "binom_test" | "binomtest" => "`binom_test` — exact binomial test. Returns two-sided p-value. Like R's binom.test().\n\n```perl\nmy $p = binomtest(7, 10, 0.5)  # p-value for 7/10 successes vs p=0.5\n```",

        // ── R base: apply family ─────────────────────────────────────────
        "sapply" => "`sapply` — apply a function to each element, return a vector. Like R's sapply().\n\n```perl\nmy @sq = @{sapply([1,2,3,4], sub { $_[0] ** 2 })}  # [1,4,9,16]\n```",
        "tapply" => "`tapply` — apply a function by group. Takes data, group labels, and function. Returns hash. Like R's tapply().\n\n```perl\nmy %means = %{tapply([1,2,3,4], [\"a\",\"a\",\"b\",\"b\"], sub { avg(@{$_[0]}) })}\np $means{a}  # 1.5\np $means{b}  # 3.5\n```",
        "do_call" | "docall" => "`do_call` — call a function with args from a list. Like R's do.call().\n\n```perl\nmy $result = docall(sub { $_[0] + $_[1] }, [3, 4])  # 7\n```",

        // ── R base: ML / clustering ──────────────────────────────────────
        "kmeans" => "`kmeans` — k-means clustering (Lloyd's algorithm). Takes array of points and k. Returns cluster assignments. Like R's kmeans().\n\n```perl\nmy @clusters = @{kmeans([[0,0],[1,0],[10,10],[11,10]], 2)}\n# [0,0,1,1] — two clusters\n```",
        "prcomp" | "pca" => "`prcomp` (alias `pca`) — Principal Component Analysis via eigendecomposition of covariance matrix. Returns eigenvalues (variance explained). Like R's prcomp().\n\n```perl\nmy @var = @{pca([[1,2],[3,4],[5,6],[7,8]])}\n# variance explained by each component\n```",

        // ── R base: random generators ────────────────────────────────────
        "rnorm" => "`rnorm` — generate n random normal variates. Args: n [, mu, sigma]. Like R's rnorm().\n\n```perl\nmy @x = @{rnorm(1000)}          # 1000 standard normal\nmy @y = @{rnorm(100, 50, 10)}   # mean=50, sd=10\n```",
        "runif" => "`runif` — generate n random uniform variates. Args: n [, min, max]. Like R's runif().\n\n```perl\nmy @x = @{runif(100, 0, 1)}     # 100 uniform [0,1]\n```",
        "rexp" => "`rexp` — generate n random exponential variates. Args: n [, rate]. Like R's rexp().\n\n```perl\nmy @x = @{rexp(100, 0.5)}       # rate=0.5 (mean=2)\n```",
        "rbinom" => "`rbinom` — generate n random binomial variates. Args: n, size, prob. Like R's rbinom().\n\n```perl\nmy @x = @{rbinom(100, 10, 0.3)} # 100 draws from Binom(10,0.3)\n```",
        "rpois" => "`rpois` — generate n random Poisson variates. Args: n, lambda. Like R's rpois().\n\n```perl\nmy @x = @{rpois(100, 5)}        # Poisson with mean 5\n```",
        "rgeom" => "`rgeom` — generate n random geometric variates. Args: n, prob.\n\n```perl\nmy @x = @{rgeom(100, 0.3)}      # trials until first success\n```",
        "rgamma" => "`rgamma` — generate n random gamma variates. Args: n, shape [, scale]. Like R's rgamma().\n\n```perl\nmy @x = @{rgamma(100, 2, 1)}    # Gamma(2,1)\n```",
        "rbeta" => "`rbeta` — generate n random beta variates. Args: n, alpha, beta. Like R's rbeta().\n\n```perl\nmy @x = @{rbeta(100, 2, 5)}     # Beta(2,5)\n```",
        "rchisq" => "`rchisq` — generate n random chi-squared variates. Args: n, df. Like R's rchisq().\n\n```perl\nmy @x = @{rchisq(100, 5)}       # Chi-sq with 5 df\n```",
        "rt" => "`rt` — generate n random Student's t variates. Args: n, df. Like R's rt().\n\n```perl\nmy @x = @{rt(100, 10)}          # t with 10 df\n```",
        "rf" => "`rf` — generate n random F variates. Args: n, d1, d2. Like R's rf().\n\n```perl\nmy @x = @{rf(100, 5, 10)}       # F(5,10)\n```",
        "rweibull" => "`rweibull` — generate n random Weibull variates. Args: n, shape [, scale].\n\n```perl\nmy @x = @{rweibull(100, 2, 1)}  # Weibull(2,1)\n```",
        "rlnorm" => "`rlnorm` — generate n random log-normal variates. Args: n [, mu, sigma].\n\n```perl\nmy @x = @{rlnorm(100, 0, 1)}    # LogN(0,1)\n```",
        "rcauchy" => "`rcauchy` — generate n random Cauchy variates. Args: n [, location, scale].\n\n```perl\nmy @x = @{rcauchy(100, 0, 1)}   # standard Cauchy\n```",

        // ── R base: quantile functions ───────────────────────────────────
        "qunif" => "`qunif` — uniform quantile. Args: p [, min, max].\n\n```perl\np qunif(0.5, 0, 10)  # 5.0 (median)\n```",
        "qexp" => "`qexp` — exponential quantile. Args: p [, rate].\n\n```perl\np qexp(0.5, 1)       # 0.693 (median of Exp(1))\n```",
        "qweibull" => "`qweibull` — Weibull quantile. Args: p, shape [, scale].\n\n```perl\np qweibull(0.5, 1, 1)  # same as qexp\n```",
        "qlnorm" => "`qlnorm` — log-normal quantile. Args: p [, mu, sigma].\n\n```perl\np qlnorm(0.5, 0, 1)  # 1.0 (median of LogN(0,1))\n```",
        "qcauchy" => "`qcauchy` — Cauchy quantile. Args: p [, location, scale].\n\n```perl\np qcauchy(0.75, 0, 1)  # 1.0\n```",

        // ── R base: additional CDFs & PMFs ───────────────────────────────
        "pgamma" => "`pgamma` — gamma CDF. Args: x, shape [, scale].\n\n```perl\np pgamma(2, 2, 1)  # P(X ≤ 2) for Gamma(2,1)\n```",
        "pbeta" => "`pbeta` — beta CDF (regularized incomplete beta). Args: x, a, b.\n\n```perl\np pbeta(0.5, 2, 5)  # P(X ≤ 0.5) for Beta(2,5)\n```",
        "pchisq" => "`pchisq` — chi-squared CDF. Args: x, df.\n\n```perl\np pchisq(3.84, 1)  # 0.95 (critical value for p=0.05)\n```",
        "pt_cdf" | "pt" => "`pt` — Student's t CDF. Args: x, df.\n\n```perl\np pt(1.96, 100)  # ≈ 0.975\n```",
        "pf_cdf" | "pf" => "`pf` — F-distribution CDF. Args: x, d1, d2.\n\n```perl\np pf(4.0, 5, 10)  # P(F ≤ 4) for F(5,10)\n```",
        "dgeom" => "`dgeom` — geometric PMF P(X=k). Args: k, prob.\n\n```perl\np dgeom(3, 0.5)  # P(first success on 4th trial)\n```",
        "dunif" => "`dunif` — uniform PDF. Args: x, a, b.\n\n```perl\np dunif(0.5, 0, 1)  # 1.0\n```",
        "dnbinom" => "`dnbinom` — negative binomial PMF. Args: k, size, prob.\n\n```perl\np dnbinom(3, 5, 0.5)  # P(3 failures before 5 successes)\n```",
        "dhyper" => "`dhyper` — hypergeometric PMF. Args: k, m (white), n (black), nn (draws).\n\n```perl\np dhyper(2, 5, 5, 3)  # P(2 white balls in 3 draws from 5W+5B)\n```",

        // ── R base: smoothing & linear models ───────────────────────────
        "lowess" | "loess" => "`lowess` (alias `loess`) — locally-weighted scatterplot smoothing (LOWESS/LOESS). Returns smoothed Y values with tricube weighting.\n\n```perl\nmy @smooth = @{lowess([1,2,3,4,5], [2,1,4,3,5])}\n```",
        "approx_fn" | "approx" => "`approx_fn` (alias `approx`) — piecewise linear interpolation at query points. Like R's approx().\n\n```perl\nmy @y = @{approx([0,1,2], [0,10,0], [0.5, 1.0, 1.5])}\n# [5, 10, 5]\n```",
        "lm_fit" | "lm" => "`lm_fit` (alias `lm`) — simple linear regression. Returns hash with intercept, slope, r_squared, residuals, fitted. Like R's lm().\n\n```perl\nmy %m = %{lm([1,2,3,4,5], [2,4,5,4,5])}\np $m{slope}       # slope\np $m{r_squared}   # R²\n```",

        // ── R base: remaining quantiles ──────────────────────────────────
        "qgamma" => "`qgamma` — gamma quantile (inverse CDF). Args: p, shape [, scale].\n\n```perl\np qgamma(0.95, 2, 1)  # 95th percentile of Gamma(2,1)\n```",
        "qbeta" => "`qbeta` — beta quantile. Args: p, alpha, beta.\n\n```perl\np qbeta(0.5, 2, 5)  # median of Beta(2,5)\n```",
        "qchisq" => "`qchisq` — chi-squared quantile. Args: p, df.\n\n```perl\np qchisq(0.95, 1)  # 3.84 (critical value)\np qchisq(0.95, 5)  # 11.07\n```",
        "qt_fn" | "qt" => "`qt` — Student's t quantile. Args: p, df.\n\n```perl\np qt(0.975, 10)   # ≈ 2.228 (two-tailed 5%)\n```",
        "qf_fn" | "qf" => "`qf` — F-distribution quantile. Args: p, d1, d2.\n\n```perl\np qf(0.95, 5, 10)  # critical F value\n```",
        "qbinom" => "`qbinom` — binomial quantile (smallest k where P(X≤k) ≥ p). Args: p, n, prob.\n\n```perl\np qbinom(0.5, 10, 0.5)  # median of Binom(10, 0.5) = 5\n```",
        "qpois" => "`qpois` — Poisson quantile (smallest k where P(X≤k) ≥ p). Args: p, lambda.\n\n```perl\np qpois(0.5, 5)  # median of Poisson(5)\n```",

        // ── R base: time series ──────────────────────────────────────────
        "acf_fn" | "acf" => "`acf_fn` (alias `acf`) — autocorrelation function. Returns ACF values for lags 0..max_lag. Like R's acf().\n\n```perl\nmy @a = @{acf([1,3,2,4,3,5,4,6], 5)}\np $a[0]  # 1.0 (lag 0 is always 1)\np $a[1]  # lag-1 autocorrelation\n```",
        "pacf_fn" | "pacf" => "`pacf_fn` (alias `pacf`) — partial autocorrelation function via Durbin-Levinson. Like R's pacf().\n\n```perl\nmy @pa = @{pacf([1,3,2,4,3,5], 3)}\n```",
        "diff_lag" | "diff_ts" => "`diff_lag` (alias `diff_ts`) — lagged differences. Args: vec [, lag, differences]. Like R's diff().\n\n```perl\np diff_lag([1,3,6,10])       # [2,3,4] (lag=1)\np diff_lag([1,3,6,10], 2)    # [5,7] (lag=2)\np diff_lag([1,3,6,10], 1, 2) # [1,1] (second differences)\n```",
        "ts_filter" | "filter_ts" => "`ts_filter` — linear convolution filter. Like R's filter(method='convolution').\n\n```perl\nmy @smooth = @{ts_filter([1,5,2,8,3], [0.25,0.5,0.25])}\n```",

        // ── R base: regression diagnostics ───────────────────────────────
        "predict_lm" | "predict" => "`predict_lm` (alias `predict`) — predict from a linear model at new x values.\n\n```perl\nmy $model = lm([1,2,3], [2,4,6])\nmy @pred = @{predict($model, [4,5,6])}  # [8,10,12]\n```",
        "confint_lm" | "confint" => "`confint_lm` (alias `confint`) — confidence intervals for model coefficients. Returns hash with intercept_lower/upper, slope_lower/upper.\n\n```perl\nmy $ci = confint(lm([1,2,3,4,5], [2,4,5,4,5]))\np $ci->{slope_lower}\n```",

        // ── R base: multivariate stats ───────────────────────────────────
        "cor_matrix" | "cor_mat" => "`cor_matrix` (alias `cor_mat`) — correlation matrix from observations. Each row is an observation vector.\n\n```perl\nmy $R = cor_mat([[1,2],[3,4],[5,6]])  # 2x2 correlation matrix\n```",
        "cov_matrix" | "cov_mat" => "`cov_matrix` (alias `cov_mat`) — covariance matrix from observations.\n\n```perl\nmy $S = cov_mat([[1,2],[3,4],[5,6]])\n```",
        "mahalanobis" | "mahal" => "`mahalanobis` — Mahalanobis distance. Args: data, center, inverse_covariance. Like R's mahalanobis().\n\n```perl\nmy @d = @{mahal([[1,2],[3,4]], [2,3], [[1,0],[0,1]])}\n```",
        "dist_matrix" | "dist_mat" => "`dist_matrix` (alias `dist_mat`) — pairwise distance matrix. Supports 'euclidean' (default), 'manhattan', 'maximum'. Like R's dist().\n\n```perl\nmy $D = dist_mat([[0,0],[1,0],[0,1]])  # 3x3 distance matrix\nmy $D2 = dist_mat([[0,0],[1,1]], \"manhattan\")  # Manhattan\n```",
        "hclust" => "`hclust` — hierarchical clustering (average linkage). Takes a distance matrix. Returns merge list [[i,j,height],...]. Like R's hclust().\n\n```perl\nmy $merges = hclust(dist_mat([[0,0],[1,0],[10,10]]))\n```",
        "cutree" => "`cutree` — cut a dendrogram into k clusters. Takes merge list from hclust and k. Returns cluster assignments. Like R's cutree().\n\n```perl\nmy @clusters = @{cutree(hclust(dist_mat($data)), 3)}\n```",
        "weighted_var" | "wvar" => "`weighted_var` — weighted variance. Args: values, weights.\n\n```perl\np wvar([1,2,3,4], [1,1,1,1])  # same as var\n```",
        "cov2cor" => "`cov2cor` — convert covariance matrix to correlation matrix. Like R's cov2cor().\n\n```perl\nmy $cor = cov2cor(cov_mat($data))\n```",

        // ── SVG Plotting ─────────────────────────────────────────────────
        "scatter_svg" | "scatter_plot" => "`scatter_svg` (alias `scatter_plot`) — generate an SVG scatter plot. Args: xs, ys [, title]. Dark theme, auto-scaled axes.\n\n```perl\nscatter_svg([1,2,3,4], [1,4,9,16], \"Squares\") |> to_file(\"scatter.svg\")\n```",
        "line_svg" | "line_plot" => "`line_svg` (alias `line_plot`) — generate an SVG line plot. Args: xs, ys [, title].\n\n```perl\nmy @x = map { $_ * 0.1 } 0..100\nmy @y = map { sin($_) } @x\nline_svg(\\@x, \\@y, \"Sine\") |> to_file(\"sine.svg\")\n```",
        "plot_svg" => "`plot_svg` — SVG line plot with auto X axis (0..n-1). Args: ys [, title].\n\n```perl\nplot_svg([map { $_ ** 2 } 0..50], \"Parabola\") |> to_file(\"plot.svg\")\n```",
        "hist_svg" | "histogram_svg" => "`hist_svg` (alias `histogram_svg`) — SVG histogram with auto-binning (sqrt rule). Args: data [, bins, title].\n\n```perl\nrnorm(1000) |> hist_svg(30, \"Normal\") |> to_file(\"hist.svg\")\n```",
        "boxplot_svg" | "box_plot" => "`boxplot_svg` (alias `box_plot`) — SVG box-and-whisker plot with IQR whiskers and outlier detection. Args: groups [, title]. Groups is array of arrays.\n\n```perl\nboxplot_svg([[1,2,3,4,5], [3,4,5,6,20]], \"Compare\") |> to_file(\"box.svg\")\n```",
        "bar_svg" | "barchart_svg" => "`bar_svg` (alias `barchart_svg`) — SVG bar chart with labeled bars and value annotations. Args: labels, values [, title].\n\n```perl\nbar_svg([\"Rust\",\"Go\",\"C++\"], [45,30,25], \"Languages\") |> to_file(\"bar.svg\")\n```",
        "pie_svg" | "pie_chart" => "`pie_svg` (alias `pie_chart`) — SVG pie chart with percentage labels. Args: labels, values [, title].\n\n```perl\npie_svg([\"A\",\"B\",\"C\"], [50,30,20]) |> to_file(\"pie.svg\")\n```",
        "heatmap_svg" | "heatmap" => "`heatmap_svg` (alias `heatmap`) — SVG heatmap with blue-cyan-yellow-red colormap. Args: matrix [, title].\n\n```perl\nheatmap_svg(cor_mat($data), \"Correlation\") |> to_file(\"heat.svg\")\n```",

        // ── Cyberpunk Terminal Art ─────────────────────────────────────
        "cyber_city" => "`cyber_city` — procedural neon cityscape with buildings, windows, stars, and antennas. Args: [width, height, seed]. Output: ANSI-colored string for terminal.\n\n```perl\np cyber_city()              # 80x24 default\np cyber_city(120, 40, 99)   # custom size and seed\n```",
        "cyber_grid" => "`cyber_grid` — retro perspective grid with vanishing point and neon glow (Tron/synthwave style). Args: [width, height].\n\n```perl\np cyber_grid()           # 80x24 default\np cyber_grid(120, 30)    # wider grid\n```",
        "cyber_rain" | "matrix_rain" => "`cyber_rain` (alias `matrix_rain`) — matrix-style digital rain with Japanese katakana and green phosphor glow. Args: [width, height, seed].\n\n```perl\np cyber_rain()              # 80x24 default\np cyber_rain(120, 40, 42)   # custom\n```",
        "cyber_glitch" | "glitch_text" => "`cyber_glitch` (alias `glitch_text`) — glitch-distort text with ANSI corruption, screen tears, and neon color bleeding. Args: text [, intensity 1-10].\n\n```perl\np cyber_glitch(\"SYSTEM BREACH\", 7)\np cyber_glitch(\"hello world\")\n```",
        "cyber_banner" | "neon_banner" => "`cyber_banner` (alias `neon_banner`) — large neon block-letter banner with gradient coloring and border. Args: text.\n\n```perl\np cyber_banner(\"STRYKE\")\np cyber_banner(\"HACK THE PLANET\")\n```",
        "cyber_circuit" => "`cyber_circuit` — circuit board pattern with traces, intersections, and glowing nodes. Args: [width, height, seed].\n\n```perl\np cyber_circuit()             # 60x20 default\np cyber_circuit(80, 30, 42)   # custom\n```",
        "cyber_skull" => "`cyber_skull` — neon skull ASCII art with glitch effects. Args: [size]. Size: \"small\" (default) or \"large\".\n\n```perl\np cyber_skull()          # small skull\np cyber_skull(\"large\")   # large skull\n```",
        "cyber_eye" => "`cyber_eye` — cyberpunk all-seeing eye motif with layered glow. Args: [size]. Size: \"small\" (default) or \"large\".\n\n```perl\np cyber_eye()          # small eye\np cyber_eye(\"large\")   # large eye\n```",

        // ── Charts (extended) ────────────────────────────────────────────
        "donut_svg" | "donut" => "`donut_svg` (alias `donut`) — SVG donut chart with percentage labels. Accepts labels+values or a hashref.\n\n```perl\ndonut_svg([\"A\",\"B\",\"C\"], [50,30,20], \"Share\") |> to_file(\"donut.svg\")\n{Rust => 45, Go => 30} |> donut(\"Languages\") |> to_file(\"donut.svg\")\n```",
        "area_svg" | "area_chart" => "`area_svg` (alias `area_chart`) — SVG filled area chart. Args: xs, ys [, title].\n\n```perl\nmy @x = 0..20\nmy @y = map { sin($_ * 0.3) } @x\narea_svg(\\@x, \\@y, \"Wave\") |> to_file(\"area.svg\")\n```",
        "hbar_svg" | "hbar" => "`hbar_svg` (alias `hbar`) — SVG horizontal bar chart. Accepts labels+values or a hashref.\n\n```perl\nhbar_svg([\"Rust\",\"Go\",\"C++\"], [45,30,25], \"Speed\") |> to_file(\"hbar.svg\")\n{alpha => 10, beta => 20} |> hbar |> to_file(\"hbar.svg\")\n```",
        "radar_svg" | "radar" | "spider" => "`radar_svg` (aliases `radar`, `spider`) — SVG radar/spider chart. Args: labels, values [, title].\n\n```perl\nradar_svg([\"Atk\",\"Def\",\"Spd\",\"HP\",\"MP\"], [8,6,9,5,7], \"Stats\") |> to_file(\"radar.svg\")\n```",
        "candlestick_svg" | "candlestick" | "ohlc" => "`candlestick_svg` (aliases `candlestick`, `ohlc`) — SVG candlestick OHLC chart. Args: array of [open,high,low,close] [, title].\n\n```perl\ncandlestick_svg([[100,110,95,105],[105,115,100,112]], \"Stock\") |> to_file(\"ohlc.svg\")\n```",
        "violin_svg" | "violin" => "`violin_svg` (alias `violin`) — SVG violin plot from array of arrays showing distribution shape.\n\n```perl\nviolin_svg([rnorm(200), rnorm(200, 2)], \"Distributions\") |> to_file(\"violin.svg\")\n```",
        "cor_heatmap" | "cor_matrix_svg" => "`cor_heatmap` (alias `cor_matrix_svg`) — SVG correlation matrix heatmap. Computes pairwise correlations and renders as heatmap.\n\n```perl\ncor_heatmap([rnorm(100), rnorm(100), rnorm(100)], \"Correlations\") |> to_file(\"cor.svg\")\n```",
        "stacked_bar_svg" | "stacked_bar" => "`stacked_bar_svg` (alias `stacked_bar`) — SVG stacked bar chart. Args: labels, series (array of arrays) [, title].\n\n```perl\nstacked_bar_svg([\"Q1\",\"Q2\",\"Q3\"], [[10,20,30],[5,15,25]], \"Revenue\") |> to_file(\"stacked.svg\")\n```",
        "wordcloud_svg" | "wordcloud" | "wcloud" => "`wordcloud_svg` (aliases `wordcloud`, `wcloud`) — SVG word cloud from frequency hashref. Word size scales with frequency.\n\n```perl\n{rust => 50, go => 30, python => 40} |> wordcloud(\"Languages\") |> to_file(\"cloud.svg\")\n```",
        "treemap_svg" | "treemap" => "`treemap_svg` (alias `treemap`) — SVG treemap from frequency hashref. Area proportional to values.\n\n```perl\n{src => 500, tests => 200, docs => 100} |> treemap(\"Codebase\") |> to_file(\"tree.svg\")\n```",

        // ── Preview ──────────────────────────────────────────────────────
        "preview" | "pvw" => "`preview` (alias `pvw`) — wrap SVG/HTML content in a cyberpunk-styled page and open in the default browser.\n\n```perl\nscatter_svg([1,2,3], [1,4,9]) |> preview\nbar_svg([\"A\",\"B\"], [10,20]) |> pvw\n```",

        // ── Audio ────────────────────────────────────────────────────────
        "audio_convert" | "aconv" => "`audio_convert` (alias `aconv`) — convert audio files between WAV, FLAC, AIFF, and MP3 formats.\n\n```perl\naudio_convert(\"song.flac\", \"song.mp3\")\naconv(\"input.wav\", \"output.mp3\")\n```",
        "audio_info" | "ainfo" => "`audio_info` (alias `ainfo`) — get audio file metadata (duration, sample rate, channels, format) as a hashref.\n\n```perl\nmy $info = ainfo(\"song.mp3\")\np $info->{duration}      # seconds\np $info->{sample_rate}   # e.g. 44100\n```",
        "id3_read" | "id3" => "`id3_read` (alias `id3`) — read ID3 tags from an MP3 file as a hashref (title, artist, album, year, etc.).\n\n```perl\nmy $tags = id3(\"song.mp3\")\np \"$tags->{artist} - $tags->{title}\"\n```",
        "id3_write" | "id3w" => "`id3_write` (alias `id3w`) — write ID3 tags to an MP3 file from a hashref.\n\n```perl\nid3w(\"song.mp3\", {title => \"New Title\", artist => \"Artist\", year => \"2026\"})\n```",

        // ── Network ─────────────────────────────────────────────────────
        "net_interfaces" | "net_ifs" | "ifconfig" => "`net_interfaces` (aliases `net_ifs`, `ifconfig`) — list all network interfaces with name, IPs, MAC, and status.\n\n```perl\nmy @ifs = net_ifs()\nfor (@ifs) { p \"$_->{name}: $_->{ipv4}\" }\n```",
        "net_ipv4" | "myip" | "myip4" => "`net_ipv4` (aliases `myip`, `myip4`) — return the first non-loopback IPv4 address as a string.\n\n```perl\np myip()   # e.g. 192.168.1.42\n```",
        "net_ipv6" | "myip6" => "`net_ipv6` (alias `myip6`) — return the first non-loopback IPv6 address as a string.\n\n```perl\np myip6()   # e.g. fe80::1\n```",
        "net_mac" | "mymac" => "`net_mac` (alias `mymac`) — return the first non-loopback MAC address.\n\n```perl\np mymac()   # e.g. aa:bb:cc:dd:ee:ff\n```",
        "net_public_ip" | "pubip" | "extip" => "`net_public_ip` (aliases `pubip`, `extip`) — fetch your public IP address via HTTP.\n\n```perl\np pubip()   # e.g. 203.0.113.42\n```",
        "net_dns" | "dns_resolve" | "resolve" => "`net_dns` (aliases `dns_resolve`, `resolve`) — resolve a hostname to IP addresses.\n\n```perl\nmy @ips = resolve(\"github.com\")\np @ips\n```",
        "net_reverse_dns" | "rdns" => "`net_reverse_dns` (alias `rdns`) — reverse DNS lookup via UDP PTR query.\n\n```perl\np rdns(\"8.8.8.8\")   # dns.google\n```",
        "net_ping" | "ping" => "`net_ping` (alias `ping`) — TCP connect ping to host:port. Returns array of RTT measurements in ms.\n\n```perl\nmy @rtt = ping(\"google.com\", 443, 5)   # 5 pings\np mean(@rtt)\n```",
        "net_port_open" | "port_open" => "`net_port_open` (alias `port_open`) — check if a TCP port is open on a host. Returns boolean.\n\n```perl\np port_open(\"localhost\", 8080)   # 1 or 0\n```",
        "net_ports_scan" | "port_scan" | "portscan" => "`net_ports_scan` (aliases `port_scan`, `portscan`) — scan a range of TCP ports on a host. Returns list of open ports.\n\n```perl\nmy @open = port_scan(\"localhost\", 80, 443)\np @open   # e.g. 80, 443\n```",
        "net_latency" | "tcplat" => "`net_latency` (alias `tcplat`) — measure TCP connect latency to host:port in milliseconds.\n\n```perl\np tcplat(\"google.com\", 443)   # e.g. 12.5\n```",
        "net_download" | "download" | "wget" => "`net_download` (aliases `download`, `wget`) — download a URL to a local file via ureq.\n\n```perl\nwget(\"https://example.com/data.csv\", \"data.csv\")\n```",
        "net_headers" | "http_headers" => "`net_headers` (alias `http_headers`) — fetch HTTP response headers as a hashref.\n\n```perl\nmy $h = http_headers(\"https://example.com\")\np $h->{\"content-type\"}\n```",
        "net_dns_servers" | "dns_servers" => "`net_dns_servers` (alias `dns_servers`) — return the system's configured DNS server addresses.\n\n```perl\nmy @dns = dns_servers()\np @dns   # e.g. 8.8.8.8, 8.8.4.4\n```",
        "net_gateway" | "gateway" => "`net_gateway` (alias `gateway`) — return the default gateway IP address.\n\n```perl\np gateway()   # e.g. 192.168.1.1\n```",
        "net_whois" | "whois" => "`net_whois` (alias `whois`) — WHOIS lookup via raw TCP connection on port 43.\n\n```perl\np whois(\"example.com\")\n```",
        "net_hostname" => "`net_hostname` — return the system hostname.\n\n```perl\np net_hostname()   # e.g. my-macbook.local\n```",
        "smtp_send" | "send_email" | "email" => "`smtp_send` (aliases `send_email`, `email`) — send an email via SMTP with TLS. Takes a hashref with to, from, subject, body, and SMTP connection fields.\n\n```perl\nsmtp_send({to => \"bob@ex.com\", from => \"me@ex.com\", subject => \"Hi\",\n  body => \"Hello!\", smtp_host => \"smtp.ex.com\", smtp_port => 587,\n  smtp_user => \"me@ex.com\", smtp_pass => $pass})\n```",

        // ── Markup / Web Scraping ───────────────────────────────────────
        "html_parse" | "parse_html" => "`html_parse` (alias `parse_html`) — parse an HTML string into an array of element hashrefs with tag, text, and attrs.\n\n```perl\nmy @els = html_parse(slurp(\"page.html\"))\nfor (@els) { p \"$_->{tag}: $_->{text}\" }\n```",
        "css_select" | "css" | "qs" | "query_selector" => "`css_select` (aliases `css`, `qs`, `query_selector`) — query parsed HTML with a CSS selector. Returns matching elements as hashrefs with tag, text, attrs, and html.\n\n```perl\nmy @links = css_select(slurp(\"page.html\"), \"a.nav\")\nfor (@links) { p \"$_->{tag} $_->{attrs}{href}\" }\n```",
        "xml_parse" | "parse_xml" => "`xml_parse` (alias `parse_xml`) — parse an XML string into a nested hashref tree with tag, text, attrs, and children.\n\n```perl\nmy $root = xml_parse(slurp(\"data.xml\"))\np $root->{tag}\nfor (@{$root->{children}}) { p $_->{text} }\n```",
        "xpath" | "xml_select" => "`xpath` (alias `xml_select`) — query XML with XPath-like expressions (//tag, //tag[@attr='val']). Returns matching nodes as hashrefs.\n\n```perl\nmy @items = xpath(slurp(\"feed.xml\"), \"//item\")\nfor (@items) { p $_->{text} }\n```",

        // ── Date extensions ─────────────────────────────────────────────
        "dateseq" | "dseq" => "`dateseq` (alias `dseq`) — generate a sequence of dates. Args: start, end [, step]. Step defaults to 1 day.\n\n```perl\nmy @days = dseq(\"2026-01-01\", \"2026-01-07\")\np @days\n```",
        "dategrep" | "dgrep" => "`dategrep` (alias `dgrep`) — filter a list of date strings by a pattern or range.\n\n```perl\nmy @jan = dgrep(\"2026-01\", @dates)\n```",
        "dateround" | "dround" => "`dateround` (alias `dround`) — round date strings to a unit (day, hour, month, etc.).\n\n```perl\np dround(\"2026-04-19T14:35:22\", \"hour\")   # 2026-04-19T15:00:00\n```",
        "datesort" | "dsort" => "`datesort` (alias `dsort`) — sort date strings chronologically.\n\n```perl\nmy @sorted = dsort(\"2026-03-01\", \"2026-01-15\", \"2026-02-10\")\np @sorted\n```",

        // ── Git ─────────────────────────────────────────────────────────
        "git_log" | "glog" => "`git_log` (alias `glog`) — get the last N commits as an array of hashrefs (hash, author, date, message).\n\n```perl\nmy @commits = glog(10)\nfor (@commits) { p \"$_->{hash} $_->{message}\" }\n```",
        "git_status" | "gst" => "`git_status` (alias `gst`) — get working tree status as an array of hashrefs (path, status).\n\n```perl\nmy @st = gst()\nfor (@st) { p \"$_->{status} $_->{path}\" }\n```",
        "git_diff" | "gdiff" => "`git_diff` (alias `gdiff`) — get the current diff as a string.\n\n```perl\np gdiff()\ngdiff() |> lines |> grep /^\\+/ |> e p\n```",
        "git_branches" | "gbr" => "`git_branches` (alias `gbr`) — list all branches as an array of strings.\n\n```perl\nmy @br = gbr()\np @br\n```",
        "git_tags" | "gtags" => "`git_tags` (alias `gtags`) — list all tags as an array of strings.\n\n```perl\nmy @tags = gtags()\np @tags\n```",
        "git_blame" | "gblame" => "`git_blame` (alias `gblame`) — blame a file, returning per-line annotation.\n\n```perl\np gblame(\"src/main.rs\")\n```",
        "git_authors" | "gauthors" => "`git_authors` (alias `gauthors`) — list unique authors sorted by commit count as hashrefs.\n\n```perl\nmy @authors = gauthors()\nfor (@authors) { p \"$_->{name}: $_->{count}\" }\n```",
        "git_files" | "gfiles" => "`git_files` (alias `gfiles`) — list all tracked files in the repo.\n\n```perl\nmy @files = gfiles()\np scalar @files   # total tracked files\n```",
        "git_show" | "gshow" => "`git_show` (alias `gshow`) — show details of a commit (message, diff, author).\n\n```perl\np gshow(\"HEAD\")\np gshow(\"abc1234\")\n```",
        "git_root" | "groot" => "`git_root` (alias `groot`) — return the repository root path.\n\n```perl\np groot()   # e.g. /home/user/project\n```",

        // ── System ──────────────────────────────────────────────────────
        "mounts" | "disk_mounts" | "filesystems" => "`mounts` (aliases `disk_mounts`, `filesystems`) — list mounted filesystems with usage (total, used, available).\n\n```perl\nmy @m = mounts()\nfor (@m) { p \"$_->{mount}: $_->{used}/$_->{total}\" }\n```",
        "thread_count" | "nthreads" => "`thread_count` (alias `nthreads`) — return the rayon thread pool size.\n\n```perl\np nthreads()   # e.g. 8\n```",
        "pool_info" | "par_info" => "`pool_info` (alias `par_info`) — return thread pool details as a hashref (threads, queued, active).\n\n```perl\nmy $info = par_info()\np $info->{threads}\n```",
        "par_bench" | "pbench" => "`par_bench` (alias `pbench`) — run a parallel throughput benchmark and return results.\n\n```perl\nmy $result = pbench(1000000)\np $result->{ops_per_sec}\n```",
        "to_pdf" => "`to_pdf` — generate a PDF from text, SVG, or structured data. Returns raw PDF bytes.\n\n```perl\n\"Hello World\" |> to_pdf |> to_file(\"out.pdf\")\nscatter_svg([1,2,3], [1,4,9]) |> to_pdf |> to_file(\"plot.pdf\")\n```",
        "jq" => "`jq` — alias for `json_jq`. Query JSON data with jq-style expressions.\n\n```perl\nmy $data = json_decode(slurp(\"data.json\"))\np jq($data, \".items[].name\")\n```",

        // ── Directory Size ──────────────────────────────────────────────
        "du" | "dir_size" => "`du` (alias `dir_size`) — compute the total size of a directory tree in bytes, recursively walking all files.\n\n```perl\nmy $bytes = du(\"/usr/local\")\np \"$bytes bytes\"\n```",
        "du_tree" | "dir_sizes" => "`du_tree` (alias `dir_sizes`) — return directory sizes as an array of hashrefs `{path, size}`, sorted descending by size. Each entry is an immediate child directory.\n\n```perl\nmy @dirs = du_tree(\"/usr/local\")\nfor (@dirs) { p \"$_->{path}: $_->{size}\" }\n```",

        // ── Process ─────────────────────────────────────────────────────
        "process_list" | "ps" | "procs" => "`process_list` (aliases `ps`, `procs`) — list running processes as an array of hashrefs with `{pid, name, uid}` keys.\n\n```perl\nmy @procs = ps()\nfor (@procs) { p \"$_->{pid} $_->{name}\" }\n```",

        // ── PDF ─────────────────────────────────────────────────────────
        "pdf_text" | "pdf_read" | "pdf_extract" => "`pdf_text` (aliases `pdf_read`, `pdf_extract`) — extract all text content from a PDF file and return it as a string.\n\n```perl\nmy $text = pdf_text(\"report.pdf\")\np $text |> lines |> cnt   # number of lines\n```",
        "pdf_pages" => "`pdf_pages` — return the number of pages in a PDF file.\n\n```perl\np pdf_pages(\"report.pdf\")   # e.g. 42\n```",

        // ── Testing ─────────────────────────────────────────────────────
        "assert_eq" | "aeq" => "`assert_eq` (alias `aeq`) — assert that two values are equal as strings. Takes A, B, and an optional message. Fails the test with a diff if A ne B.\n\n```perl\nassert_eq $got, $expected, \"username matches\"\naeq length(@arr), 3\n```",
        "assert_ne" | "ane" => "`assert_ne` (alias `ane`) — assert that two values are not equal as strings. Takes A, B, and an optional message. Fails if A eq B.\n\n```perl\nassert_ne $token, \"\", \"token must not be empty\"\nane $a, $b\n```",
        "assert_ok" | "aok" => "`assert_ok` (alias `aok`) — assert that a value is truthy (defined and non-zero/non-empty). Fails if the value is falsy or undef.\n\n```perl\nassert_ok $result, \"fetch returned data\"\naok $user->{active}\n```",
        "assert_err" => "`assert_err` — assert that a value is falsy or undef. The inverse of `assert_ok`. Useful for verifying error conditions or absent values.\n\n```perl\nassert_err $deleted_user, \"user should be gone\"\nassert_err 0\n```",
        "assert_true" | "atrue" => "`assert_true` (alias `atrue`) — alias for `assert_ok`. Assert that a value is truthy.\n\n```perl\nassert_true $connected, \"should be connected\"\natrue $flag\n```",
        "assert_false" | "afalse" => "`assert_false` (alias `afalse`) — alias for `assert_err`. Assert that a value is falsy or undef.\n\n```perl\nassert_false $error, \"no error expected\"\nafalse $disabled\n```",
        "assert_gt" => "`assert_gt` — assert that the first numeric value is strictly greater than the second. Fails with the actual values on mismatch.\n\n```perl\nassert_gt $elapsed, 0, \"elapsed must be positive\"\nassert_gt scalar(@results), 10\n```",
        "assert_lt" => "`assert_lt` — assert that the first numeric value is strictly less than the second.\n\n```perl\nassert_lt $latency, 100, \"latency under 100ms\"\nassert_lt $errors, $threshold\n```",
        "assert_ge" => "`assert_ge` — assert that the first numeric value is greater than or equal to the second.\n\n```perl\nassert_ge $count, 1, \"at least one result\"\nassert_ge $version, 2\n```",
        "assert_le" => "`assert_le` — assert that the first numeric value is less than or equal to the second.\n\n```perl\nassert_le $memory, $limit, \"within memory budget\"\nassert_le length($buf), 4096\n```",
        "assert_match" | "amatch" => "`assert_match` (alias `amatch`) — assert that a string matches a regex pattern. Fails with the actual string and pattern on mismatch.\n\n```perl\nassert_match $email, qr/\\@/, \"must contain @\"\namatch $line, qr/^OK/\n```",
        "assert_contains" | "acontains" => "`assert_contains` (alias `acontains`) — assert that a string contains a given substring. Fails with both values on mismatch.\n\n```perl\nassert_contains $html, \"<title>\", \"page has title\"\nacontains $log, \"SUCCESS\"\n```",
        "assert_near" | "anear" => "`assert_near` (alias `anear`) — assert that two floats are approximately equal within an epsilon tolerance (default 1e-9). Essential for floating-point comparisons.\n\n```perl\nassert_near 0.1 + 0.2, 0.3, 1e-10, \"float add\"\nanear $pi, 3.14159, 1e-5\n```",
        "assert_dies" | "adies" => "`assert_dies` (alias `adies`) — assert that a block throws an error. Passes if the block dies, fails if it returns normally.\n\n```perl\nassert_dies { die \"boom\" } \"should throw\"\nadies { 1 / 0 }\n```",
        "test_run" | "run_tests" => "`test_run` (alias `run_tests`) — print a test summary with pass/fail counts and exit with code 1 if any test failed. Call at the end of a test file to report results.\n\n```perl\n# ... assertions above ...\ntest_run   # prints summary, exits 1 on failure\n```",

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

/// Public entry point for `stryke docs TOPIC` — returns raw markdown doc text.
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
    // completion-words file (the bulk of the ~1800 builtins).
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

/// Grouped category list for the `stryke docs` book view and the static-site
/// `docs/reference.html` generator (`cargo run --bin gen-docs`). Each tuple
/// is (chapter name, topic names); topics must match `doc_for_label_text`
/// keys or the generator will skip them.
pub const DOC_CATEGORIES: &[(&str, &[&str])] = &[
    (
        "Parallel Primitives",
        &[
            "pmap",
            "pmaps",
            "pmap_chunked",
            "pgrep",
            "pgreps",
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
            "pflat_maps",
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
            "~>",
            "->>",
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
        "stryke Extensions",
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
            "from_json",
            "from_yaml",
            "from_toml",
            "from_xml",
            "from_csv",
            "xopen",
            "clip",
            "paste",
            "to_table",
            "sparkline",
            "bar_chart",
            "flame",
            "histo",
            "gauge",
            "spinner",
            "csv_read",
            "csv_write",
            "dataframe",
            "sqlite",
            "digits",
            "letters",
            "sentences",
            "paragraphs",
            "sections",
            "numbers",
            "graphemes",
            "columns",
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
            "blake2b",
            "blake2s",
            "blake3",
            "argon2_hash",
            "argon2_verify",
            "bcrypt_hash",
            "bcrypt_verify",
            "scrypt_hash",
            "scrypt_verify",
            "pbkdf2",
            "random_bytes",
            "random_bytes_hex",
            "aes_encrypt",
            "aes_decrypt",
            "chacha_encrypt",
            "chacha_decrypt",
            "ed25519_keygen",
            "ed25519_sign",
            "ed25519_verify",
            "x25519_keygen",
            "x25519_dh",
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
            "sha3_256",
            "sha3_512",
            "shake128",
            "shake256",
            "ripemd160",
            "siphash",
            "hmac_sha1",
            "hmac_sha384",
            "hmac_sha512",
            "hmac_md5",
            "hkdf_sha256",
            "hkdf_sha512",
            "poly1305",
            "rsa_keygen",
            "rsa_encrypt",
            "rsa_decrypt",
            "rsa_sign",
            "rsa_verify",
            "ecdsa_p256_keygen",
            "ecdsa_p256_sign",
            "ecdsa_p256_verify",
            "ecdsa_p384_keygen",
            "ecdsa_p384_sign",
            "ecdsa_p384_verify",
            "ecdsa_secp256k1_keygen",
            "ecdsa_secp256k1_sign",
            "ecdsa_secp256k1_verify",
            "ecdh_p256",
            "ecdh_p384",
            "base32_encode",
            "base32_decode",
            "base58_encode",
            "base58_decode",
            "totp",
            "totp_verify",
            "hotp",
            "aes_cbc_encrypt",
            "aes_cbc_decrypt",
            "qr_ascii",
            "qr_png",
            "qr_svg",
            "barcode_code128",
            "barcode_code39",
            "barcode_ean13",
            "barcode_svg",
            "brotli",
            "brotli_decode",
            "xz",
            "xz_decode",
            "bzip2",
            "bzip2_decode",
            "lz4",
            "lz4_decode",
            "snappy",
            "snappy_decode",
            "lzw",
            "lzw_decode",
            "tar_create",
            "tar_extract",
            "tar_list",
            "tar_gz_create",
            "tar_gz_extract",
            "zip_create",
            "zip_extract",
            "zip_list",
            "md4",
            "xxh32",
            "xxh64",
            "xxh3",
            "xxh3_128",
            "murmur3",
            "murmur3_128",
            "blowfish_encrypt",
            "blowfish_decrypt",
            "des3_encrypt",
            "des3_decrypt",
            "twofish_encrypt",
            "twofish_decrypt",
            "camellia_encrypt",
            "camellia_decrypt",
            "cast5_encrypt",
            "cast5_decrypt",
            "salsa20",
            "salsa20_decrypt",
            "xsalsa20",
            "xsalsa20_decrypt",
            "secretbox",
            "secretbox_open",
            "nacl_box_keygen",
            "nacl_box",
            "nacl_box_open",
        ],
    ),
    (
        "Special Math Functions",
        &[
            "erf",
            "erfc",
            "gamma",
            "lgamma",
            "digamma",
            "beta_fn",
            "lbeta",
            "betainc",
            "gammainc",
            "gammaincc",
            "gammainc_reg",
            "gammaincc_reg",
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
            "du",
            "du_tree",
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
            "process_list",
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
    (
        "Charts (SVG)",
        &[
            "scatter_svg",
            "line_svg",
            "plot_svg",
            "hist_svg",
            "boxplot_svg",
            "bar_svg",
            "pie_svg",
            "heatmap_svg",
            "donut_svg",
            "area_svg",
            "hbar_svg",
            "radar_svg",
            "candlestick_svg",
            "violin_svg",
            "cor_heatmap",
            "stacked_bar_svg",
            "wordcloud_svg",
            "treemap_svg",
            "preview",
        ],
    ),
    (
        "Audio",
        &["audio_convert", "audio_info", "id3_read", "id3_write"],
    ),
    (
        "Network Utilities",
        &[
            "net_interfaces",
            "net_ipv4",
            "net_ipv6",
            "net_mac",
            "net_public_ip",
            "net_dns",
            "net_reverse_dns",
            "net_ping",
            "net_port_open",
            "net_ports_scan",
            "net_latency",
            "net_download",
            "net_headers",
            "net_dns_servers",
            "net_gateway",
            "net_whois",
            "net_hostname",
            "smtp_send",
        ],
    ),
    (
        "Markup / Web Scraping",
        &["html_parse", "css_select", "xml_parse", "xpath"],
    ),
    (
        "Date Utilities",
        &["dateseq", "dategrep", "dateround", "datesort"],
    ),
    (
        "Git",
        &[
            "git_log",
            "git_status",
            "git_diff",
            "git_branches",
            "git_tags",
            "git_blame",
            "git_authors",
            "git_files",
            "git_show",
            "git_root",
        ],
    ),
    (
        "System",
        &[
            "mounts",
            "thread_count",
            "pool_info",
            "par_bench",
            "to_pdf",
            "pdf_text",
            "pdf_pages",
            "jq",
        ],
    ),
    (
        "Testing",
        &[
            "assert_eq",
            "assert_ne",
            "assert_ok",
            "assert_err",
            "assert_true",
            "assert_false",
            "assert_gt",
            "assert_lt",
            "assert_ge",
            "assert_le",
            "assert_match",
            "assert_contains",
            "assert_near",
            "assert_dies",
            "test_run",
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
            "pmaps",
            "${1:source} |> pmaps { ${0} } |> ep",
            "Streaming parallel map (snippet)",
        ),
        (
            "pgrep",
            "my @${1:out} = pgrep { ${0} } @${2:list};",
            "Parallel grep (snippet)",
        ),
        (
            "pgreps",
            "${1:source} |> pgreps { ${0} } |> ep",
            "Streaming parallel grep (snippet)",
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
            "par_lines \"${1:file}\", fn {\n\t${0}\n};",
            "Parallel line scan (snippet)",
        ),
        (
            "par_walk",
            "par_walk \"${1:dir}\", fn {\n\t${0}\n};",
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
        (
            "scatter_svg",
            "scatter_svg([${1:xs}], [${2:ys}], \"${3:Title}\") |> to_file(\"${4:plot}.svg\")",
            "SVG scatter plot (snippet)",
        ),
        (
            "bar_svg",
            "bar_svg([${1:labels}], [${2:values}], \"${3:Title}\") |> to_file(\"${4:chart}.svg\")",
            "SVG bar chart (snippet)",
        ),
        (
            "preview",
            "${1:svg_expr} |> preview",
            "Preview SVG in browser (snippet)",
        ),
        (
            "port_scan",
            "my @open = port_scan(\"${1:host}\", ${2:80}, ${3:443})\np @open",
            "Scan TCP port range (snippet)",
        ),
        (
            "git_log",
            "my @commits = glog(${1:10})\nfor (@commits) { p \"\\$_->{hash} \\$_->{message}\" }",
            "Git log (snippet)",
        ),
        (
            "net_ping",
            "my @rtt = ping(\"${1:host}\", ${2:443}, ${3:5})\np mean(@rtt)",
            "TCP ping with RTT (snippet)",
        ),
        (
            "wget",
            "wget(\"${1:url}\", \"${2:output_file}\")",
            "Download URL to file (snippet)",
        ),
        (
            "id3",
            "my \\$tags = id3(\"${1:file.mp3}\")\np \"\\$tags->{artist} - \\$tags->{title}\"",
            "Read MP3 ID3 tags (snippet)",
        ),
        (
            "donut_svg",
            "${1:hashref} |> donut(\"${2:Title}\") |> to_file(\"${3:donut}.svg\")",
            "SVG donut chart (snippet)",
        ),
        (
            "wordcloud",
            "${1:freq_hash} |> wordcloud(\"${2:Title}\") |> to_file(\"${3:cloud}.svg\")",
            "SVG word cloud (snippet)",
        ),
        (
            "test",
            "#!/usr/bin/env stryke\n\n${1:# test description}\nassert_eq ${2:got}, ${3:expected}, \"${4:label}\"\nassert_ok ${5:value}, \"${6:is truthy}\"\nassert_dies { ${7:die \"boom\"} } \"${8:should throw}\"\n\ntest_run\n",
            "Test file scaffold (snippet)",
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
            SubSigParam::Scalar(n, _ty, _default) => {
                idx.scalars.insert(n.clone());
            }
            SubSigParam::Array(n, _default) => {
                idx.arrays.insert(n.clone());
            }
            SubSigParam::Hash(n, _default) => {
                idx.hashes.insert(n.clone());
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

    #[test]
    fn utf16_col_to_byte_ascii() {
        let s = "hello";
        assert_eq!(utf16_col_to_byte_idx(s, 0), 0);
        assert_eq!(utf16_col_to_byte_idx(s, 3), 3);
        assert_eq!(utf16_col_to_byte_idx(s, 5), 5);
        assert_eq!(utf16_col_to_byte_idx(s, 10), 5);
    }

    #[test]
    fn utf16_col_to_byte_multibyte() {
        let s = "héllo";
        assert_eq!(utf16_col_to_byte_idx(s, 0), 0);
        assert_eq!(utf16_col_to_byte_idx(s, 1), 1);
        assert_eq!(utf16_col_to_byte_idx(s, 2), 3);
    }

    #[test]
    fn utf16_col_to_byte_emoji() {
        let s = "a😀b";
        assert_eq!(utf16_col_to_byte_idx(s, 0), 0);
        assert_eq!(utf16_col_to_byte_idx(s, 1), 1);
        assert_eq!(utf16_col_to_byte_idx(s, 3), 5);
    }

    #[test]
    fn identifier_span_scalar() {
        let line = "my $foo = 1;";
        let (s, e) = identifier_span_bytes(line, 4).unwrap();
        assert_eq!(&line[s..e], "foo");
    }

    #[test]
    fn identifier_span_at_start() {
        let line = "print hello";
        let (s, e) = identifier_span_bytes(line, 0).unwrap();
        assert_eq!(&line[s..e], "print");
    }

    #[test]
    fn identifier_span_at_end() {
        let line = "hello world";
        let (s, e) = identifier_span_bytes(line, 9).unwrap();
        assert_eq!(&line[s..e], "world");
    }

    #[test]
    fn identifier_span_at_space_edge() {
        let line = "hello world";
        let result = identifier_span_bytes(line, 5);
        if let Some((s, e)) = result {
            assert!(&line[s..e] == "hello" || &line[s..e] == "world");
        }
    }

    #[test]
    fn resolve_sub_decl_exact_match() {
        let mut m = HashMap::new();
        m.insert("main::foo".to_string(), 10usize);
        m.insert("Pkg::foo".to_string(), 20usize);
        assert_eq!(resolve_sub_decl_line(&m, "main::foo"), Some(10));
        assert_eq!(resolve_sub_decl_line(&m, "Pkg::foo"), Some(20));
    }

    #[test]
    fn resolve_sub_decl_ambiguous() {
        let mut m = HashMap::new();
        m.insert("A::foo".to_string(), 1usize);
        m.insert("B::foo".to_string(), 2usize);
        assert!(resolve_sub_decl_line(&m, "foo").is_none());
    }

    #[test]
    fn highlights_multiple_occurrences() {
        let src = "$x = 1; $x = 2; $x = 3;";
        let h = highlights_for_identifier(src, "x");
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn highlights_different_sigils() {
        let src = "@arr = (1); push @arr, 2;";
        let h = highlights_for_identifier(src, "arr");
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn bare_completion_qualified() {
        let (_, s) = raw_at("Foo::Bar::baz", 13);
        assert!(s.contains("baz") || s.contains("Foo"));
    }

    #[test]
    fn completion_at_sigil_only() {
        let (m, s) = raw_at("$", 1);
        assert!(matches!(m, LineCompletionMode::Scalar(_)));
        assert_eq!(s, "");
    }

    #[test]
    fn builtin_docs_exist() {
        use super::doc_text_for;
        let builtins = ["print", "say", "chomp", "length", "substr", "split", "join"];
        for b in builtins {
            assert!(doc_text_for(b).is_some(), "Doc for '{}' should exist", b);
        }
    }
}
