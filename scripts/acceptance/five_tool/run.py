#!/usr/bin/env python3
"""Run isolated native Codex acceptance for the consolidated five-tool API."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import uuid
from typing import Any

from common import Proof, Rpc, RpcError, command_overrides, initialize, raw_tool_result, sha256_file, sha256_json, start_thread, tool_result
from fixture import Fixture
import scope_candidates as scope


TOOLS = ["get_state", "query", "command", "execute", "help"]
DEV = (
    "You are the Sol Executor working under the root Astra for this isolated TectD five-tool acceptance. "
    "Do not create descendants. Use only the TectD MCP tools explicitly requested. "
    "Do not use shell, editors, web, external services, messages, or subagents. Do not access or modify anything "
    "outside the supplied temporary task directory. Native metadata authenticates the thread; never invent identity."
)
ONE_CHILD_DEV = (
    "You are the Sol Executor working under the root Astra for this isolated TectD acceptance. "
    "For this explicitly approved test only, create exactly one descendant using gpt-5.6-sol at medium effort. "
    "The child must create no descendants and must use only TectD get_state, help, query, and command as requested; "
    "no execute, Scope opening, implementation, Program completion, shell, editors, web, external services, or messages. "
    "You may only spawn that child and wait for, resume, or send input to the same child. Native metadata authenticates identity."
)


def git(source: pathlib.Path, *arguments: str) -> str:
    return subprocess.check_output(["git", "-C", str(source), *arguments], text=True).strip()


def source_snapshot(source: pathlib.Path) -> dict[str, Any]:
    raw = subprocess.check_output(
        ["git", "-C", str(source), "status", "--porcelain=v1", "--untracked-files=all", "-z"]
    )
    entries = [entry.decode(errors="surrogateescape") for entry in raw.split(b"\0") if entry]
    paths: list[str] = []
    status: list[str] = []
    index = 0
    while index < len(entries):
        entry = entries[index]
        status.append(entry)
        value = entry[3:]
        if entry[:2] in {"R ", " R", "C ", " C"} and index + 1 < len(entries):
            index += 1
            value = entries[index]
            status.append(value)
        path = source / value
        if path.is_file():
            paths.append(value)
        index += 1
    return {
        "head": git(source, "rev-parse", "HEAD"),
        "head_tree": git(source, "rev-parse", "HEAD^{tree}"),
        "status": status,
        "changed_file_sha256": {path: sha256_file(source / path) for path in sorted(set(paths))},
    }


DELEGATION_OVERRIDES = ["-c", "features.multi_agent=false", "-c", "features.multi_agent_v2=false"]


def app_command(codex: pathlib.Path, fixture: Fixture) -> list[str]:
    base = command_overrides(fixture.package, fixture.launcher, fixture.daemon_socket, fixture.host_config, fixture.workspace_key)
    return [str(codex), *base, *DELEGATION_OVERRIDES, "app-server", "--listen", "stdio://"]


def delegation_features(codex: pathlib.Path, fixture: Fixture) -> dict[str, bool]:
    output = subprocess.check_output([*app_command(codex, fixture)[:-3], "features", "list"], text=True)
    rows = [line.split() for line in output.splitlines()]
    states = {row[0]: row[2] == "true" for row in rows if len(row) == 3 and row[0] in {"multi_agent", "multi_agent_v2"}}
    if states != {"multi_agent": False, "multi_agent_v2": False}:
        raise AssertionError("owned app-server delegation features did not resolve disabled")
    return states


def find_server(app: Rpc, thread_id: str) -> dict[str, Any]:
    listing = app.request("mcpServerStatus/list", {"threadId": thread_id, "detail": "full"})
    return next(server for server in listing["data"] if server["name"] == "tectd")


def assert_fixture_server(server: dict[str, Any], fixture: Fixture, proof: Proof, phase: str) -> None:
    if not fixture.launch_attestation.is_file():
        proof.check(f"{phase} owned MCP launcher attested", False)
    attestation = json.loads(fixture.launch_attestation.read_text())
    digest = lambda value: hashlib.sha256(value.encode()).hexdigest()
    expected = {
        "cwd": str(fixture.package),
        "socket_sha256": digest(str(fixture.daemon_socket)),
        "host_config_sha256": digest(str(fixture.host_config)),
        "workspace_key_sha256": digest(fixture.workspace_key),
    }
    proof.check(
        f"{phase} resolved tectd server points only to owned fixture",
        attestation == expected and server.get("runtimeStatus") == "connected",
        {"launch_attestation_sha256": sha256_json(attestation), "runtime_status": server.get("runtimeStatus")},
    )


def find_entity(payload: Any, name: str) -> dict[str, Any]:
    if isinstance(payload, dict):
        value = payload.get(name)
        if isinstance(value, dict):
            return value
        for child in payload.values():
            try:
                return find_entity(child, name)
            except KeyError:
                pass
    elif isinstance(payload, list):
        for child in payload:
            try:
                return find_entity(child, name)
            except KeyError:
                pass
    raise KeyError(name)


def recommended_call(payload: dict[str, Any]) -> dict[str, Any]:
    actions = payload.get("actions", [])
    recommended = payload.get("recommended_action")
    if not isinstance(actions, list) or not actions:
        raise AssertionError("response has no actions")
    if not isinstance(recommended, int) or recommended < 0 or recommended >= len(actions):
        raise AssertionError("response has no valid recommended_action index")
    action = actions[recommended]
    if action.get("kind") != "ready_call":
        raise AssertionError(f"recommended action needs caller completion: {action.get('kind')}")
    if set(action) != {"kind", "tool", "arguments"} or action["tool"] not in TOOLS:
        raise AssertionError("recommended ready action is not an exact five-tool call")
    arguments = action["arguments"]
    if action["tool"] in {"query", "command", "execute"}:
        if not isinstance(arguments, dict) or set(arguments) != {"route", "params"} or not isinstance(arguments["params"], dict):
            raise AssertionError("routed ready action is not a complete public call")
    elif action["tool"] == "get_state" and arguments != {}:
        raise AssertionError("get_state ready action must have empty arguments")
    elif action["tool"] == "help" and not isinstance(arguments, dict):
        raise AssertionError("help ready action must contain its complete arguments")
    return action


def missing_paths(payload: dict[str, Any], kind: str) -> list[str]:
    descriptor = "input" if kind == "needs_input" else "context_input"
    paths: list[str] = []
    for action in payload.get("actions", []):
        if action.get("kind") != kind:
            continue
        if action.get("tool") not in TOOLS or not isinstance(action.get("arguments"), dict):
            raise AssertionError(f"invalid {kind} action envelope")
        fields = action.get(descriptor, {}).get("fields", [])
        for field in fields:
            path = field.get("path")
            if not isinstance(path, str) or not path.startswith("arguments.params."):
                raise AssertionError(f"invalid {kind} field path")
            paths.append(path)
    return paths


def call_action(app: Rpc, thread_id: str, action: dict[str, Any]) -> dict[str, Any]:
    payload, failed = tool_result(app, thread_id, action["tool"], action["arguments"])
    if failed:
        raise AssertionError("backend-provided ready action failed")
    return payload


def rejected_route(app: Rpc, thread_id: str, tool: str, arguments: dict[str, Any]) -> tuple[bool, Any]:
    payload, failed = tool_result(app, thread_id, tool, arguments)
    code = payload.get("error", {}).get("code")
    return failed and code == "invalid_arguments", {"is_error": failed, "code": code}


def rejected_legacy_tool(app: Rpc, thread_id: str, tool: str) -> tuple[bool, Any]:
    try:
        result = raw_tool_result(app, thread_id, tool, {})
    except RpcError as error:
        code = error.error.get("code")
        message = str(error.error.get("message", "")).lower()
        return code == -32602 and ("unknown tool" in message or "not found" in message), {"rpc_code": code, "message": message[:240]}
    if result.get("isError") is not True:
        return False, {"is_error": result.get("isError")}
    content = result.get("content", [])
    texts = [item.get("text", "") for item in content if item.get("type") == "text"]
    try:
        payload = json.loads(texts[-1])
    except (IndexError, json.JSONDecodeError):
        return False, {"is_error": True, "canonical_payload": False}
    code = payload.get("error", {}).get("code")
    return code == "invalid_arguments", {"is_error": True, "code": code}


def deterministic(app: Rpc, thread_id: str, fixture: Fixture, proof: Proof) -> None:
    server = find_server(app, thread_id)
    assert_fixture_server(server, fixture, proof, "deterministic")
    catalog = server["tools"]
    if not isinstance(catalog, dict):
        raise AssertionError("native MCP status returned a non-object tool catalog")
    tools = [{"name": name, **definition} for name, definition in catalog.items()]
    names = [tool["name"] for tool in tools]
    proof.check("native client discovers exactly five public tools", len(names) == 5 and set(names) == set(TOOLS), names)
    proof.data["tool_schemas"] = [
        {"name": tool["name"], "schema_sha256": sha256_json(tool.get("inputSchema")), "annotations": tool.get("annotations")}
        for tool in tools
    ]
    proof.persist()

    search, failed = tool_result(app, thread_id, "help", {"mode": "search", "text": "program", "tool": "query"})
    proof.check("help search works before workspace open", not failed and bool(search))
    method, failed = tool_result(app, thread_id, "help", {"mode": "describe", "method": "tectd-program"})
    proof.check("help method works before workspace open", not failed and bool(method))
    candidate_method, failed = tool_result(app, thread_id, "help", {"mode": "describe", "method": "tectd-scope-candidates"})
    proof.check("Scope-candidate method works before workspace open", not failed and bool(candidate_method))
    unopened, failed = tool_result(app, thread_id, "get_state", {})
    proof.check("native unopened get_state succeeds", not failed and unopened.get("status") == "uninitialized")
    open_action = recommended_call(unopened)
    proof.check(
        "get_state supplies an executable workspace.open call",
        open_action == {"kind": "ready_call", "tool": "command", "arguments": {"route": "workspace.open", "params": {}}},
        open_action,
    )
    opened = call_action(app, thread_id, open_action)
    proof.check("backend ready action executes without help lookup", bool(opened.get("workspace")))
    native_id = find_entity(opened, "session").get("native_session_id")
    proof.check("Codex host replaces forged metadata with native thread identity", native_id == thread_id)
    proof.check(
        "context template identifies the exact nested task-directory field",
        "arguments.params.task_directory" in missing_paths(opened, "needs_context"),
    )

    programs, failed = tool_result(app, thread_id, "query", {"route": "program.list", "params": {"limit": 25}})
    proof.check("representative query route succeeds", not failed and bool(programs))
    narrative = "Exact isolated acceptance narrative. Preserve this input byte for byte."
    begun, failed = tool_result(
        app,
        thread_id,
        "command",
        {"route": "program.begin", "params": {"request_id": str(uuid.uuid4()), "input": narrative}},
    )
    program = find_entity(begun, "program")
    proof.check("representative command creates a recoverable Program draft", not failed and program.get("status") == "draft")
    proof.check(
        "Program templates expose missing values without masquerading as calls",
        "arguments.params.name" in missing_paths(begun, "needs_input"),
    )
    recovered, failed = tool_result(
        app,
        thread_id,
        "query",
        {"route": "program.get", "params": {"program_id": program["id"], "after_input": 0, "limit": 25}},
    )
    proof.check("Program draft and exact input recover through query", not failed and narrative in json.dumps(recovered, ensure_ascii=False))

    inspected, failed = tool_result(
        app,
        thread_id,
        "command",
        {"route": "setup.inspect", "params": {"task_directory": str(fixture.task)}},
    )
    proof.check("setup inspection binds only the owned fixture directory", not failed and not (fixture.task / "AGENTS.md").exists())
    proof.check(
        "setup begin template names the exact missing raw input path",
        "arguments.params.input" in missing_paths(inspected, "needs_input"),
    )
    setup_begun, failed = tool_result(
        app,
        thread_id,
        "command",
        {"route": "setup.begin", "params": {"request_id": str(uuid.uuid4()), "input": "Prepare the isolated fixture instructions."}},
    )
    setup = find_entity(setup_begun, "setup")
    proof.check(
        "setup save template remains explicitly incomplete",
        "arguments.params.content" in missing_paths(setup_begun, "needs_input"),
    )
    content = "# Isolated acceptance fixture\n\nDo not access resources outside this temporary directory.\n"
    saved, failed = tool_result(
        app,
        thread_id,
        "command",
        {"route": "setup.save", "params": {"setup_id": setup["id"], "revision": setup["revision"], "input_cursor": 1, "ready": True, "content": content}},
    )
    ready = find_entity(saved, "setup")
    proof.check(
        "setup ready intent is durable before filesystem publication",
        not failed and ready.get("status") == "draft" and ready.get("current_step") == "ready_to_apply"
        and not (fixture.task / "AGENTS.md").exists(),
    )
    applied, failed = tool_result(
        app,
        thread_id,
        "execute",
        {"route": "setup.apply", "params": {"setup_id": ready["id"], "revision": ready["revision"]}},
    )
    proof.check("execute publishes exact bytes only in owned fixture", not failed and (fixture.task / "AGENTS.md").read_text() == content)
    proof.data["representative_results"] = {
        "help_search_sha256": sha256_json(search),
        "help_method_sha256": sha256_json(method),
        "help_candidate_method_sha256": sha256_json(candidate_method),
        "opened_sha256": sha256_json(opened),
        "program_get_sha256": sha256_json(recovered),
        "setup_apply_sha256": sha256_json(applied),
        "fixture_agents_sha256": sha256_file(fixture.task / "AGENTS.md"),
    }
    before_hash = sha256_file(fixture.task / "AGENTS.md")
    before_database = fixture.database_fingerprint()
    legacy_failed, legacy_detail = rejected_legacy_tool(app, thread_id, "open_workspace")
    route_failed, route_detail = rejected_route(app, thread_id, "query", {"route": "unknown.route", "params": {}})
    filesystem_unchanged = sha256_file(fixture.task / "AGENTS.md") == before_hash
    database_unchanged = fixture.database_fingerprint() == before_database
    proof.check(
        "legacy tool and unknown route return explicit refusals without effects",
        legacy_failed and route_failed and filesystem_unchanged and database_unchanged,
        {"legacy": legacy_detail, "route": route_detail, "filesystem_unchanged": filesystem_unchanged, "database_unchanged": database_unchanged},
    )
    proof.persist()


def seed_model_thread(app: Rpc, thread_id: str, proof: Proof) -> str:
    state, failed = tool_result(app, thread_id, "get_state", {})
    if failed:
        raise AssertionError("model fixture get_state failed")
    opened = call_action(app, thread_id, recommended_call(state))
    if not opened.get("workspace"):
        raise AssertionError("model fixture workspace did not open")
    begun, failed = tool_result(
        app,
        thread_id,
        "command",
        {
            "route": "program.begin",
            "params": {"request_id": str(uuid.uuid4()), "input": "Read this isolated Program through the offered ready action."},
        },
    )
    if failed:
        raise AssertionError("model fixture Program seed failed")
    program_id = find_entity(begun, "program")["id"]
    proof.data["model_fixture"] = {"program_id": program_id, "seeded_by": "deterministic native calls"}
    proof.persist()
    return program_id


def model_turn(app: Rpc, thread_id: str, expected_program_id: str, proof: Proof) -> None:
    prompt = (
        "Use only the TectD MCP server. Call get_state once and directly execute its recommended ready query action "
        "for the current Program without a help lookup. Then use help search for setup routes and help describe method "
        "tectd-program. Report what you read. Use only get_state, query, and help; do not mutate anything."
    )
    turn_id, items = scope.collect_model_turn(app, thread_id, prompt, proof)
    calls, parse_errors = scope.capture_model_calls(items)
    proof.data["scope_candidate_model_capture"]["calls"] = calls
    proof.data["scope_candidate_model_capture"]["parse_errors"] = parse_errors
    proof.persist()
    if parse_errors:
        raise AssertionError("fresh Sol turn emitted malformed MCP call evidence")
    names = [call["tool"] for call in calls]
    item_types = [item.get("type") for item in items]
    proof.check("fresh Sol native help turn completed", True)
    scope.assert_model_item_boundary(items)
    proof.check(
        "fresh Sol turn used no shell subagent or external tool",
        True,
        item_types,
    )
    proof.check("fresh Sol turn used only get_state query and help", bool(calls) and set(names) <= {"get_state", "query", "help"}, names)
    scope.assert_successful_calls(calls, {"get_state", "query", "help"})
    proof.check("fresh Sol turn consumed the ready query action", "get_state" in names and "query" in names and "help" in names, names)
    query_arguments = [call["arguments"] for call in calls if call["tool"] == "query"]
    proof.check(
        "fresh Sol turn used the exact offered program.get payload",
        {"route": "program.get", "params": {"program_id": expected_program_id}} in query_arguments,
        query_arguments,
    )
    proof.data["model_turn"] = {
        "thread_id": thread_id,
        "turn_id": turn_id,
        "model": "gpt-5.6-sol",
        "effort": "low",
        "tool_names": names,
        "calls": calls,
    }
    proof.persist()


def main() -> None:
    parser = argparse.ArgumentParser()
    default_source = pathlib.Path(__file__).resolve().parents[3]
    parser.add_argument("--source", type=pathlib.Path, default=default_source)
    parser.add_argument("--codex", type=pathlib.Path, default=pathlib.Path("/Applications/Codex.app/Contents/Resources/codex"))
    parser.add_argument("--postgres-bin", type=pathlib.Path, default=pathlib.Path("/opt/homebrew/opt/postgresql@18/bin"))
    parser.add_argument("--proof", type=pathlib.Path, required=True)
    parser.add_argument(
        "--protected-artifact",
        type=pathlib.Path,
        action="append",
        default=[],
        help="additional nonsecret installed artifact to hash before and after acceptance",
    )
    parser.add_argument("--model-turn", action="store_true")
    parser.add_argument("--scope-candidates", action="store_true", help="run the reviewed Scope-candidate scenario")
    parser.add_argument("--allow-one-child-sol", action="store_true", help="test-only explicit one-child evidence mode")
    parser.add_argument("--final", action="store_true", help="require clean committed source and label proof final")
    parser.add_argument("--keep-fixture", action="store_true")
    args = parser.parse_args()
    if args.scope_candidates and not args.model_turn:
        parser.error("--scope-candidates requires --model-turn")
    if args.allow_one_child_sol and not (args.scope_candidates and args.model_turn):
        parser.error("--allow-one-child-sol requires --model-turn --scope-candidates")
    source = args.source.resolve()
    if args.final and git(source, "status", "--porcelain=v1"):
        raise SystemExit("final acceptance requires a clean committed source worktree")
    home = pathlib.Path.home()
    protected_paths = [home / ".codex/config.toml", home / ".config/tectd/install.json", home / ".config/tectd/README.md"]
    acceptance = home / ".local/share/tectd/acceptance"
    protected_paths.extend(sorted(acceptance.glob("install-*/native-install-proof.json")))
    for pattern in ["upgrade-*/upgrade-proof.json", "upgrade-*/post-native-readonly-captured.json", "upgrade-*/preservation-proof.json"]:
        protected_paths.extend(sorted(acceptance.glob(pattern)))
    protected_paths.extend(path.resolve() for path in args.protected_artifact)
    protected_before = {str(path): sha256_file(path) for path in protected_paths if path.is_file()}
    proof = Proof(
        args.proof.resolve(),
        {
            "schema": "tectd-five-tool-native-acceptance.v1",
            "status": "running",
            "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "source": source_snapshot(source),
            "codex": {"version": subprocess.check_output([str(args.codex), "--version"], text=True).strip(), "sha256": sha256_file(args.codex)},
            "model_phase_requested": args.model_turn,
            "one_child_test_override_requested": args.allow_one_child_sol,
            "evidence_kind": "final_clean_commit" if args.final else "exploratory_feature_smoke",
            "protected_artifact_hashes_before": protected_before,
            "checks": [],
        },
    )
    fixture = Fixture(source, args.postgres_bin, args.keep_fixture)
    apps: list[Rpc] = []
    deterministic_home: pathlib.Path | None = None
    try:
        fixture.prepare()
        proof.data["owned_app_server_features"] = {"overrides": DELEGATION_OVERRIDES, "effective": delegation_features(args.codex, fixture)}
        proof.data["artifacts"] = {
            name: sha256_file(fixture.binaries / name) for name in ["tectd", "tectd-mcp", "tect-admin"]
        }
        proof.data["artifacts"]["packaged_mcp"] = sha256_file(fixture.package / "bin/tectd-mcp")
        proof.persist()
        deterministic_home = pathlib.Path(tempfile.mkdtemp(prefix="codex-five-tool-deterministic-"))
        deterministic_home.chmod(0o700)
        env = {**os.environ, "CODEX_HOME": str(deterministic_home)}
        env.pop("CODEX_SESSION_ID", None)
        env.pop("CODEX_THREAD_ID", None)
        first = Rpc(app_command(args.codex, fixture), env, fixture.task)
        apps.append(first)
        initialize(first, "tectd_five_tool_deterministic")
        first_thread = start_thread(first, fixture.task, DEV)
        proof.data["native_threads"] = [{"phase": "deterministic", "id": first_thread}]
        deterministic(first, first_thread, fixture, proof)
        first.close()
        apps.remove(first)
        shutil.rmtree(deterministic_home, ignore_errors=True)
        if args.model_turn:
            fixture.workspace_key = "native-five-model-" + uuid.uuid4().hex
            model_env = dict(os.environ)
            model_env.pop("CODEX_SESSION_ID", None)
            model_env.pop("CODEX_THREAD_ID", None)
            second = Rpc(app_command(args.codex, fixture), model_env, fixture.task)
            apps.append(second)
            initialize(second, "tectd_five_tool_model")
            second_thread = start_thread(second, fixture.task, ONE_CHILD_DEV if args.allow_one_child_sol else DEV)
            proof.data["native_threads"].append({"phase": "model", "id": second_thread})
            assert_fixture_server(find_server(second, second_thread), fixture, proof, "model")
            if args.scope_candidates:
                call = lambda tool, arguments: tool_result(second, second_thread, tool, arguments)
                scenario = scope.seed_candidate_scenario(call, str(fixture.source_fixture))
                proof.data["scope_candidate_fixture"] = scenario
                proof.data["scope_candidate_daemon_restart"] = fixture.restart_daemon()
                proof.persist()
                scope.run_candidate_model_turn(second, second_thread, scenario, proof, args.allow_one_child_sol)
            else:
                model_program_id = seed_model_thread(second, second_thread, proof)
                model_turn(second, second_thread, model_program_id, proof)
        else:
            proof.data["model_phase"] = {"status": "not_run", "reason": "--model-turn was not supplied"}
        proof.data["status"] = "pass"
    except Exception as error:
        proof.data["status"] = "fail"
        proof.data["failure"] = type(error).__name__ + ": " + str(error)
        raise
    finally:
        for app in apps:
            app.close()
        if deterministic_home is not None:
            shutil.rmtree(deterministic_home, ignore_errors=True)
        proof.data["cleanup"] = fixture.cleanup()
        protected_after = {str(path): sha256_file(path) for path in protected_paths if path.is_file()}
        proof.data["protected_artifact_hashes_after"] = protected_after
        proof.data.setdefault("checks", []).append(
            {"name": "installed release and persistent Codex config unchanged", "passed": protected_before == protected_after}
        )
        if protected_before != protected_after:
            proof.data["status"] = "fail"
            proof.data.setdefault("failure", "protected installed artifact changed")
        proof.data["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        proof.persist()


if __name__ == "__main__":
    main()
