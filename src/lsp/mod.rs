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

/// A code action offered by the server, with its (possibly lazy) edit.
#[derive(Debug, Clone)]
pub struct Action {
    pub title: String,
    pub raw: Value,
}

/// Typed events handed to the editor each tick.
pub enum Event {
    DiagnosticsUpdated(PathBuf),
    Hover(Option<String>),
    Definition(Option<Location>),
    Actions(Vec<Action>),
    ActionResolved(Box<Value>),
    ExpandedMacro(Option<(String, String)>),
    InlayLine(Option<String>),
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
    ResolveAction,
    ExpandMacro,
    InlayLine { text: String },
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
                        "definition": {}
                    },
                    "window": { "workDoneProgress": true },
                    "workspace": { "applyEdit": true, "workspaceEdit": { "documentChanges": true } }
                },
                "initializationOptions": {
                    "checkOnSave": true
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
    pub fn inlay_line(&mut self, path: &Path, line: usize, text: String, len: usize) {
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
                "window/workDoneProgress/create"
                | "client/registerCapability"
                | "workspace/semanticTokens/refresh"
                | "workspace/inlayHint/refresh" => {
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
                events.push(Event::InlayLine(splice_inlay_hints(&text, &result)));
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

/// Builds the type-annotated version of a line by splicing inlay hint labels
/// at their utf-8 columns (the ticket's line-scope hover).
fn splice_inlay_hints(text: &str, result: &Value) -> Option<String> {
    let hints = result.as_array()?;
    if hints.is_empty() {
        return None;
    }
    // (col, label) sorted by col; labels may be strings or part arrays
    let mut inserts: Vec<(usize, String)> = hints
        .iter()
        .filter_map(|h| {
            let col = h["position"]["character"].as_u64()? as usize;
            let label = match &h["label"] {
                Value::String(s) => s.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|p| p["value"].as_str())
                    .collect::<String>(),
                _ => return None,
            };
            let pad_left = h["paddingLeft"].as_bool().unwrap_or(false);
            let pad_right = h["paddingRight"].as_bool().unwrap_or(false);
            let mut label = label;
            if pad_left {
                label.insert(0, ' ');
            }
            if pad_right {
                label.push(' ');
            }
            Some((col, label))
        })
        .collect();
    if inserts.is_empty() {
        return None;
    }
    inserts.sort_by_key(|(c, _)| *c);
    let mut out = String::new();
    let mut last = 0usize;
    for (col, label) in inserts {
        let col = col.min(text.len());
        match text.get(last..col) {
            Some(chunk) => out.push_str(chunk),
            None => return None, // not a char boundary: bail to plain hover
        }
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
    fn splices_hints_into_line() {
        let line = "let x = vec![1];";
        let result = serde_json::json!([
            { "position": { "line": 0, "character": 5 }, "label": ": Vec<i32>" },
            { "position": { "line": 0, "character": 5 },
              "label": [ ], "paddingLeft": false }
        ]);
        assert_eq!(
            splice_inlay_hints(line, &result).as_deref(),
            Some("let x: Vec<i32> = vec![1];")
        );
    }

    #[test]
    fn project_root_prefers_outermost_cargo_toml() {
        let base = std::env::temp_dir().join(format!("tailored-lsp-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("member/src")).unwrap();
        std::fs::write(base.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(base.join("member/Cargo.toml"), "[package]\n").unwrap();
        let file = base.join("member/src/lib.rs");
        assert_eq!(project_root(&file), Some(base.clone()));
        assert_eq!(project_root(Path::new("/tmp/nowhere/x.rs")), None);
    }
}
