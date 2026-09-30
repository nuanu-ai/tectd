"""Fixed one-use Owner-approved S05 case; only --execute enables inference.

This file is trusted composition data, not cryptographic human authentication.
Root invocation enforces the current human approval; the fixed case and durable
exclusive INTENT fence constrain it. Normal development routes are unchanged.
No model choice, fallback, case, key, directory or command override is exposed.
Offline test composition is explicitly labelled and is never live proof.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

from scripts.codex_app_server_case import CaseRejected, _json, _private_directory, _write_evidence, _unique_object
from scripts.codex_app_server_observer import AppServerObserver, AppServerReceipt, ExecutionIntent
from scripts.codex_app_server_profile import build_one_off_launch_profile
from scripts.codex_app_server_rpc import OwnedAppServerRpc
from scripts.codex_route_catalogue import (PERSISTED_CASE_ID, PERSISTED_INVOCATION_KEY,
    persisted_catalogue, persisted_marker_json, persisted_prompt, select_persisted_route)


CASE_ID = PERSISTED_CASE_ID
INVOCATION_KEY = PERSISTED_INVOCATION_KEY
EVIDENCE_DIR = Path("/Users/tony/Work/Projects/nuanu-ai-lab/artifacts/jev-live-eval-20260919/"
                    "tectd-v2-active-jev-implementation-plan-20260921/evidence/"
                    "s05-appserver-host-observer/persisted-luna56-c39e8241")
MODEL, EFFORT = "gpt-5.6-luna", "xhigh"


@dataclass(frozen=True)
class PreparedPersistedCase:
    case_id: str
    marker_json: str
    intent: ExecutionIntent


def prepare_persisted_case(cwd: str) -> PreparedPersistedCase:
    """Pure preparation; fixed case/key/prompt/pair only."""
    prompt = persisted_prompt()
    selection = select_persisted_route(task_input_digest=hashlib.sha256(prompt.encode()).hexdigest())
    return PreparedPersistedCase(CASE_ID, persisted_marker_json(),
                              ExecutionIntent(selection, prompt, INVOCATION_KEY, cwd, ephemeral_thread=False))


def evaluate_marker(case: PreparedPersistedCase, receipt: AppServerReceipt) -> dict[str, Any]:
    """Semantic check of a direct return; caller values cannot authenticate a host."""
    if type(case) is not PreparedPersistedCase or type(receipt) is not AppServerReceipt:
        raise CaseRejected("immutable prepared case and direct observer receipt required")
    if case != prepare_persisted_case(case.intent.cwd):
        raise CaseRejected("prepared case differs from the fixed persisted scope")
    capture = getattr(receipt, "notification_capture_complete", False) is True
    result = {"case_id": CASE_ID, "host_kind": "APP_SERVER", "status": "configured_case_rejected",
              "marker_matches": False, "evidence_kind": receipt.evidence_kind,
              "notification_capture_complete": capture, "selected": asdict(case.intent.selection),
              "requested": None, "recommended": None, "observed_actual": None,
              "dispatched_configured": None if receipt.dispatched_configured is None else asdict(receipt.dispatched_configured),
              "prompt_sha256": case.intent.prompt_digest, "thread_id": receipt.thread_id,
              "turn_id": receipt.turn_id, "observer_status": receipt.status, "failure": receipt.failure}
    if (receipt.intent_digest != case.intent.digest or receipt.selection != case.intent.selection or
            receipt.invocation_key != INVOCATION_KEY or receipt.prompt_digest != case.intent.prompt_digest or
            receipt.requested is not None or receipt.recommended is not None or
            receipt.observed_actual is not None or receipt.host_kind != "APP_SERVER" or
            receipt.status != "completed_configured_route" or receipt.terminal_outcome != "completed" or
            not receipt.thread_id or not receipt.turn_id or receipt.dispatched_configured is None or
            (receipt.dispatched_configured.model, receipt.dispatched_configured.provider,
             receipt.dispatched_configured.effort) != (MODEL, "openai", EFFORT)):
        result["failure"] = "owned observer did not establish this fixed configured case"
        return result
    if receipt.evidence_kind not in {"owned_stdio", "offline_fixture"} or (
            receipt.evidence_kind == "owned_stdio" and not capture):
        result["failure"] = "owned stdio evidence and complete capture required for live acceptance"
        return result
    try:
        events = json.loads(receipt.events_json)
        reads = [event["response"] for event in events if isinstance(event, dict)
                 and event.get("method") == "thread/read" and "response" in event]
        if len(reads) != 1 or reads[0]["thread"]["id"] != receipt.thread_id:
            raise CaseRejected("exact persisted thread readback required")
        turns = [turn for turn in reads[0]["thread"]["turns"] if turn["id"] == receipt.turn_id]
        if len(turns) != 1:
            raise CaseRejected("exact persisted turn required")
        answers = [item["text"] for item in turns[0]["items"] if item.get("type") == "agentMessage"
                   and isinstance(item.get("text"), str) and item["text"].strip()]
        if len(answers) != 1:
            raise CaseRejected("exactly one persisted marker answer required")
        marker = json.loads(answers[0], object_pairs_hook=_unique_object)
        if not isinstance(marker, dict) or marker != json.loads(case.marker_json):
            raise CaseRejected("persisted answer differs from fixed four-key marker")
        result.update(marker_matches=True, failure=None,
                      status="completed_configured_case" if receipt.evidence_kind == "owned_stdio"
                      else "completed_offline_case")
    except (ValueError, TypeError, KeyError, IndexError) as error:
        result["failure"] = str(error) if isinstance(error, CaseRejected) else "invalid persisted marker evidence"
    return result


def _execute_composition(*, evidence_dir: Path, profile_factory: Any,
                         rpc_factory: Any, observer_factory: Any, offline: bool) -> dict[str, Any]:
    """Private explicit offline seam, never caller files/booleans for host authentication."""
    directory_fd = _private_directory(evidence_dir)
    prefix = "APP_SERVER-" + CASE_ID
    try:
        profile = profile_factory()
        with tempfile.TemporaryDirectory(prefix="codex-s05-persisted-", dir="/tmp") as temporary_cwd:
            case = prepare_persisted_case(str(Path(temporary_cwd).resolve()))
            # O_EXCL is the pre-host, durable approval reservation. Even a host
            # launch failure consumes this case; no retry or relaunch is allowed.
            _write_evidence(directory_fd, prefix + "-INTENT.json", {
                "host_kind": "APP_SERVER", "case_id": CASE_ID, "invocation_key": INVOCATION_KEY,
                "root_approval_scope": "one isolated fixed case c39e8241; enforced by root invocation",
                "authority_kind": "trusted_root_composition_not_cryptographic_authentication",
                "marker_json": case.marker_json, "intent": asdict(case.intent),
                "intent_digest": case.intent.digest, "prompt_sha256": case.intent.prompt_digest,
                "catalogue_sha256": persisted_catalogue().digest,
                "launch_profile_digest": profile.digest,
                "configured_mcp_count_disabled": profile.configured_mcp_count,
                "configured_plugin_count_disabled": profile.configured_plugin_count,
                "composition_kind": "offline_fixture" if offline else "owned_stdio"})
            try:
                with rpc_factory(profile.argv, case.intent.cwd) as rpc:
                    rpc.initialize({"name": "s05-fixed-persisted-case", "version": "1.0.0"})
                    observer = observer_factory(rpc, ledger_dir=evidence_dir / "ledger")
                    receipt = observer.run_once(case.intent, completion_timeout=180)
                if offline and receipt.evidence_kind != "offline_fixture":
                    raise CaseRejected("offline composition must return an offline fixture")
            except Exception as error:
                result = {"case_id": CASE_ID, "host_kind": "APP_SERVER", "status": "case_failed_without_retry",
                          "error_type": type(error).__name__, "receipt_available": False,
                          "selected": asdict(case.intent.selection), "requested": None, "recommended": None,
                          "dispatched_configured": None, "observed_actual": None,
                          "prompt_sha256": case.intent.prompt_digest}
                _write_evidence(directory_fd, prefix + "-RESULT.json", result)
                return result
            _write_evidence(directory_fd, prefix + "-RECEIPT.json", asdict(receipt))
            result = evaluate_marker(case, receipt)
            _write_evidence(directory_fd, prefix + "-RESULT.json", result)
            return result
    finally:
        os.close(directory_fd)


def execute_persisted_case() -> dict[str, Any]:
    return _execute_composition(evidence_dir=EVIDENCE_DIR, profile_factory=build_one_off_launch_profile,
                                rpc_factory=OwnedAppServerRpc, observer_factory=AppServerObserver, offline=False)


def _execute_offline_case(*, evidence_dir: Path, profile_factory: Any,
                          rpc_factory: Any, observer_factory: Any) -> dict[str, Any]:
    """Explicit private test composition; cannot return a live completion label."""
    return _execute_composition(evidence_dir=evidence_dir, profile_factory=profile_factory,
                                rpc_factory=rpc_factory, observer_factory=observer_factory, offline=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args(argv)
    try:
        if not args.execute:
            print(_json({"status": "preview_only", "host_kind": "APP_SERVER", "case_id": CASE_ID,
                         "invocation_key": INVOCATION_KEY, "evidence_dir": str(EVIDENCE_DIR),
                         "selected_route": persisted_catalogue().routes[0].route_id,
                         "model": MODEL, "effort": EFFORT, "purpose": "routine",
                         "ephemeral_thread": False,
                         "catalogue_sha256": persisted_catalogue().digest,
                         "prompt": persisted_prompt(), "prompt_sha256": hashlib.sha256(persisted_prompt().encode()).hexdigest(),
                         "marker_json": persisted_marker_json(), "requested": None,
                         "recommended": None, "observed_actual": None}))
            return 0
        result = execute_persisted_case()
        print(_json(result))
        return 0 if result["status"] == "completed_configured_case" else 1
    except Exception as error:
        print(_json({"status": "case_failed_without_retry", "host_kind": "APP_SERVER",
                     "error_type": type(error).__name__, "observed_actual": None}))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
