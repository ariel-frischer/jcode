#!/usr/bin/env python3
import json
import socket
import sys


def read_message(reader):
    length = None
    while True:
        line = reader.readline()
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
    body = reader.read(length)
    return json.loads(body)


def send(message, writer):
    body = json.dumps(message, separators=(",", ":")).encode()
    writer.write(f"Content-Length: {len(body)}\r\n\r\n".encode())
    writer.write(body)
    writer.flush()


def serve(reader, writer):
    seq = 1
    while True:
        request = read_message(reader)
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
            }, writer)
            seq += 1
            send({
                "seq": seq,
                "type": "event",
                "event": "stopped",
                "body": {"reason": "entry"},
            }, writer)
            seq += 1
        elif command == "threads":
            response["body"] = {"threads": [{"id": 1, "name": "fake"}]}
        elif command == "disconnect":
            send(response, writer)
            break
        send(response, writer)


if len(sys.argv) >= 3 and sys.argv[1] == "--tcp":
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
        server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server.bind(("127.0.0.1", int(sys.argv[2])))
        server.listen(1)
        connection, _ = server.accept()
    with connection:
        with connection.makefile("rb") as reader, connection.makefile("wb") as writer:
            serve(reader, writer)
elif len(sys.argv) >= 3 and sys.argv[1] == "--unix":
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
        server.bind(sys.argv[2])
        server.listen(1)
        connection, _ = server.accept()
        with connection:
            with connection.makefile("rb") as reader, connection.makefile("wb") as writer:
                serve(reader, writer)
else:
    serve(sys.stdin.buffer, sys.stdout.buffer)
