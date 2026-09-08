//! The rust-analyzer client (ticket #6). One language server, spawned when a
//! Rust file inside a cargo project is first shown; utf-8 position encoding
//! is negotiated so all offsets are plain byte columns — no utf-16 math.

mod transport;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use transport::Transport;

/// Severity subset the editor renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// One diagnostic, positions in utf-8 columns (negotiated).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub severity: Severity,
    pub message: String,
}

/// A jump target from goto-definition.
#[derive(Debug, Clone)]
pub struct Location {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
}

/// One inlay hint, position in utf-8 columns; the label flattened from
/// its parts and padded as the server asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHint {
    pub line: usize,
    pub col: usize,
    pub label: String,
    /// LSP InlayHintKind: 1 = type, 2 = parameter, absent for the rest
    /// (chaining, closing brace, lifetime, adjustment, binding mode…).
    pub kind: Option<i64>,
}

/// A code action offered by the server, with its (possibly lazy) edit.
#[derive(Debug, Clone)]
pub struct Action {
    pub title: String,
    pub raw: Value,
}

/// One completion candidate (ticket #22, iteration one).
#[derive(Clone)]
pub struct CompletionItem {
    /// Display text in the popup.
    pub label: String,
    /// Plain text inserted on accept (snippet placeholders stripped).
    pub insert_text: String,
    /// Text matched against the typed prefix (label when absent).
    pub filter_text: String,
    /// Server-provided ranking key (label when absent).
    pub sort_text: String,
    /// Type / signature, shown dimmed beside the label.
    pub detail: Option<String>,
    /// LSP CompletionItemKind (2=method, 3=function, 5=field, …).
    pub kind: Option<i64>,
}

/// Typed events handed to the editor each tick.
pub enum Event {
    DiagnosticsUpdated(PathBuf),
    Hover(Option<String>),
    Definition(Option<Location>),
    Actions(Vec<Action>),
    ActionResolved(Box<Value>),
    Completion(Vec<CompletionItem>),
    ExpandedMacro(Option<(String, String)>),
    /// The line-scope hover: (annotated line, signature label).
    InlayLine(Option<String>, Option<String>),
    /// Every hint of a document, for the buffer revision the request named
    /// (the compiler's-eye view, #93).
    InlayHints(PathBuf, u64, Vec<InlayHint>),
    /// The server's hints changed underneath (a dependency was indexed):
    /// documents that show them should ask again.
    InlayRefresh,
    /// The workspace edit for renaming to the given name — or the server's
    /// reason it can't ("cannot rename a builtin"), verbatim (#97).
    Renamed(String, Result<Value, String>),
    ApplyEdit(Box<Value>),
    Progress(Option<String>),
    ServerExited(String),
}

/// What a pending request id maps back to.
enum Pending {
    Initialize,
    Hover,
    Definition,
    Actions,
    Completion,
    ResolveAction,
    ExpandMacro,
    InlayLine { text: String },
    SignatureFor { annotated: Option<String> },
    InlayDoc { path: PathBuf, revision: u64 },
    Rename { new_name: String },
}

pub struct Client {
    transport: Transport,
    root: PathBuf,
    next_id: i64,
    pending: HashMap<i64, Pending>,
    initialized: bool,
    /// Documents opened on the server, with the last version sent.
    open_docs: HashMap<PathBuf, i64>,
    pub diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    /// Latest indexing progress label, for the statusline.
    pub progress: Option<String>,
    queued: Vec<Value>,
    /// Position of the in-flight gK request (for the signature follow-up).
    inlay_pos: Option<(PathBuf, usize, usize)>,
}

fn uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    uri.strip_prefix("file://").map(PathBuf::from)
}

/// The cargo project root governing `file`, if any.
pub fn project_root(file: &Path) -> Option<PathBuf> {
    let mut dir = file.parent()?;
    let mut found = None;
    loop {
        if dir.join("Cargo.toml").is_file() {
            found = Some(dir.to_path_buf()); // keep climbing: workspace root wins
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => return found,
        }
    }
}

