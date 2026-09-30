"""Controlled fresh S05 App Server case; inference requires explicit --execute.

The fixed Owner-selected route is Luna/xhigh for a routine JSON marker task.
Requested/recommended are absent; this is not a JEV recommendation or a native
agents receipt. Marker correctness never establishes actual serving identity.
No arbitrary commands, models, fallbacks, prompts or working directories can be
supplied through this CLI. Production composition uses normal local auth and
the fixed isolation profile; tests patch composition and remain offline fixtures.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

from scripts.codex_app_server_observer import AppServerObserver, AppServerReceipt, ExecutionIntent
from scripts.codex_app_server_profile import build_launch_profile
from scripts.codex_app_server_rpc import OwnedAppServerRpc
from scripts.codex_route_catalogue import development_catalogue, select_route


SELECTED_ROUTE = "codex-app-server-routine-luna-xhigh-v1"
MODEL, EFFORT, PURPOSE = "gpt-6-luna", "xhigh", "routine"


class CaseRejected(ValueError):
    """The fixed case or its observed marker does not match the prepared input."""


def _json(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False)


def _identifier(value: str, field: str) -> str:
    if not isinstance(value, str) or re.fullmatch(r"[A-Za-z0-9_-]{1,128}", value) is None:
        raise CaseRejected(f"{field} must contain 1-128 ASCII letters, numbers, underscores or hyphens")
    return value


def marker_json(case_id: str) -> str:
    _identifier(case_id, "case id")
    return _json({"kind": "S05_APP_SERVER_MARKER_V1", "case_id": case_id,
                  "selected_route_id": SELECTED_ROUTE,
                  "catalogue_sha256": development_catalogue().digest})


def fixed_prompt(case_id: str) -> str:
    return ("Perform this bounded no-tools JSON echo task. Return exactly the single JSON object "
            "below, with no Markdown, commentary or additional fields. Do not use tools, web, "
            "files, MCP, plugins or agents. The object is a supplied case marker, not a claim "
            "about your model identity.\n" + marker_json(case_id))


@dataclass(frozen=True)
class PreparedCase:
    case_id: str
    marker_json: str
    intent: ExecutionIntent


def prepare_case(case_id: str, invocation_key: str, cwd: str) -> PreparedCase:
    """Pure deterministic preparation; no host calls, files or model choices."""
    _identifier(invocation_key, "invocation key")
    catalogue = development_catalogue()
    prompt = fixed_prompt(case_id)
    prompt_digest = hashlib.sha256(prompt.encode("utf-8")).hexdigest()
    selection = select_route(route_id=SELECTED_ROUTE, model=MODEL, effort=EFFORT,
                             purpose=PURPOSE, task_input_digest=prompt_digest,
                             catalogue_version=catalogue.version, catalogue_digest=catalogue.digest)
    return PreparedCase(case_id, marker_json(case_id),
                        ExecutionIntent(selection, prompt, invocation_key, cwd))


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise CaseRejected("duplicate marker key")
        result[key] = value
    return result


def evaluate_marker(case: PreparedCase, receipt: AppServerReceipt) -> dict[str, Any]:
    """Check observed payload semantics; this function does not authenticate receipts.

    Production calls this only on its owned observer's direct return. Offline
    fixtures can demonstrate marker validation but cannot report live acceptance.
    """
    if type(receipt) is not AppServerReceipt:
        raise CaseRejected("immutable direct observer receipt required")
    capture_complete = getattr(receipt, "notification_capture_complete", False) is True
    result = {"case_id": case.case_id, "host_kind": "APP_SERVER",
              "status": "configured_case_rejected", "marker_matches": False,
              "evidence_kind": receipt.evidence_kind,
              "notification_capture_complete": capture_complete,
              "selected": asdict(case.intent.selection), "requested": None, "recommended": None,
              "dispatched_configured": None if receipt.dispatched_configured is None else asdict(receipt.dispatched_configured),
              "observed_actual": None, "prompt_sha256": case.intent.prompt_digest,
              "thread_id": receipt.thread_id, "turn_id": receipt.turn_id,
              "observer_status": receipt.status, "failure": receipt.failure}
    if (receipt.host_kind != "APP_SERVER" or
            receipt.intent_digest != case.intent.digest or receipt.selection != case.intent.selection or
            receipt.prompt_digest != case.intent.prompt_digest or receipt.invocation_key != case.intent.invocation_key or
            receipt.requested is not None or receipt.recommended is not None or
            receipt.observed_actual is not None or receipt.status != "completed_configured_route" or
            receipt.terminal_outcome != "completed" or not receipt.thread_id or not receipt.turn_id or
            receipt.dispatched_configured is None or
            (receipt.dispatched_configured.model, receipt.dispatched_configured.provider,
             receipt.dispatched_configured.effort) != (MODEL, "openai", EFFORT)):
        result["failure"] = "owned observer did not establish this configured case"
        return result
    if receipt.evidence_kind not in {"owned_stdio", "offline_fixture"}:
        result["failure"] = "unknown observer evidence kind"
        return result
    if receipt.evidence_kind == "owned_stdio" and not capture_complete:
        result["failure"] = "owned observer notification capture is incomplete"
        return result
    try:
        events = json.loads(receipt.events_json)
        reads = [event["response"] for event in events if isinstance(event, dict)
                 and event.get("method") == "thread/read" and "response" in event]
        if len(reads) != 1:
            raise CaseRejected("exactly one persisted readback required")
        thread = reads[0]["thread"]
        if thread["id"] != receipt.thread_id:
            raise CaseRejected("marker readback thread mismatch")
        turns = [turn for turn in thread["turns"] if turn["id"] == receipt.turn_id]
        if len(turns) != 1:
            raise CaseRejected("marker readback turn mismatch")
        answers = [item["text"] for item in turns[0]["items"] if item.get("type") == "agentMessage"
                   and isinstance(item.get("text"), str) and item["text"].strip()]
        if len(answers) != 1:
            raise CaseRejected("exactly one nonempty persisted marker answer required")
        marker = json.loads(answers[0], object_pairs_hook=_unique_object)
        if not isinstance(marker, dict) or marker != json.loads(case.marker_json):
            raise CaseRejected("persisted answer differs from fixed case marker")
        result["marker_matches"] = True
        result["failure"] = None
        result["status"] = ("completed_configured_case" if receipt.evidence_kind == "owned_stdio"
                            else "completed_offline_case")
    except (CaseRejected, ValueError, TypeError, KeyError, IndexError) as error:
        result["failure"] = str(error) if isinstance(error, CaseRejected) else "invalid persisted marker evidence"
    return result


def _private_directory(directory: Path) -> int:
    if not directory.is_absolute():
        raise CaseRejected("evidence directory must be an exact absolute path")
    try:
        directory.mkdir(mode=0o700)
    except FileExistsError:
        pass
    fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    info = os.fstat(fd)
    if info.st_uid != os.getuid() or not stat.S_ISDIR(info.st_mode) or info.st_mode & 0o077:
        os.close(fd)
        raise CaseRejected("evidence directory must be owner-only")
    return fd


def _write_evidence(directory_fd: int, name: str, value: Any) -> None:
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                 0o600, dir_fd=directory_fd)
    with os.fdopen(fd, "w", encoding="utf-8") as stream:
        stream.write(_json(value) + "\n")
        stream.flush()
        os.fsync(stream.fileno())
    os.fsync(directory_fd)


def execute_case(*, case_id: str, invocation_key: str, evidence_dir: Path) -> dict[str, Any]:
    """Explicit production invocation only; no retry or alternate route exists."""
    _identifier(case_id, "case id")
    _identifier(invocation_key, "invocation key")
    directory_fd = _private_directory(evidence_dir)
    prefix = f"APP_SERVER-{case_id}"
    try:
        profile = build_launch_profile(MODEL, EFFORT)
        with tempfile.TemporaryDirectory(prefix="codex-s05-case-") as temporary_cwd:
            case = prepare_case(case_id, invocation_key, str(Path(temporary_cwd).resolve()))
            _write_evidence(directory_fd, prefix + "-INTENT.json", {
                "host_kind": "APP_SERVER", "case_id": case_id, "marker_json": case.marker_json,
                "intent": asdict(case.intent), "intent_digest": case.intent.digest,
                "prompt_sha256": case.intent.prompt_digest, "launch_profile_digest": profile.digest,
                "configured_mcp_count_disabled": profile.configured_mcp_count,
                "configured_plugin_count_disabled": profile.configured_plugin_count})
            # Fixed profile and normal local auth only. The temporary cwd has no
            # project files or instructions and is removed after this owned child.
            try:
                with OwnedAppServerRpc(profile.argv, case.intent.cwd) as rpc:
                    rpc.initialize({"name": "s05-controlled-app-server-case", "version": "1.0.0"})
                    observer = AppServerObserver(rpc, ledger_dir=evidence_dir / "ledger")
                    receipt = observer.run_once(case.intent, completion_timeout=60)
            except Exception as error:
                result = {"case_id": case_id, "host_kind": "APP_SERVER",
                          "status": "case_failed_without_retry", "error_type": type(error).__name__,
                          "receipt_available": False, "requested": None, "recommended": None,
                          "selected": asdict(case.intent.selection), "dispatched_configured": None,
                          "observed_actual": None, "prompt_sha256": case.intent.prompt_digest}
                _write_evidence(directory_fd, prefix + "-RESULT.json", result)
                return result
            _write_evidence(directory_fd, prefix + "-RECEIPT.json", asdict(receipt))
            result = evaluate_marker(case, receipt)
            _write_evidence(directory_fd, prefix + "-RESULT.json", result)
            return result
    finally:
        os.close(directory_fd)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case-id", required=True)
    parser.add_argument("--invocation-key", required=True)
    parser.add_argument("--evidence-dir", required=True, type=Path)
    parser.add_argument("--execute", action="store_true", help="explicitly invoke the one fixed model task")
    args = parser.parse_args(argv)
    try:
        _identifier(args.case_id, "case id")
        _identifier(args.invocation_key, "invocation key")
        if not args.evidence_dir.is_absolute():
            raise CaseRejected("evidence directory must be an exact absolute path")
        if not args.execute:
            prompt = fixed_prompt(args.case_id)
            print(_json({"status": "preview_only", "host_kind": "APP_SERVER",
                         "selected_route": SELECTED_ROUTE, "model": MODEL, "effort": EFFORT,
                         "purpose": PURPOSE, "requested": None, "recommended": None,
                         "observed_actual": None, "catalogue_sha256": development_catalogue().digest,
                         "prompt": prompt, "prompt_sha256": hashlib.sha256(prompt.encode()).hexdigest(),
                         "marker_json": marker_json(args.case_id)}))
            return 0
        result = execute_case(case_id=args.case_id, invocation_key=args.invocation_key,
                              evidence_dir=args.evidence_dir)
        print(_json(result))
        return 0 if result["status"] == "completed_configured_case" else 1
    except Exception as error:
        # Keep possibly sensitive server/config exception contents out of stdout.
        print(_json({"status": "case_failed_without_retry", "host_kind": "APP_SERVER",
                     "error_type": type(error).__name__, "observed_actual": None}))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
