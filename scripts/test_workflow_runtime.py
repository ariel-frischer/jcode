#!/usr/bin/env python3
"""Bounded Linux candidate acceptance. Never submits a model message.

Usage: JCODE_SCRATCH_DIR=<private scratch> python3 scripts/test_workflow_runtime.py <candidate>
Uses only Python's standard library. Creates private HOME/socket/artifact fixtures,
inert auth-shaped placeholders, and a rejecting counting proxy. TUI startup probes
are rejected, not forwarded. Observer-only intervals require zero new requests.
Retains evidence under scratch and terminates only processes/testers it creates.
"""
import json, os, socket, subprocess, sys, tempfile, time, threading, re
from pathlib import Path

binary = Path(sys.argv[1]).resolve()
root = Path(tempfile.mkdtemp(prefix="wf-", dir=os.environ["JCODE_SCRATCH_DIR"]))
home = root / "home"
home.mkdir(mode=0o700)
# Deliberately inert fixture, never a copied credential or a real login.
(home / "openai-auth.json").write_text(json.dumps({"openai_accounts": [{"label": "openai-1", "access_token": "offline-fixture-not-a-credential", "refresh_token": "offline-fixture-not-a-credential", "account_id": "offline-fixture", "expires_at": int(time.time()) + 86400}], "active_openai_account": "openai-1"}))
(home / "openai-auth.json").chmod(0o600)
run = root / "run"
run.mkdir(mode=0o700)
work = root / "work"
work.mkdir()
sock = run / "jcode.sock"
env = {key: os.environ[key] for key in ("PATH", "LANG", "TERM") if key in os.environ}
env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / "config"), JCODE_HOME=str(home), JCODE_RUNTIME_DIR=str(run), JCODE_SOCKET=str(sock), JCODE_DEBUG_CONTROL="1", JCODE_NO_TELEMETRY="1", JCODE_NO_UPDATE="1", JCODE_WORKFLOW_ENABLED="true", JCODE_WORKFLOW_AUTOSPEC_ENABLED="true", JCODE_WORKFLOW_POLL_SECONDS="1", JCODE_WORKFLOW_QUIET_SECONDS="30", HTTP_PROXY="http://127.0.0.1:9", HTTPS_PROXY="http://127.0.0.1:9", ALL_PROXY="http://127.0.0.1:9", NO_PROXY="")
(root / "identity.json").write_text(json.dumps({"binary": str(binary), "root": str(root)}))
env["TMPDIR"] = str(root)
network_attempts = []
proxy = socket.socket()
proxy.bind(("127.0.0.1", 0))
proxy.listen()
proxy.settimeout(.2)
proxy_done = threading.Event()
def reject_network():
    while not proxy_done.is_set():
        try: connection, _ = proxy.accept()
        except socket.timeout: continue
        with connection:
            connection.settimeout(1)
            try: line = connection.recv(4096).split(b"\r\n", 1)[0].decode(errors="replace")
            except socket.timeout: line = "no request data"
            network_attempts.append(line)
            connection.sendall(b"HTTP/1.1 503 Offline fixture\r\nContent-Length: 0\r\n\r\n")
thread = threading.Thread(target=reject_network, daemon=True)
thread.start()
for key in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"):
    env[key] = "http://127.0.0.1:" + str(proxy.getsockname()[1])
print("runtime", root, flush=True)
class Client:
    def __init__(self, target=None, enabled=True, endpoint=None):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.settimeout(8)
        self.sock.connect(str(endpoint or sock))
        self.buffer = b""
        request = {"type": "subscribe", "id": 1, "working_dir": str(work)}
        if enabled: request["workflow_progress"] = True
        if target: request["target_session_id"] = target
        self.sock.sendall((json.dumps(request) + "\n").encode())
        self.session = self.until(lambda e: e.get("type") == "session")["session_id"]
    def read(self):
        while b"\n" not in self.buffer:
            data = self.sock.recv(65536)
            if not data: raise EOFError("server disconnected")
            self.buffer += data
        line, self.buffer = self.buffer.split(b"\n", 1)
        return json.loads(line)
    def until(self, predicate, seconds=12):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            event = self.read()
            if predicate(event): return event
        raise AssertionError("expected event did not arrive")
    def workflows(self, predicate=lambda w: bool(w)):
        return self.until(lambda e: e.get("type") == "workflow_status" and predicate(e["workflows"]))["workflows"]
    def close(self): self.sock.close()