impl Client {
    /// Spawns rust-analyzer for a project root. Returns None when the binary
    /// is unavailable.
    pub fn start(root: PathBuf) -> Option<Self> {
        let child = Command::new("rust-analyzer")
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let transport = Transport::new(child).ok()?;
        let mut client = Self {
            transport,
            root,
            next_id: 0,
            pending: HashMap::new(),
            initialized: false,
            open_docs: HashMap::new(),
            diagnostics: HashMap::new(),
            progress: Some("starting".into()),
            queued: Vec::new(),
            inlay_pos: None,
        };
        client.request(
            Pending::Initialize,
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": uri(&client.root),
                "capabilities": {
                    "general": { "positionEncodings": ["utf-8"] },
                    "textDocument": {
                        "hover": { "contentFormat": ["plaintext", "markdown"] },
                        "publishDiagnostics": {},
                        "codeAction": {
                            "codeActionLiteralSupport": {
                                "codeActionKind": { "valueSet": ["", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source"] }
                            },
                            "resolveSupport": { "properties": ["edit"] }
                        },
                        "inlayHint": {},
                        "rename": { "prepareSupport": false },
                        "definition": {},
                        "completion": {
                            "completionItem": {
                                "snippetSupport": false,
                                "labelDetailsSupport": true
                            }
                        }
                    },
                    "window": { "workDoneProgress": true },
                    "workspace": { "applyEdit": true, "workspaceEdit": { "documentChanges": true } }
                },
                "initializationOptions": {
                    "checkOnSave": true,
                    // direnv's `.direnv/flake-profile-*` symlinks into the nix
                    // store's shell environment; rust-analyzer's scanner
                    // follows it and never finishes loading the workspace —
                    // no hover, no hints, no diagnostics, silently (#93)
                    "files": { "excludeDirs": [".direnv"] },
                    // every hint kind the compiler's-eye view (#93) prints;
                    // nothing is shown inline, so the breadth costs nothing
                    "inlayHints": {
                        "maxLength": null,
                        "typeHints": { "enable": true, "hideClosureInitialization": false, "hideNamedConstructor": false },
                        "parameterHints": { "enable": true },
                        "chainingHints": { "enable": true },
                        "closingBraceHints": { "enable": true, "minLines": 25 },
                        "closureReturnTypeHints": { "enable": "always" },
                        "lifetimeElisionHints": { "enable": "always", "useParameterNames": true },
                        // not expressionAdjustmentHints: every auto-ref/deref
                        // as `&**&x` is the MIR's eye, not the compiler's, and
                        // "reborrow" hides almost none of them (tried, #93)
                        "bindingModeHints": { "enable": true }
                    }
                }
            }),
        );
        Some(client)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn request(&mut self, pending: Pending, method: &str, params: Value) -> i64 {
        self.next_id += 1;
        let id = self.next_id;
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if self.initialized || matches!(pending, Pending::Initialize) {
            let _ = self.transport.send(&msg);
        } else {
            self.queued.push(msg);
        }
        self.pending.insert(id, pending);
        id
    }

    fn notify(&mut self, method: &str, params: Value) {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        if self.initialized {
            let _ = self.transport.send(&msg);
        } else {
            self.queued.push(msg);
        }
    }

    fn respond(&mut self, id: &Value, result: Value) {
        let msg = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        let _ = self.transport.send(&msg);
    }

    // ------------------------------------------------------------------
    // document sync
    // ------------------------------------------------------------------

    pub fn is_open(&self, path: &Path) -> bool {
        self.open_docs.contains_key(path)
    }

    pub fn did_open(&mut self, path: &Path, text: &str, version: i64) {
        self.open_docs.insert(path.to_path_buf(), version);
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": {
                "uri": uri(path), "languageId": "rust", "version": version, "text": text
            }}),
        );
    }

    /// Full-content sync; returns false when the version was already sent.
    pub fn did_change(&mut self, path: &Path, text: &str, version: i64) -> bool {
        match self.open_docs.get(path) {
            Some(v) if *v == version => return false,
            None => return false,
            _ => {}
        }
        self.open_docs.insert(path.to_path_buf(), version);
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri(path), "version": version },
                "contentChanges": [{ "text": text }]
            }),
        );
        true
    }

    pub fn did_save(&mut self, path: &Path) {
        if self.is_open(path) {
            self.notify(
                "textDocument/didSave",
                json!({ "textDocument": { "uri": uri(path) } }),
            );
        }
    }

    pub fn did_close(&mut self, path: &Path) {
        if self.open_docs.remove(path).is_some() {
            self.notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": uri(path) } }),
            );
            self.diagnostics.remove(path);
        }
    }

    // ------------------------------------------------------------------
    // requests
    // ------------------------------------------------------------------

    fn doc_pos(path: &Path, line: usize, col: usize) -> Value {
        json!({
            "textDocument": { "uri": uri(path) },
            "position": { "line": line, "character": col }
        })
    }

    pub fn hover(&mut self, path: &Path, line: usize, col: usize) {
        self.request(
            Pending::Hover,
            "textDocument/hover",
            Self::doc_pos(path, line, col),
        );
    }

    pub fn definition(&mut self, path: &Path, line: usize, col: usize) {
        self.request(
            Pending::Definition,
            "textDocument/definition",
            Self::doc_pos(path, line, col),
        );
    }

    pub fn completion(&mut self, path: &Path, line: usize, col: usize) {
        self.request(
            Pending::Completion,
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri(path) },
                "position": { "line": line, "character": col },
                "context": { "triggerKind": 1 }
            }),
        );
    }

    pub fn code_actions(
        &mut self,
        path: &Path,
        line: usize,
        col: usize,
        end_col: usize,
        diags: Vec<Value>,
    ) {
        self.request(
            Pending::Actions,
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": uri(path) },
                "range": {
                    "start": { "line": line, "character": col },
                    "end": { "line": line, "character": end_col }
                },
                "context": { "diagnostics": diags }
            }),
        );
    }

    pub fn resolve_action(&mut self, action: Value) {
        self.request(Pending::ResolveAction, "codeAction/resolve", action);
    }

    pub fn expand_macro(&mut self, path: &Path, line: usize, col: usize) {
        self.request(
            Pending::ExpandMacro,
            "rust-analyzer/expandMacro",
            Self::doc_pos(path, line, col),
        );
    }

    /// The line-scope hover: inlay hints for one line, spliced by the editor.
    /// `col` (cursor) also anchors the signature-help follow-up.
    pub fn inlay_line(&mut self, path: &Path, line: usize, col: usize, text: String, len: usize) {
        self.inlay_pos = Some((path.to_path_buf(), line, col));
        self.request(
            Pending::InlayLine { text },
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri(path) },
                "range": {
                    "start": { "line": line, "character": 0 },
                    "end": { "line": line, "character": len }
                }
            }),
        );
    }

    /// Asks for the workspace edit that renames the symbol at a position.
    pub fn rename(&mut self, path: &Path, line: usize, col: usize, new_name: &str) {
        self.request(
            Pending::Rename {
                new_name: new_name.to_string(),
            },
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri(path) },
                "position": { "line": line, "character": col },
                "newName": new_name
            }),
        );
    }

    /// Every hint of a document — the compiler's-eye view (#93). `lines`
    /// is the line count so the range covers the whole text; `revision` is
    /// echoed back so stale answers can be told from fresh ones.
    pub fn inlay_document(&mut self, path: &Path, lines: usize, revision: u64) {
        self.request(
            Pending::InlayDoc {
                path: path.to_path_buf(),
                revision,
            },
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri(path) },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": lines, "character": 0 }
                }
            }),
        );
    }

    pub fn shutdown(&mut self) {
        let _ = self
            .transport
            .send(&json!({ "jsonrpc": "2.0", "id": 999_999, "method": "shutdown" }));
        let _ = self
            .transport
            .send(&json!({ "jsonrpc": "2.0", "method": "exit" }));
        self.transport.shutdown();
    }

    // ------------------------------------------------------------------
    // incoming
    // ------------------------------------------------------------------

    /// Drains server messages into editor-facing events.
    pub fn drain(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let msg = match self.transport.incoming.try_recv() {
                Ok(m) => m,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if self.initialized || !self.pending.is_empty() {
                        self.initialized = false;
                        self.pending.clear();
                        events.push(Event::ServerExited("rust-analyzer exited".into()));
                    }
                    break;
                }
            };
            self.handle(msg, &mut events);
        }
        events
    }

    fn handle(&mut self, msg: Value, events: &mut Vec<Event>) {
        // server -> client request: must be answered or RA stalls
        if let (Some(id), Some(method)) = (msg.get("id"), msg.get("method").and_then(Value::as_str))
        {
            let id = id.clone();
            match method {
                "workspace/configuration" => {
                    let n = msg["params"]["items"].as_array().map_or(1, Vec::len);
                    self.respond(&id, json!(vec![Value::Null; n]));
                }
                "workspace/inlayHint/refresh" => {
                    self.respond(&id, Value::Null);
                    events.push(Event::InlayRefresh);
                }
                "window/workDoneProgress/create"
                | "client/registerCapability"
                | "workspace/semanticTokens/refresh" => {
                    self.respond(&id, Value::Null);
                }
                "workspace/applyEdit" => {
                    events.push(Event::ApplyEdit(Box::new(msg["params"]["edit"].clone())));
                    self.respond(&id, json!({ "applied": true }));
                }
                _ => self.respond(&id, Value::Null),
            }
            return;
        }

        // notification
        if let Some(method) = msg.get("method").and_then(Value::as_str) {
            match method {
                "textDocument/publishDiagnostics" => {
                    let params = &msg["params"];
                    if let Some(path) = params["uri"].as_str().and_then(uri_to_path) {
                        let diags = params["diagnostics"]
                            .as_array()
                            .map(|a| a.iter().filter_map(parse_diagnostic).collect())
                            .unwrap_or_default();
                        self.diagnostics.insert(path.clone(), diags);
                        events.push(Event::DiagnosticsUpdated(path));
                    }
                }
                "$/progress" => {
                    let params = &msg["params"];
                    let value = &params["value"];
                    let label = match value["kind"].as_str() {
                        Some("end") => None,
                        _ => Some(
                            value["title"]
                                .as_str()
                                .or_else(|| value["message"].as_str())
                                .unwrap_or("working")
                                .to_string(),
                        ),
                    };
                    self.progress = label.clone();
                    events.push(Event::Progress(label));
                }
                _ => {}
            }
            return;
        }

        // response
        let Some(id) = msg.get("id").and_then(Value::as_i64) else {
            return;
        };
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        let result = msg.get("result").cloned().unwrap_or(Value::Null);
        let error = msg
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string);
        match pending {
            Pending::Initialize => {
                self.initialized = true;
                let _ = self
                    .transport
                    .send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
                for queued in std::mem::take(&mut self.queued) {
                    let _ = self.transport.send(&queued);
                }
            }
            Pending::Hover => events.push(Event::Hover(parse_hover(&result))),
            Pending::Definition => events.push(Event::Definition(parse_definition(&result))),
            Pending::Actions => {
                let actions = result
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| {
                                Some(Action {
                                    title: v["title"].as_str()?.to_string(),
                                    raw: v.clone(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                events.push(Event::Actions(actions));
            }
            Pending::Completion => events.push(Event::Completion(parse_completion(&result))),
            Pending::ResolveAction => events.push(Event::ActionResolved(Box::new(result))),
            Pending::ExpandMacro => {
                let expansion = result["expansion"].as_str().map(|e| {
                    (
                        result["name"].as_str().unwrap_or("macro").to_string(),
                        e.to_string(),
                    )
                });
                events.push(Event::ExpandedMacro(expansion));
            }
            Pending::InlayLine { text } => {
                // second half of gK: the active call signature carries the
                // parameter TYPES the inlay hints lack
                let annotated = splice_inlay_hints(&text, &result);
                let pos = self.inlay_pos.take();
                match pos {
                    Some((path, line, col)) => {
                        self.request(
                            Pending::SignatureFor { annotated },
                            "textDocument/signatureHelp",
                            Self::doc_pos(&path, line, col),
                        );
                    }
                    None => events.push(Event::InlayLine(annotated, None)),
                }
            }
            Pending::Rename { new_name } => {
                let outcome = match error {
                    Some(e) => Err(e),
                    None => Ok(result),
                };
                events.push(Event::Renamed(new_name, outcome));
            }
            Pending::InlayDoc { path, revision } => {
                events.push(Event::InlayHints(
                    path,
                    revision,
                    parse_inlay_hints(&result),
                ));
            }
            Pending::SignatureFor { annotated } => {
                let signature = result["signatures"]
                    .as_array()
                    .and_then(|sigs| {
                        let active = result["activeSignature"].as_u64().unwrap_or(0) as usize;
                        sigs.get(active).or_else(|| sigs.first())
                    })
                    .and_then(|s| s["label"].as_str())
                    .map(str::to_string);
                events.push(Event::InlayLine(annotated, signature));
            }
        }
    }
}

fn parse_diagnostic(v: &Value) -> Option<Diagnostic> {
    let range = &v["range"];
    let severity = match v["severity"].as_i64().unwrap_or(1) {
        1 => Severity::Error,
        2 => Severity::Warning,
        _ => Severity::Info,
    };
    Some(Diagnostic {
        line: range["start"]["line"].as_u64()? as usize,
        col: range["start"]["character"].as_u64()? as usize,
        end_line: range["end"]["line"].as_u64()? as usize,
        end_col: range["end"]["character"].as_u64()? as usize,
        severity,
        message: v["message"].as_str()?.to_string(),
    })
}

/// A CompletionList (`{items:[…]}`) or a bare item array → typed items.
/// Snippet-format inserts are reduced to plain text (no snippets, #22).
fn parse_completion(result: &Value) -> Vec<CompletionItem> {
    let items = result
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| result.as_array());
    let Some(items) = items else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|v| {
            let label = v["label"].as_str()?.to_string();
            let is_snippet = v["insertTextFormat"].as_i64() == Some(2);
            let raw_insert = v["insertText"]
                .as_str()
                .or_else(|| v["textEdit"]["newText"].as_str())
                .unwrap_or(&label);
            let insert_text = if is_snippet {
                strip_snippet(raw_insert)
            } else {
                raw_insert.to_string()
            };
            Some(CompletionItem {
                filter_text: v["filterText"].as_str().unwrap_or(&label).to_string(),
                sort_text: v["sortText"].as_str().unwrap_or(&label).to_string(),
                detail: v["detail"].as_str().map(str::to_string),
                kind: v["kind"].as_i64(),
                insert_text,
                label,
            })
        })
        .collect()
}

