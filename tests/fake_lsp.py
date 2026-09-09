#!/usr/bin/env python3
"""A canned rust-analyzer stand-in for the LSP integration tests.

Speaks Content-Length framing on stdio. On didOpen it publishes one error
and one warning diagnostic; hover, definition, code actions, inlay hints
and macro expansion return fixed data the tests assert on.
"""
import json
import os
import sys

# Every didOpen/didChange is appended (one JSON object per line) to the file
# named by UNEI_FAKE_LSP_LOG, so tests can assert what text the server holds.
LOG = os.environ.get("UNEI_FAKE_LSP_LOG")


def log(method, params):
    if not LOG:
        return
    doc = params.get("textDocument", {})
    text = doc.get("text")
    if text is None:
        changes = params.get("contentChanges", [])
        text = changes[-1].get("text") if changes else None
    with open(LOG, "a") as f:
        f.write(json.dumps({"method": method, "uri": doc.get("uri"),
                            "version": doc.get("version"), "text": text}) + "\n")


def send(msg):
    body = json.dumps(msg).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode())
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()


def read():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":")[1])
    if length is None:
        return None
    return json.loads(sys.stdin.buffer.read(length))


while True:
    msg = read()
    if msg is None:
        break
    method = msg.get("method", "")
    mid = msg.get("id")

    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "capabilities": {"positionEncoding": "utf-8"}}})
    elif method == "textDocument/didChange":
        log(method, msg["params"])
    elif method == "textDocument/didOpen":
        log(method, msg["params"])
        uri = msg["params"]["textDocument"]["uri"]
        send({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
              "params": {"uri": uri, "diagnostics": [
                  {"range": {"start": {"line": 0, "character": 3},
                             "end": {"line": 0, "character": 7}},
                   "severity": 1, "message": "bad thing"},
                  {"range": {"start": {"line": 1, "character": 0},
                             "end": {"line": 1, "character": 4}},
                   "severity": 2, "message": "iffy thing"},
              ]}})
    elif method == "textDocument/hover":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "contents": {"kind": "markdown", "value": "fn main()\n\ndocs here"}}})
    elif method == "textDocument/definition":
        uri = msg["params"]["textDocument"]["uri"]
        send({"jsonrpc": "2.0", "id": mid, "result": [
            {"uri": uri, "range": {"start": {"line": 2, "character": 4},
                                   "end": {"line": 2, "character": 8}}}]})
    elif method == "textDocument/codeAction":
        uri = msg["params"]["textDocument"]["uri"]
        send({"jsonrpc": "2.0", "id": mid, "result": [
            {"title": "replace first word", "edit": {"changes": {uri: [
                {"range": {"start": {"line": 0, "character": 0},
                           "end": {"line": 0, "character": 2}},
                 "newText": "FIXED"}]}}},
            {"title": "do nothing"},
            # the test document has 4 lines; line 4 is the end of the file
            # (after the final newline) — a legal LSP position for an append
            {"title": "append tail", "edit": {"changes": {uri: [
                {"range": {"start": {"line": 4, "character": 0},
                           "end": {"line": 4, "character": 0}},
                 "newText": "// tail\n"}]}}},
        ]})
    elif method == "textDocument/inlayHint":
        rng = msg["params"]["range"]
        if rng["end"]["line"] > rng["start"]["line"]:
            # the document-wide request of the compiler's-eye view (#93):
            # one hint of every shape over the 4-line fixture
            send({"jsonrpc": "2.0", "id": mid, "result": [
                {"position": {"line": 0, "character": 7},
                 "label": "<'a>"},                                  # lifetime
                {"position": {"line": 1, "character": 5},
                 "label": [{"value": ": Vec<"}, {"value": "i32"}, {"value": ">"}],
                 "kind": 1},                                         # type, in parts
                {"position": {"line": 1, "character": 13},
                 "label": "n:", "kind": 2, "paddingRight": True},    # parameter
                {"position": {"line": 1, "character": 16},
                 "label": "Vec<i32>", "paddingLeft": True},          # chaining, at EOL
                {"position": {"line": 3, "character": 1},
                 "label": "fn main", "paddingLeft": True}]})         # closing brace
        else:
            send({"jsonrpc": "2.0", "id": mid, "result": [
                {"position": {"line": 0, "character": 5},
                 "label": ": Vec<i32>", "kind": 1},
                {"position": {"line": 0, "character": 8},
                 "label": "noisy:", "kind": 2}]})
    elif method == "textDocument/references":
        uri = msg["params"]["textDocument"]["uri"]
        other = uri.rsplit("/", 1)[0] + "/other.rs"
        # the name on line 1 of the fixture plus its twin in src/other.rs
        send({"jsonrpc": "2.0", "id": mid, "result": [
            {"uri": other, "range": {"start": {"line": 0, "character": 3},
                                     "end": {"line": 0, "character": 8}}},
            {"uri": uri, "range": {"start": {"line": 1, "character": 3},
                                   "end": {"line": 1, "character": 8}}}]})
    elif method == "textDocument/rename":
        uri = msg["params"]["textDocument"]["uri"]
        name = msg["params"]["newName"]
        if name == "reserved":
            send({"jsonrpc": "2.0", "id": mid, "error": {
                "code": -32602, "message": "Cannot rename a reserved name"}})
        else:
            # a two-file edit: `main` on line 1 of the fixture, `other` in
            # src/other.rs (which the editor has to open to apply)
            other = uri.rsplit("/", 1)[0] + "/other.rs"
            send({"jsonrpc": "2.0", "id": mid, "result": {"changes": {
                uri: [{"range": {"start": {"line": 1, "character": 3},
                                 "end": {"line": 1, "character": 7}},
                       "newText": name}],
                other: [{"range": {"start": {"line": 0, "character": 3},
                                   "end": {"line": 0, "character": 8}},
                         "newText": name}]}}})
    elif method == "textDocument/signatureHelp":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "signatures": [{"label": "fn vec_of(n: usize) -> Vec<i32>"}],
            "activeSignature": 0}})
    elif method == "rust-analyzer/expandMacro":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "name": "demo", "expansion": "fn expanded() {}\n"}})
    elif method == "shutdown":
        send({"jsonrpc": "2.0", "id": mid, "result": None})
    elif method == "exit":
        break
