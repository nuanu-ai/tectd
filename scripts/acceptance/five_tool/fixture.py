#!/usr/bin/env python3
"""Owned temporary PostgreSQL, package, host enrollment, and daemon fixture."""

from __future__ import annotations

import os
import pathlib
import shutil
import signal
import subprocess
import tempfile
import time
import urllib.parse
import uuid
import hashlib
import model_capture

CANDIDATE_PROGRAM_INPUT = (
    "Continuously evolve workspace notification capabilities. Maintain an ongoing backlog that may "
    "include email, SMS, push notifications, and notification analytics. Plan and deliver only the "
    "specific notification increment requested at each continuation; no backlog item is promised "
    "until it is explicitly requested and authorized."
)
CANDIDATE_PROGRAM_FIELDS = {
    "name": "Ongoing workspace notifications",
    "intent": "Continuously evolve useful workspace notification capabilities through separately requested increments.",
    "basis": "An ongoing backlog may contain email, SMS, push, and analytics work; the current continuation supplies the authorized increment.",
    "boundaries": "The Program may cover future email, SMS, push, and analytics capabilities, while each planning window stays within its exact current request.",
    "constraints": "Do not treat backlog presence as current authorization; candidate planning opens no Scope and performs no implementation.",
    "success": "Requested notification increments are durably planned and reviewed as they arrive; future SMS, push, and analytics backlog remains optional and separately authorized.",
}
CANDIDATE_PLANNING_INPUT = (
    "Current requested feature: workspace administrators can choose email delivery frequency and "
    "recipients, see the saved settings after reload, and send one test notification before enabling "
    "delivery. Exclude SMS, push notifications, analytics, and unrelated notification-platform "
    "cleanup from this request. Review and correct this supplied proposed breakdown using the current "
    "TectD methodology and applicable rules: (1) add preference tables, (2) add preference API "
    "endpoints, (3) add settings UI."
)

def collect_model_turn(app, thread_id: str, prompt: str, proof, allow_one_child: bool = False):
    position = len(app.notifications)
    started = app.request(
        "turn/start",
        {"threadId": thread_id, "input": [{"type": "text", "text": prompt}],
         "model": "gpt-5.6-sol", "effort": "medium"},
    )
    turn_id = started["turn"]["id"]
    raw = model_capture.RawEventLog(proof.path)
    capture = {"thread_id": thread_id, "turn_id": turn_id, "model": "gpt-5.6-sol",
               "effort": "medium", "turn_start": started, "lineage_event_items": [],
               "observed_actor_thread_ids": [], "raw_event_log": raw.evidence(),
               "allow_one_child_sol": allow_one_child}
    proof.data["scope_candidate_model_capture"] = capture
    proof.persist()
    items = []
    actors: set[str] = set()
    deadline = time.monotonic() + 600
    terminal = None

    def record(event: dict[str, Any]) -> bool:
        nonlocal terminal
        raw.append(event)
        params = event.get("params", {})
        actor = params.get("threadId")
        if actor is not None:
            actors.add(str(actor))
            capture["observed_actor_thread_ids"] = sorted(actors)
        item = params.get("item")
        meaningful = event.get("method") in {"turn/completed", "turn/failed", "error"}
        if isinstance(item, dict):
            kind = item.get("type")
            status = item.get("status")
            meaningful = meaningful or kind in {"subAgentActivity", "collabAgentToolCall"}
            meaningful = meaningful or (kind == "mcpToolCall" and status in {"completed", "failed"})
            if kind in {"subAgentActivity", "collabAgentToolCall"}:
                capture["lineage_event_items"].append(item)
        if params.get("threadId") == thread_id and (
            params.get("turnId") == turn_id or params.get("turn", {}).get("id") == turn_id
        ):
            if event.get("method") == "item/completed" and isinstance(item, dict):
                items.append(item)
            if event.get("method") == "turn/completed":
                terminal = params["turn"]
        capture["raw_event_log"] = raw.evidence()
        return meaningful

    def drain_ready() -> None:
        while True:
            try:
                app.notifications.append(app._read(0.05))
            except TimeoutError:
                return

    try:
        while time.monotonic() < deadline and terminal is None:
            if position >= len(app.notifications):
                try:
                    app.notifications.append(app._read(30))
                except TimeoutError:
                    continue
            while position < len(app.notifications):
                event = app.notifications[position]
                position += 1
                if record(event):
                    proof.persist()
                    if allow_one_child:
                        model_capture.refresh_lineage(app, thread_id, capture, proof)
        if allow_one_child:
            model_capture.refresh_lineage(app, thread_id, capture, proof)
            drain_ready()
            while position < len(app.notifications):
                record(app.notifications[position])
                position += 1
            model_capture.refresh_lineage(app, thread_id, capture, proof)
            drain_ready()
            while position < len(app.notifications):
                record(app.notifications[position])
                position += 1
    finally:
        capture["raw_event_log"] = raw.evidence()
        raw.close()
        proof.persist()
    capture["terminal"] = terminal
    capture["items"] = items
    proof.persist()
    if terminal is None or terminal.get("status") != "completed":
        raise AssertionError("model turn did not complete")
    if allow_one_child:
        lineage = model_capture.assert_one_sol_child(capture, thread_id)
        model_capture.assert_parent_boundary(capture)
        capture["approved_child"] = lineage
        proof.persist()
        return turn_id, model_capture.child_items(capture)
    return turn_id, items