def debug(command, session=None, runtime=run):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(20)
        connection.connect(str(runtime / "jcode-debug.sock"))
        connection.sendall((json.dumps({"type": "debug_command", "id": 99, "command": command, "session_id": session}) + "\n").encode())
        result = json.loads(connection.makefile("rb").readline())
        assert result.get("ok"), result
        return result["output"]

def tester_command(tester, command):
    cmd = root / ("jcode_debug_cmd_" + tester)
    response = root / ("jcode_debug_response_" + tester)
    response.unlink(missing_ok=True)
    temp = cmd.with_suffix(".pending")
    temp.write_text(command)
    temp.replace(cmd)
    end = time.monotonic() + 10
    while time.monotonic() < end:
        if response.exists() and response.stat().st_size:
            text = response.read_text()
            response.unlink(missing_ok=True)
            return text
        time.sleep(.05)
    raise AssertionError("tester response timeout: " + command)

def tasks(status="InProgress"):
    (work / "tasks.yaml").write_text("phases:\n  - number: 1\n    title: Build\n    tasks:\n      - id: T001\n        title: Offline producer\n        status: " + status + "\n")

def register(session, directory=work):
    return debug("tool:bg " + json.dumps({"action": "observe", "observation": {"working_dir": str(directory), "tasks_file": "tasks.yaml", "status_file": "status.json", "label": "Offline acceptance"}}), session)

