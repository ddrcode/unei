#!/usr/bin/env python3
"""A canned rust-analyzer stand-in for the LSP integration tests.

Speaks Content-Length framing on stdio. On didOpen it publishes one error
and one warning diagnostic; hover, definition, code actions, inlay hints
and macro expansion return fixed data the tests assert on.
"""
import json
import sys


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
    elif method == "textDocument/didOpen":
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
        ]})
    elif method == "textDocument/inlayHint":
        send({"jsonrpc": "2.0", "id": mid, "result": [
            {"position": {"line": 0, "character": 5},
             "label": ": Vec<i32>"}]})
    elif method == "rust-analyzer/expandMacro":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "name": "demo", "expansion": "fn expanded() {}\n"}})
    elif method == "shutdown":
        send({"jsonrpc": "2.0", "id": mid, "result": None})
    elif method == "exit":
        break
