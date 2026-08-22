#!/usr/bin/env python3
import json
import sys


def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        key, value = line.decode("ascii").split(":", 1)
        headers[key.lower()] = value.strip()
    length = int(headers["content-length"])
    body = sys.stdin.buffer.read(length)
    return json.loads(body)


def send(message):
    body = json.dumps(message, separators=(",", ":")).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode())
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()


while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        send({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {
                "capabilities": {
                    "hoverProvider": True,
                    "definitionProvider": True,
                    "referencesProvider": True,
                    "documentSymbolProvider": True,
                    "documentFormattingProvider": True,
                    "renameProvider": True,
                    "codeActionProvider": True,
                }
            },
        })
    elif method == "shutdown":
        send({"jsonrpc": "2.0", "id": message["id"], "result": None})
    elif method in {"textDocument/didOpen", "textDocument/didChange"}:
        document = message["params"].get("textDocument", {})
        text = document.get("text", "")
        if method.endswith("didChange"):
            changes = message["params"].get("contentChanges", [])
            text = changes[-1].get("text", "") if changes else ""
        diagnostics = []
        if "error" in text:
            diagnostics.append({
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 5}},
                "severity": 1,
                "source": "fake-lsp",
                "message": "fake error",
            })
        send({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {"uri": document["uri"], "version": document.get("version"), "diagnostics": diagnostics},
        })
    elif "id" in message:
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"fake": True}})
