#!/usr/bin/env python3
import json
import sys


def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        name, _, value = line.partition(b":")
        if name.lower() == b"content-length":
            length = int(value.strip())
    if length is None:
        return None
    body = sys.stdin.buffer.read(length)
    return json.loads(body)


def send(message):
    body = json.dumps(message, separators=(",", ":")).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode())
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()


seq = 1
while True:
    request = read_message()
    if request is None:
        break
    command = request.get("command")
    response = {
        "seq": seq,
        "type": "response",
        "request_seq": request.get("seq"),
        "success": True,
        "command": command,
    }
    seq += 1
    if command == "initialize":
        response["body"] = {
            "supportsConfigurationDoneRequest": True,
            "supportsReadMemoryRequest": True,
            "supportsModulesRequest": True,
        }
    elif command == "launch":
        send({
            "seq": seq,
            "type": "event",
            "event": "output",
            "body": {"category": "console", "output": "fake adapter ✓\n"},
        })
        seq += 1
        send({
            "seq": seq,
            "type": "event",
            "event": "stopped",
            "body": {"reason": "entry"},
        })
        seq += 1
    elif command == "threads":
        response["body"] = {"threads": [{"id": 1, "name": "fake"}]}
    elif command == "disconnect":
        send(response)
        break
    send(response)