clients = []
testers = []
extra_processes = []
with (root / "server.log").open("w") as log:
    process = subprocess.Popen([str(binary), "serve"], env=env, cwd=work, stdout=log, stderr=log, start_new_session=True)
    try:
        for _ in range(150):
            if sock.exists(): break
            if process.poll() is not None: raise RuntimeError("server exited: " + (root / "server.log").read_text()[-3000:])
            time.sleep(.1)
        assert Path(f"/proc/{process.pid}/exe").resolve() == binary
        a = Client(); clients.append(a)
        time.sleep(2)
        observer_network_baseline = len(network_attempts)
        print("session", a.session, flush=True)
        tasks()
        (work / "status.json").write_text('{"state":"running"}')
        print("register", register(a.session), flush=True)
        initial = a.workflows()
        print("initial", json.dumps(initial), flush=True)
        tasks("Completed")
        changed = a.workflows(lambda w: bool(w) and w[0].get("completed") == 1)
        print("producer", json.dumps(changed), flush=True)
        (work / "status.json").write_text('{"state":"failed","error_code":"insufficient_quota","message":"MUST_NOT_DISPLAY_RAW"}')
        failed = a.workflows(lambda w: "Credits exhausted" in json.dumps(w))
        assert "MUST_NOT_DISPLAY_RAW" not in json.dumps(failed)
        print("credits", json.dumps(failed), flush=True)
        time.sleep(1.1)
        (work / "tasks.yaml").write_text("malformed: [")
        time.sleep(1.2)
        sticky = a.workflows(lambda w: "Credits exhausted" in json.dumps(w) and w[0].get("detail") is not None)
        b = Client(); clients.append(b)
        assert b.workflows(lambda w: not w) == []
        resumed = Client(a.session); clients.append(resumed)
        assert resumed.session == a.session
        restored = resumed.workflows(lambda w: "Credits exhausted" in json.dumps(w))
        resumed.sock.sendall((json.dumps({"type": "resume_session", "id": 2, "session_id": b.session}) + "\n").encode())
        resumed.until(lambda event: event.get("type") == "history" and event.get("session_id") == b.session)
        assert resumed.workflows(lambda w: not w) == []
        resumed.sock.sendall((json.dumps({"type": "resume_session", "id": 3, "session_id": a.session}) + "\n").encode())
        resumed.until(lambda event: event.get("type") == "history" and event.get("session_id") == a.session)
        resumed.workflows(lambda w: "Credits exhausted" in json.dumps(w))
        legacy = Client(a.session, False); clients.append(legacy)
        legacy.sock.settimeout(2)
        try:
            while True:
                assert legacy.read().get("type") != "workflow_status"
        except socket.timeout: pass
        assert len(network_attempts) == observer_network_baseline, "observer initiated network request"
        print("PASS producer, sticky credits, owner isolation, reconnect, legacy; zero observer requests", flush=True)
        tasks()
        for cols, rows in [(40,30), (80,30), (120,30), (80,10)]:
            directory = root / f"tester-{cols}-{rows}"
            directory.mkdir()
            (directory / "tasks.yaml").write_text((work / "tasks.yaml").read_text())
            (directory / "status.json").write_text('{"state":"failed","error_code":"insufficient_quota"}')
            spawn = json.loads(debug("tester:spawn " + json.dumps({"binary": str(binary), "cwd": str(directory), "cols": cols, "rows": rows})))
            tester = spawn["id"]
            testers.append(tester)
            time.sleep(2)
            mapping = json.loads(debug("clients:map"))["clients"]
            session = next(c["session_id"] for c in mapping if c["working_dir"] == str(directory))
            tester_command(tester, "keys:esc")
            register(session, directory)
            assert "enabled" in tester_command(tester, "debug-enable")
            tester_command(tester, "set_input:First line\nSecond line")
            time.sleep(2)
            state = json.loads(tester_command(tester, "state"))
            assert not state["processing"] and state["messages"] == 0
            frame = tester_command(tester, "screen-json")
            (root / f"frame-{cols}-{rows}.json").write_text(frame)
            captured = json.loads(frame)
            assert captured["terminal_size"] == [cols, rows]
            assert captured["rendered_text"]["input_area"] == "First line\nSecond line"
            output = (root / ("jcode_tester_stdout_" + tester)).read_text(errors="replace")
            plain = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", output)
            assert ("Workflows" in plain) == (rows >= 30)
            if rows >= 30:
                assert "Failed" in plain and "checkpoint" in plain
                assert "Creditsexhausted" in plain.replace(" ", "")
            print("PASS FRAME", cols, rows, "multiline input preserved; panel bounded", flush=True)
            debug("tester:" + tester + ":stop")
            testers.remove(tester)
        time.sleep(2)
        observer_network_baseline = len(network_attempts)
        # An actual unread socket cannot stall the healthy observer/owner feed.
        # Kernel buffering is bounded separately from the watch's tested 10k coalescing.
        for index in range(48):
            directory = root / f"pressure-{index}"; directory.mkdir()
            (directory / "tasks.yaml").write_text((root / "tester-80-30" / "tasks.yaml").read_text())
            (directory / "status.json").write_text('{"state":"running"}')
            register(a.session, directory)
        slow = Client(a.session); clients.append(slow)
        slow.sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 1024)
        tasks()
        for index in range(24):
            (work / "status.json").write_text(json.dumps({"state": "retrying" if index % 2 == 0 else "running"}))
            a.workflows(lambda w: w and w[0]["health"] == ("waiting" if index % 2 == 0 else "running"))
        (work / "status.json").write_text('{"state":"completed"}')
        newest = a.workflows(lambda w: w and w[0]["health"] == "completed")
        try:
            recovered = slow.workflows(lambda w: w and w[0]["health"] == "completed")
            slow_result = "recovered latest snapshot"
        except (EOFError, ConnectionResetError, BrokenPipeError):
            reconnected = Client(a.session); clients.append(reconnected)
            recovered = reconnected.workflows(lambda w: w and w[0]["health"] == "completed")
            slow_result = "bounded writer disconnected; reconnect restored latest"
        print("SLOW CLIENT", slow_result, flush=True)
        assert recovered[0]["id"] == newest[0]["id"]
        print("PASS unread client recovery and healthy-client progress", flush=True)
        assert len(network_attempts) == observer_network_baseline, "passive polling initiated network request"
        # A separate daemon with the same HOME must fail observation, not write stale state.
        second_run = root / "run-second"; second_run.mkdir()
        second_sock = second_run / "jcode.sock"
        second_env = dict(env, JCODE_RUNTIME_DIR=str(second_run), JCODE_SOCKET=str(second_sock))
        with (root / "second-server.log").open("w") as second_log:
            second = subprocess.Popen([str(binary), "serve"], env=second_env, cwd=work, stdout=second_log, stderr=second_log, start_new_session=True)
        extra_processes.append(second)
        for _ in range(100):
            if second_sock.exists(): break
            assert second.poll() is None, "second daemon exited"
            time.sleep(.1)
        competitor = Client(endpoint=second_sock); clients.append(competitor)
        locked = competitor.workflows(lambda w: "observer_error" in json.dumps(w))
        assert "unavailable" in json.dumps(locked).lower()
        print("PASS cross-process observer lock", json.dumps(locked), flush=True)
        for owned in [a.session, b.session]:
            assert json.loads(debug("history", owned)) == []
            print("USAGE", debug("usage", owned), flush=True)
        print("NETWORK ATTEMPTS", network_attempts, flush=True)
        # TUI onboarding/usage probes attempted only rejected proxy CONNECTs.
        # Observer-only intervals above assert zero additional transport requests.
        print("PASS zero observer requests, zero model tokens and no main-session messages", flush=True)
        disabled_home = root / "disabled-home"; disabled_home.mkdir(mode=0o700)
        (disabled_home / "openai-auth.json").write_bytes((home / "openai-auth.json").read_bytes())
        (disabled_home / "openai-auth.json").chmod(0o600)
        disabled_run = root / "disabled-run"; disabled_run.mkdir()
        disabled_sock = disabled_run / "jcode.sock"
        disabled_env = dict(env, HOME=str(disabled_home), JCODE_HOME=str(disabled_home), XDG_CONFIG_HOME=str(disabled_home / "config"), JCODE_RUNTIME_DIR=str(disabled_run), JCODE_SOCKET=str(disabled_sock), JCODE_WORKFLOW_ENABLED="false")
        with (root / "disabled-server.log").open("w") as disabled_log:
            disabled = subprocess.Popen([str(binary), "serve"], env=disabled_env, cwd=work, stdout=disabled_log, stderr=disabled_log, start_new_session=True)
        extra_processes.append(disabled)
        for _ in range(100):
            if disabled_sock.exists(): break
            assert disabled.poll() is None
            time.sleep(.1)
        disabled_client = Client(endpoint=disabled_sock); clients.append(disabled_client)
        disabled_client.sock.settimeout(2)
        try:
            while True: assert disabled_client.read().get("type") != "workflow_status"
        except socket.timeout: pass
        assert not (disabled_home / "workflow").exists()
        print("PASS disabled server: no workflow messages or registry IO", flush=True)
        (root / "evidence.json").write_text(json.dumps({"initial": initial, "changed": changed, "failed": failed, "sticky": sticky, "restored": restored, "slow_client": slow_result, "lock": locked, "observer_request_delta": 0, "blocked_startup_requests": network_attempts, "disabled_registry_absent": True}, indent=2))
    finally:
        for tester in testers:
            try: debug("tester:" + tester + ":stop")
            except (OSError, AssertionError) as error:
                print("Tester cleanup error:", error, file=sys.stderr)
        for client in clients: client.close()
        for extra in extra_processes:
            extra.terminate()
            try: extra.wait(timeout=5)
            except subprocess.TimeoutExpired: extra.kill(); extra.wait()
        proxy_done.set()
        thread.join(timeout=2)
        proxy.close()
        process.terminate()
        try: process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