/// Removes LSP snippet placeholders — `$0`, `$1`, `${1:name}` — leaving the
/// literal text around them (so `foo($0)` becomes `foo()`).
fn strip_snippet(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // escaped char in a snippet: keep the next literal
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else if c == '$' {
            match chars.peek() {
                Some('{') => {
                    // ${n:placeholder} — drop up to the matching brace
                    for n in chars.by_ref() {
                        if n == '}' {
                            break;
                        }
                    }
                }
                Some(d) if d.is_ascii_digit() => {
                    while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                        chars.next();
                    }
                }
                _ => out.push('$'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_hover(result: &Value) -> Option<String> {
    let contents = result.get("contents")?;
    let text = contents["value"]
        .as_str()
        .map(str::to_string)
        .or_else(|| contents.as_str().map(str::to_string))?;
    Some(text)
}

fn parse_definition(result: &Value) -> Option<Location> {
    let loc = if result.is_array() {
        result.get(0)?
    } else {
        result
    };
    // Location or LocationLink
    let (uri_v, range) = if loc.get("targetUri").is_some() {
        (&loc["targetUri"], &loc["targetSelectionRange"])
    } else {
        (&loc["uri"], &loc["range"])
    };
    Some(Location {
        path: uri_to_path(uri_v.as_str()?)?,
        line: range["start"]["line"].as_u64()? as usize,
        col: range["start"]["character"].as_u64()? as usize,
    })
}

/// The hints in a `textDocument/inlayHint` result, labels flattened and
/// padded, in server order.
fn parse_inlay_hints(result: &Value) -> Vec<InlayHint> {
    let Some(hints) = result.as_array() else {
        return Vec::new();
    };
    hints
        .iter()
        .filter_map(|h| {
            let label = match &h["label"] {
                Value::String(s) => s.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|p| p["value"].as_str())
                    .collect::<String>(),
                _ => return None,
            };
            let mut label = label;
            if h["paddingLeft"].as_bool().unwrap_or(false) {
                label.insert(0, ' ');
            }
            if h["paddingRight"].as_bool().unwrap_or(false) {
                label.push(' ');
            }
            Some(InlayHint {
                line: h["position"]["line"].as_u64()? as usize,
                col: h["position"]["character"].as_u64()? as usize,
                label,
                kind: h["kind"].as_i64(),
            })
        })
        .collect()
}

/// Builds the type-annotated version of a line by splicing inlay hint labels
/// at their utf-8 columns (the ticket's line-scope hover).
fn splice_inlay_hints(text: &str, result: &Value) -> Option<String> {
    // parameter-name hints (kind 2) add noise, not information — only type
    // hints (kind 1) belong in the annotated line
    let mut inserts: Vec<(usize, String)> = parse_inlay_hints(result)
        .into_iter()
        .filter(|h| h.kind == Some(1))
        .map(|h| (h.col, h.label))
        .collect();
    if inserts.is_empty() {
        return None;
    }
    inserts.sort_by_key(|(c, _)| *c);
    let mut out = String::new();
    let mut last = 0usize;
    for (col, label) in inserts {
        let col = col.min(text.len());
        // a non-boundary column bails out to no annotation at all
        out.push_str(text.get(last..col)?);
        out.push_str(&label);
        last = col;
    }
    out.push_str(text.get(last..)?);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_snippet_removes_placeholders() {
        assert_eq!(strip_snippet("foo($0)"), "foo()");
        assert_eq!(strip_snippet("push(${1:value})"), "push()");
        assert_eq!(strip_snippet("plain"), "plain");
        assert_eq!(strip_snippet("a${1}b$0c"), "abc");
        assert_eq!(strip_snippet(r"cost \$5"), "cost $5");
    }

    #[test]
    fn parse_completion_shapes() {
        // CompletionList with items; snippet reduced to plain
        let list = serde_json::json!({
            "isIncomplete": false,
            "items": [
                { "label": "len", "detail": "usize", "kind": 5 },
                { "label": "push", "insertText": "push($0)", "insertTextFormat": 2, "kind": 2 },
                { "label": "map", "textEdit": { "newText": "map" }, "sortText": "0001" },
            ]
        });
        let items = parse_completion(&list);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "len");
        assert_eq!(items[0].detail.as_deref(), Some("usize"));
        assert_eq!(items[1].insert_text, "push()"); // snippet stripped
        assert_eq!(items[2].insert_text, "map");
        // a bare array is also accepted
        let arr = serde_json::json!([{ "label": "x" }]);
        assert_eq!(parse_completion(&arr).len(), 1);
        // filter/sort default to the label
        assert_eq!(items[0].filter_text, "len");
    }

    #[test]
    fn splices_type_hints_only() {
        let line = "let x = vec![1];";
        let result = serde_json::json!([
            { "position": { "line": 0, "character": 5 },
              "label": ": Vec<i32>", "kind": 1 },
            { "position": { "line": 0, "character": 13 },
              "label": "n:", "kind": 2, "paddingRight": true }
        ]);
        assert_eq!(
            splice_inlay_hints(line, &result).as_deref(),
            Some("let x: Vec<i32> = vec![1];"),
            "type hints splice; parameter-name hints are filtered"
        );
    }

    #[test]
    fn project_root_prefers_outermost_cargo_toml() {
        let base = std::env::temp_dir().join(format!("unei-lsp-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("member/src")).unwrap();
        std::fs::write(base.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(base.join("member/Cargo.toml"), "[package]\n").unwrap();
        let file = base.join("member/src/lib.rs");
        assert_eq!(project_root(&file), Some(base.clone()));
        assert_eq!(project_root(Path::new("/tmp/nowhere/x.rs")), None);
    }
}