class Fixture:
    def __init__(self, source: pathlib.Path, postgres_bin: pathlib.Path, keep: bool = False):
        self.source = source.resolve()
        self.pg_bin = postgres_bin.resolve()
        self.keep = keep
        self.root = pathlib.Path(tempfile.mkdtemp(prefix="tectd-5t-"))
        self.private = self.root / "private"
        self.private.mkdir(mode=0o700)
        self.pg_data = self.root / "postgres"
        self.pg_socket = self.private / "postgres-socket"
        self.pg_socket.mkdir()
        self.task = self.root / "task"
        self.task.mkdir()
        self.source_fixture = self.root / "source-fixture"
        self.source_fixture.mkdir()
        self.package_parent = self.root / "package"
        self.host_config = self.private / "host.json"
        self.daemon_socket = self.private / "tectd.sock"
        self.launch_attestation = self.private / "mcp-launch.json"
        self.workspace_key = "native-five-" + uuid.uuid4().hex
        self.port = 55432 + os.getpid() % 1000
        self.daemon: subprocess.Popen[bytes] | None = None
        self.pg_started = False

    def _start_daemon(self) -> None:
        if self.daemon is not None and self.daemon.poll() is None:
            raise RuntimeError("owned daemon is already running")
        if self.daemon_socket.exists():
            raise RuntimeError("owned daemon socket was not removed before startup")
        self.daemon = subprocess.Popen(
            [str(self.binaries / "tectd")],
            env={**os.environ, "TECT_DATABASE_URL": self.runtime_url(), "TECT_SOCKET": str(self.daemon_socket)},
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and not self.daemon_socket.exists() and self.daemon.poll() is None:
            time.sleep(0.03)
        if not self.daemon_socket.exists() or self.daemon.poll() is not None:
            raise RuntimeError("owned daemon did not create its socket")

    def restart_daemon(self) -> dict[str, int]:
        if self.daemon is None or self.daemon.poll() is not None:
            raise RuntimeError("owned daemon is not running")
        old_pid = self.daemon.pid
        old_inode = self.daemon_socket.stat().st_ino
        self.daemon.send_signal(signal.SIGINT)
        self.daemon.wait(timeout=5)
        deadline = time.monotonic() + 5
        while self.daemon_socket.exists() and time.monotonic() < deadline:
            time.sleep(0.03)
        if self.daemon_socket.exists():
            raise RuntimeError("owned daemon did not remove its socket")
        self._start_daemon()
        assert self.daemon is not None
        new_inode = self.daemon_socket.stat().st_ino
        if self.daemon.pid == old_pid or new_inode == old_inode:
            raise RuntimeError("owned daemon restart did not replace process and socket")
        return {
            "pid_before": old_pid,
            "pid_after": self.daemon.pid,
            "socket_inode_before": old_inode,
            "socket_inode_after": new_inode,
        }

    @property
    def package(self) -> pathlib.Path:
        return self.package_parent / "tectd"

    @property
    def launcher(self) -> pathlib.Path:
        return self.package_parent / "launch-mcp.py"

    @property
    def binaries(self) -> pathlib.Path:
        return self.root / "bin"

    @property
    def build_binaries(self) -> pathlib.Path:
        return self.source / "target" / "debug"

    def _run(self, command: list[str], env: dict[str, str] | None = None) -> None:
        result = subprocess.run(command, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
        if result.returncode:
            detail = result.stderr.strip()[-2000:]
            raise RuntimeError(f"owned fixture command failed ({pathlib.Path(command[0]).name}): {detail}")

    def admin_url(self) -> str:
        query = urllib.parse.urlencode({"host": str(self.pg_socket), "port": self.port})
        return f"postgresql:///tect_acceptance?{query}"

    def runtime_url(self) -> str:
        query = urllib.parse.urlencode({"host": str(self.pg_socket), "port": self.port, "user": "tect_runtime"})
        return f"postgresql:///tect_acceptance?{query}"

    def prepare(self) -> None:
        for name in ["tectd", "tectd-mcp", "tect-admin"]:
            if not (self.build_binaries / name).is_file():
                raise FileNotFoundError(f"build-ready binary missing: {self.build_binaries / name}")
        self.binaries.mkdir()
        for name in ["tectd", "tectd-mcp", "tect-admin"]:
            shutil.copy2(self.build_binaries / name, self.binaries / name)
        self._run(["git", "init", "--quiet", str(self.source_fixture)])
        (self.source_fixture / "README.md").write_text("isolated source fixture\n")
        self._run(["git", "-C", str(self.source_fixture), "add", "README.md"])
        self._run(
            [
                str(self.pg_bin / "initdb"),
                "-D",
                str(self.pg_data),
                "--auth=trust",
                "--no-locale",
                "--encoding=UTF8",
            ]
        )
        options = f"-k {self.pg_socket} -h '' -p {self.port}"
        self._run([str(self.pg_bin / "pg_ctl"), "-D", str(self.pg_data), "-o", options, "-w", "start"])
        self.pg_started = True
        psql = [str(self.pg_bin / "psql"), "-X", "-v", "ON_ERROR_STOP=1", "-h", str(self.pg_socket), "-p", str(self.port)]
        self._run(psql + ["-d", "postgres", "-c", "CREATE ROLE tect_runtime LOGIN NOSUPERUSER NOBYPASSRLS"])
        self._run(psql + ["-d", "postgres", "-c", "CREATE DATABASE tect_acceptance"])
        admin_env = {**os.environ, "TECT_ADMIN_DATABASE_URL": self.admin_url()}
        self._run([str(self.binaries / "tect-admin"), "migrate", "--runtime-role", "tect_runtime"], admin_env)
        self._run(
            [
                str(self.binaries / "tect-admin"),
                "enroll",
                "--source-root",
                str(self.root),
                "--setup-root",
                str(self.root),
                "--out",
                str(self.host_config),
            ],
            admin_env,
        )
        self._run(
            [
                "python3",
                str(self.source / "scripts/package-codex-plugin.py"),
                "--binary",
                str(self.binaries / "tectd-mcp"),
                "--output",
                str(self.package_parent),
            ]
        )
        self.launcher.write_text(
            "#!/usr/bin/env python3\n"
            "import hashlib,json,os,pathlib\n"
            f"out=pathlib.Path({str(self.launch_attestation)!r})\n"
            "h=lambda value: hashlib.sha256(value.encode()).hexdigest()\n"
            "data={'cwd':os.getcwd(),'socket_sha256':h(os.environ.get('TECT_SOCKET','')),"
            "'host_config_sha256':h(os.environ.get('TECT_HOST_CONFIG','')),"
            "'workspace_key_sha256':h(os.environ.get('TECT_WORKSPACE_KEY',''))}\n"
            "out.write_text(json.dumps(data,sort_keys=True)+'\\n')\n"
            "os.execv('/bin/sh',['sh','./run.sh'])\n"
        )
        self.launcher.chmod(0o700)
        self._start_daemon()

    def database_fingerprint(self) -> str:
        result = subprocess.run(
            [
                str(self.pg_bin / "pg_dump"),
                "--data-only",
                "--no-owner",
                "--no-privileges",
                "-h",
                str(self.pg_socket),
                "-p",
                str(self.port),
                "tect_acceptance",
            ],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        stable = b"\n".join(
            line for line in result.stdout.splitlines()
            if not line.startswith(b"\\restrict ") and not line.startswith(b"\\unrestrict ")
        )
        return hashlib.sha256(stable).hexdigest()

    def cleanup(self) -> dict[str, bool | str]:
        if self.daemon is not None and self.daemon.poll() is None:
            self.daemon.send_signal(signal.SIGINT)
            try:
                self.daemon.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.daemon.kill()
                self.daemon.wait(timeout=5)
        if self.pg_started:
            subprocess.run(
                [str(self.pg_bin / "pg_ctl"), "-D", str(self.pg_data), "-m", "fast", "-w", "stop"],
                check=False,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
        daemon_stopped = self.daemon is None or self.daemon.poll() is not None
        pg_stopped = not self.pg_started or not (self.pg_data / "postmaster.pid").exists()
        credential = self.host_config
        credential.unlink(missing_ok=True)
        result: dict[str, bool | str] = {
            "daemon_stopped": daemon_stopped,
            "postgres_stopped": pg_stopped,
            "daemon_socket_removed": not self.daemon_socket.exists(),
            "temporary_host_credential_removed": not credential.exists(),
            "real_workspace_agents_mutated": False,
            "persistent_installation_touched": False,
        }
        if self.keep:
            result["fixture_preserved"] = str(self.root)
        else:
            shutil.rmtree(self.root, ignore_errors=True)
            result["fixture_removed"] = not self.root.exists()
        return result
