"""Synthetic source/private scripted RPC logic only; no production authority."""

from copy import deepcopy
from dataclasses import replace
import hashlib
import json
from pathlib import Path
import tempfile
import threading
import unittest

from scripts.caller_host_routing import (CallerHostRouting, CallerRoutingRejected,
    CallerRoutingRequest, CurrentHostSelection, TrustedCurrentSelectionPort, _json,
    _validate_material)
from scripts.codex_app_server_observer import (AppServerObserver, ObservationRejected,
    _caller_intent_from_trusted_source, _mint_caller_source_ticket, _sha)
from scripts.test_codex_app_server_observer import OfflineRpc, CWD


GOLDEN = Path(__file__).resolve().parents[1] / "crates/application/src/model_route_execution/synthetic_selection_golden.json"
GOLDEN_SHA = "6b56a713aa759f56d7e51016ff4e3f5e001802551657741e25f81f4fb8058598"
PROMPT = "Return this synthetic bounded caller case."


def golden():
    return json.loads(GOLDEN.read_text(encoding="utf-8"))


def request(route="owner-sol61", key="synthetic-selection-key"):
    m = golden()
    return CallerRoutingRequest(m["preparation"]["request_key"], m["decision"]["id"],
        m["disposition"]["id"], m["preparation"]["work"]["approved_matrix_selection"]["task_id"], 3,
        m["preparation"]["eligible"]["work_context_digest"], m["preparation"]["eligible"]["catalogue_digest"],
        route, key)


class SyntheticCurrentSource(TrustedCurrentSelectionPort):
    """Private fixture issuer, not an installed or authenticated source adapter."""

    def __init__(self, mutate=None):
        self.calls = []
        self.mutate = mutate
        self.raw_reply = None
        self.fail = False

    def resolve_current(self, pins, *, input_sha256):
        self.calls.append((pins, input_sha256))
        if self.fail:
            raise ValueError("synthetic missing/revoked current source")
        expected = request(pins.selected_route_id, pins.invocation_key)
        if pins != expected:
            raise ValueError("synthetic source pin mismatch")
        m = golden()
        m["input_sha256"] = input_sha256
        m["invocation_key"] = pins.invocation_key
        row = next((r for r in m["preparation"]["catalogue"]["routes"] if r["id"] == pins.selected_route_id), None)
        if row is None:
            raise ValueError("synthetic unavailable route")
        m["selected_route"] = {"route_id": row["id"], **{f: row[f] for f in ("provider", "model", "effort")}}
        m["configured_route"] = deepcopy(m["selected_route"])
        if self.mutate:
            self.mutate(m)
        raw = self.raw_reply(_json(m)) if self.raw_reply else _json(m)
        return CurrentHostSelection(raw, hashlib.sha256(raw.encode("utf-8")).hexdigest())


class ScriptedRpc(OfflineRpc):
    """Pure in-memory replies; never a subprocess or network transport."""

    def __init__(self, prompt=PROMPT):
        super().__init__()
        self.models["data"].append({"model": "gpt-6-luna", "supportedReasoningEfforts": [{"reasoningEffort": "xhigh"}]})
        self.started["thread"]["ephemeral"] = False
        self.read["thread"]["turns"][0]["items"][0]["content"][0]["text"] = prompt
        self.read["thread"]["turns"][0]["items"][0]["clientId"] = None
        self.start_mutation = None

    def request(self, method, params, timeout=30):
        if method == "thread/start":
            self.started.update(model=params["model"], reasoningEffort=params["config"]["model_reasoning_effort"])
            if self.start_mutation:
                self.start_mutation(self.started)
        return super().request(method, params, timeout)


class CallerRoutingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.ledger = Path(self.temporary.name) / "ledger"

    def composition(self, source=None, rpc=None):
        source = source or SyntheticCurrentSource()
        rpc = rpc or ScriptedRpc()
        observer = object.__new__(AppServerObserver)
        observer._configure(rpc, self.ledger, "offline_fixture")
        bridge = CallerHostRouting()
        bridge._install(source, observer)
        return bridge, source, rpc, observer

    def test_exact_rust_synthetic_canonical_golden_parity(self):
        raw = GOLDEN.read_text(encoding="utf-8").removesuffix("\n")
        self.assertEqual(len(raw.encode("utf-8")), 12251)
        self.assertEqual(hashlib.sha256(raw.encode("utf-8")).hexdigest(), GOLDEN_SHA)
        self.assertEqual(_json(json.loads(raw)), raw)
        result = _validate_material(CurrentHostSelection(raw, GOLDEN_SHA), request(), "1" * 64)
        self.assertEqual(result["source_binding"]["matrix_save_actor_id"], "00000000-0000-0000-0000-000000000012")
        self.assertNotEqual(result["invoking_session_id"], result["preparation"]["origin_session_id"])

    def test_public_default_and_untyped_source_request_deny(self):
        with self.assertRaisesRegex(CallerRoutingRejected, "no trusted"):
            CallerHostRouting().run_once(request(), prompt=PROMPT, cwd=CWD)
        bridge, source, rpc, _ = self.composition()
        with self.assertRaises(CallerRoutingRejected):
            bridge.run_once({"accepted": True}, prompt=PROMPT, cwd=CWD)
        self.assertEqual(source.calls, [])
        self.assertEqual(rpc.calls, [])
        with self.assertRaises(CallerRoutingRejected):
            bridge._install({"accepted": True}, bridge._observer)

    def test_two_source_selections_control_exact_different_rpc_pairs(self):
        for route, pair in (("owner-sol61", ("gpt-6.1-sol", "medium")), ("owner-luna6", ("gpt-6-luna", "xhigh"))):
            with self.subTest(route=route), tempfile.TemporaryDirectory() as temporary:
                self.ledger = Path(temporary) / "ledger"
                bridge, source, rpc, _ = self.composition()
                receipt = bridge.run_once(request(route), prompt=PROMPT, cwd=CWD)
                self.assertEqual(receipt.status, "completed_configured_route")
                self.assertEqual(receipt.evidence_kind, "offline_fixture")
                self.assertFalse(receipt.notification_capture_complete)
                self.assertIsNone(receipt.observed_actual)
                calls = dict(rpc.calls)
                self.assertEqual((calls["thread/start"]["model"], calls["thread/start"]["config"]["model_reasoning_effort"]), pair)
                self.assertEqual((calls["turn/start"]["model"], calls["turn/start"]["effort"]), pair)
                self.assertIs(calls["thread/start"]["ephemeral"], False)
                self.assertIs(calls["thread/start"]["allowProviderModelFallback"], False)
                self.assertEqual(source.calls[0][1], hashlib.sha256(PROMPT.encode()).hexdigest())
                if route == "owner-sol61":
                    self.assertEqual((receipt.requested.route_id, receipt.recommended.route_id, receipt.selection.route_id), ("disabled-audit", "owner-luna6", "owner-sol61"))
                material = json.loads(receipt.source_binding_json)
                self.assertEqual(material["source_binding"], golden()["source_binding"])
                self.assertEqual(material["decision"]["prepared"], material["preparation"])

    def test_missing_reject_abstain_wrong_actor_old_or_changed_binding_precedes_host(self):
        mutations = (
            lambda m: m["disposition"].update(action="Reject"),
            lambda m: m["disposition"].update(actor_id="00000000-0000-0000-0000-000000000019"),
            lambda m: m["decision"].update(outcome={"Abstained": {"reason": "Explicit"}}),
            lambda m: m["preparation"].update(preparation="NoEligibleRoutes"),
            lambda m: m["preparation"]["work"]["approved_matrix_selection"].update(task_revision=2),
            lambda m: m["preparation"]["eligible"].update(catalogue_digest="f" * 64),
            lambda m: m["source_binding"].update(requirements_semantic_digest="f" * 64),
            lambda m: m["configured_route"].update(effort="xhigh"),
            lambda m: m["preparation"]["work"]["role"]["Known"]["provenance"]["OperatingEvidence"].update(expires_at_epoch_ms=1),
        )
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                bridge, _, rpc, _ = self.composition(SyntheticCurrentSource(mutate))
                with self.assertRaises(CallerRoutingRejected):
                    bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
                self.assertEqual(rpc.calls, [])
                self.assertFalse(self.ledger.exists())
        bridge, source, rpc, _ = self.composition()
        source.fail = True
        with self.assertRaises(CallerRoutingRejected):
            bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
        self.assertEqual(rpc.calls, [])

    def test_distinct_original_source_and_mapped_work_provenance_is_preserved(self):
        def distinct(m):
            m["source_binding"]["locator"].update(scope_id="00000000-0000-0000-0000-000000000014",
                candidate_set_id="00000000-0000-0000-0000-000000000015",
                work_candidate_id="00000000-0000-0000-0000-000000000016", expected_work_revision=1)
        bridge, _, _, _ = self.composition(SyntheticCurrentSource(distinct))
        receipt = bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
        self.assertEqual(receipt.status, "completed_configured_route")
        material = json.loads(receipt.source_binding_json)
        locator = material["source_binding"]["locator"]
        link = material["preparation"]["work"]["selection_link"]
        self.assertNotEqual(locator["candidate_set_id"], link["candidate_set_id"])
        self.assertNotEqual(locator["work_candidate_id"], link["mapped_work_node_id"])
        self.assertNotEqual(locator["expected_work_revision"], link["mapped_work_node_revision"])
        self.assertNotEqual(locator["scope_id"], material["source_binding"]["scope_id"])

    def test_rust_u64_bounds_and_i64_operating_timestamps(self):
        def values(m, value):
            p = m["preparation"]
            p["catalogue"]["version"] = value
            p["eligible"]["catalogue_version"] = value
            for name in ("remaining_budget_units", "available_latency_ms"):
                p["work"][name]["Known"]["value"] = value
            for row in p["catalogue"]["routes"]:
                row["minimum_budget_units"] = value
                row["minimum_latency_ms"] = value
            m["decision"]["prepared"] = deepcopy(p)
        bridge, _, _, _ = self.composition(SyntheticCurrentSource(lambda m: values(m, 2**64 - 1)))
        self.assertEqual(bridge.run_once(request(), prompt=PROMPT, cwd=CWD).status, "completed_configured_route")
        for mutate in (lambda m: values(m, 2**64),
                lambda m: m["preparation"]["work"]["role"]["Known"]["provenance"]["OperatingEvidence"].update(expires_at_epoch_ms=2**63),
                lambda m: m["source_binding"]["locator"].update(expected_work_revision=2**63),
                lambda m: m["preparation"]["eligible"].update(catalogue_version=True)):
            bridge, _, rpc, _ = self.composition(SyntheticCurrentSource(mutate))
            with self.assertRaises(CallerRoutingRejected):
                bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
            self.assertEqual(rpc.calls, [])

    def test_closed_integer_types_confirmed_facts_and_usize_parity(self):
        def snapshot(m):
            m["decision"]["prepared"] = deepcopy(m["preparation"])
            raw = _json(m)
            return CurrentHostSelection(raw, hashlib.sha256(raw.encode()).hexdigest())
        for field in ("expected_task_revision",):
            for bad in (True, 1.0, "1", 2**63):
                with self.subTest(request_field=field, value=bad), self.assertRaises(CallerRoutingRejected):
                    replace(request(), **{field: bad})
        for field in ("eligible_version", "source_revision", "mapped_index", "mapped_indices", "fact_revision", "budget", "latency"):
            for bad in (True, 1.0, "1", 2**64):
                with self.subTest(field=field, value=bad):
                    m = golden()
                    p = m["preparation"]
                    work = p["work"]
                    if field == "eligible_version": p["eligible"]["catalogue_version"] = bad
                    elif field == "source_revision": m["source_binding"]["locator"]["expected_work_revision"] = bad
                    elif field == "mapped_index": work["selection_link"]["mapped_draft_node_index"] = bad
                    elif field == "mapped_indices": work["approved_matrix_selection"]["mapped_draft_node_indices"] = [bad]
                    elif field == "fact_revision":
                        work["selection_link"]["mapped_work_node_revision"] = 1
                        for name in ("role", "tool", "data_class", "remaining_budget_units", "available_latency_ms"):
                            work[name]["Known"]["provenance"]["OperatingEvidence"]["work_node_revision"] = 1
                        work["role"]["Known"]["provenance"]["OperatingEvidence"]["work_node_revision"] = bad
                    else: work["remaining_budget_units" if field == "budget" else "available_latency_ms"]["Known"]["value"] = bad
                    with self.assertRaises(CallerRoutingRejected):
                        _validate_material(snapshot(m), request(), "1" * 64)
        m = golden()
        m["preparation"]["work"]["approved_matrix_selection"]["mapped_draft_node_indices"] = [2**64 - 1]
        m["preparation"]["work"]["selection_link"]["mapped_draft_node_index"] = 2**64 - 1
        self.assertEqual(_validate_material(snapshot(m), request(), "1" * 64)["preparation"]["work"]["selection_link"]["mapped_draft_node_index"], 2**64 - 1)
        def confirmed(m, field):
            work = m["preparation"]["work"]
            original = work[field]["Known"]["provenance"]["OperatingEvidence"]
            work[field]["Known"]["provenance"] = {"ConfirmedWorkRequirement": {
                "source_ref": original["source_ref"], "work_node_id": original["work_node_id"],
                "work_node_revision": original["work_node_revision"],
                "frozen_snapshot_id": work["context_authority"]["frozen_snapshot_id"],
                "requirements_semantic_digest": work["context_authority"]["requirements_semantic_digest"]}}
        m = golden()
        confirmed(m, "role")
        _validate_material(snapshot(m), request(), "1" * 64)
        for field in ("remaining_budget_units", "available_latency_ms"):
            m = golden()
            confirmed(m, field)
            with self.assertRaises(CallerRoutingRejected):
                _validate_material(snapshot(m), request(), "1" * 64)
        m = golden()
        confirmed(m, "role")
        m["preparation"]["work"]["role"]["Known"]["provenance"]["ConfirmedWorkRequirement"]["requirements_semantic_digest"] = "f" * 64
        with self.assertRaises(CallerRoutingRejected):
            _validate_material(snapshot(m), request(), "1" * 64)

    def test_producer_text_fields_preserve_rationale_controls_and_source_references(self):
        def snapshot(m):
            m["decision"]["prepared"] = deepcopy(m["preparation"])
            raw = _json(m)
            return CurrentHostSelection(raw, hashlib.sha256(raw.encode()).hexdigest())
        m = golden()
        rationale = " \tAccepted source advice.\nCaller selects the other allowed route.\t "
        m["disposition"]["rationale"] = rationale
        choice = "choice\nwith\ttyped source semantics"
        m["preparation"]["work"]["approved_matrix_selection"]["selected_choice_id"] = choice
        for row in m["preparation"]["catalogue"]["routes"]:
            row["allowed_matrix_choice_ids"] = [choice]
        for field in ("role", "tool", "data_class", "remaining_budget_units", "available_latency_ms"):
            m["preparation"]["work"][field]["Known"]["provenance"]["OperatingEvidence"]["source_ref"] = "source\ninternal\ttab"
        m["preparation"]["work"]["host_capabilities"]["Known"]["provenance"]["Host"]["evidence_ref"] = "host\ninternal\ttab"
        result = _validate_material(snapshot(m), request(), "1" * 64)
        self.assertEqual(result["disposition"]["rationale"], rationale)
        self.assertEqual(result["preparation"]["work"]["approved_matrix_selection"]["selected_choice_id"], choice)
        for field, bad_values in (("rationale", (" \n\t ", "has\0nul", "é" * 2049)),
                ("choice", ("", " choice", "choice ", "has\0nul", "é" * 2049)),
                ("source_ref", ("", " source", "source ", "has\0nul", "é" * 257)),
                ("host_ref", ("", " host", "host ", "has\0nul", "é" * 257))):
            for bad in bad_values:
                m = golden()
                if field == "rationale": m["disposition"]["rationale"] = bad
                elif field == "choice":
                    m["preparation"]["work"]["approved_matrix_selection"]["selected_choice_id"] = bad
                    for row in m["preparation"]["catalogue"]["routes"]: row["allowed_matrix_choice_ids"] = [bad]
                elif field == "source_ref": m["preparation"]["work"]["role"]["Known"]["provenance"]["OperatingEvidence"]["source_ref"] = bad
                else: m["preparation"]["work"]["host_capabilities"]["Known"]["provenance"]["Host"]["evidence_ref"] = bad
                with self.subTest(field=field, bad=repr(bad[:20])), self.assertRaises(CallerRoutingRejected):
                    _validate_material(snapshot(m), request(), "1" * 64)

    def test_mapped_indices_preserve_actual_producer_ordering_and_cardinality(self):
        for indices in ([], [0, 0], [1, 0], list(range(101))):
            m = golden()
            m["preparation"]["work"]["approved_matrix_selection"]["mapped_draft_node_indices"] = indices
            m["decision"]["prepared"] = deepcopy(m["preparation"])
            raw = _json(m)
            with self.assertRaises(CallerRoutingRejected):
                _validate_material(CurrentHostSelection(raw, hashlib.sha256(raw.encode()).hexdigest()), request(), "1" * 64)
        m = golden()
        m["preparation"]["work"]["approved_matrix_selection"]["mapped_draft_node_indices"] = list(range(100))
        m["decision"]["prepared"] = deepcopy(m["preparation"])
        raw = _json(m)
        _validate_material(CurrentHostSelection(raw, hashlib.sha256(raw.encode()).hexdigest()), request(), "1" * 64)

    def test_private_ticket_is_bound_to_source_issuer_observer_and_exact_projection(self):
        bridge, _, _, observer = self.composition()
        captured = []
        direct_run = observer.run_once
        def capture(intent):
            captured.append(intent)
            return direct_run(intent)
        observer.run_once = capture
        bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
        intent = captured[0]
        ticket = intent._source_ticket
        uninstalled = object.__new__(AppServerObserver)
        uninstalled._configure(ScriptedRpc(), self.ledger, "offline_fixture")
        with self.assertRaisesRegex(ObservationRejected, "issuer"):
            _mint_caller_source_ticket(observer=uninstalled, issuer=None,
                selection=ticket.selection, requested=ticket.requested, recommended=ticket.recommended,
                invocation_key=ticket.invocation_key, input_sha256=ticket.input_sha256,
                source_binding_json=ticket.source_binding_json, source_binding_digest=ticket.source_binding_digest)
        with self.assertRaises(ObservationRejected):
            _caller_intent_from_trusted_source(source_ticket={"material_json": ticket.source_binding_json}, prompt=PROMPT, cwd=CWD)
        with self.assertRaises(ObservationRejected):
            _caller_intent_from_trusted_source(source_ticket=ticket, prompt="substituted", cwd=CWD)
        other, _, other_rpc, other_observer = self.composition()
        with self.assertRaisesRegex(ObservationRejected, "another source/observer"):
            other_observer.run_once(intent)
        self.assertEqual(other_rpc.calls, [])
        with self.assertRaisesRegex(ObservationRejected, "issuer"):
            _mint_caller_source_ticket(observer=other_observer, issuer=ticket.issuer,
                selection=ticket.selection, requested=ticket.requested, recommended=ticket.recommended,
                invocation_key=ticket.invocation_key, input_sha256=ticket.input_sha256,
                source_binding_json=ticket.source_binding_json, source_binding_digest=ticket.source_binding_digest)
        changed = json.loads(ticket.source_binding_json)
        changed["input_sha256"] = "f" * 64
        raw = _json(changed)
        with self.assertRaisesRegex(ObservationRejected, "projection"):
            _mint_caller_source_ticket(observer=observer, issuer=ticket.issuer,
                selection=ticket.selection, requested=ticket.requested, recommended=ticket.recommended,
                invocation_key=ticket.invocation_key, input_sha256=ticket.input_sha256,
                source_binding_json=raw, source_binding_digest=_sha(raw))
        observer._caller_source_issuer = object()
        with self.assertRaisesRegex(ObservationRejected, "installed caller composition"):
            _caller_intent_from_trusted_source(source_ticket=ticket, prompt=PROMPT, cwd=CWD)

    def test_input_substitution_and_source_pins_deny_before_host(self):
        source = SyntheticCurrentSource(lambda m: m.update(input_sha256="1" * 64))
        bridge, _, rpc, _ = self.composition(source)
        with self.assertRaises(CallerRoutingRejected):
            bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
        self.assertEqual(rpc.calls, [])
        bridge, _, rpc, _ = self.composition()
        for field, value in (("expected_task_revision", 2), ("expected_work_context_digest", "f" * 64), ("expected_catalogue_digest", "f" * 64)):
            with self.assertRaises(CallerRoutingRejected):
                bridge.run_once(replace(request(), **{field: value}), prompt=PROMPT, cwd=CWD)
        self.assertEqual(rpc.calls, [])
        bridge, _, rpc, _ = self.composition()
        with self.assertRaises(CallerRoutingRejected):
            bridge.run_once(request("disabled-audit"), prompt=PROMPT, cwd=CWD)
        self.assertEqual(rpc.calls, [])

    def test_duplicate_unknown_float_noncanonical_or_wrong_digest_source_deny(self):
        for raw_reply in (
            lambda raw: raw.replace('{"configured_route":', '{"unexpected":0,"configured_route":', 1),
            lambda raw: raw.replace('{"configured_route":', '{"schema":"forged","configured_route":', 1),
            lambda raw: raw.replace('"advisory_config_revision":2', '"advisory_config_revision":2.0'),
            lambda raw: raw + "\n",
            lambda raw: raw.replace('"authority_schema":', '"unknown_nested":0,"authority_schema":', 1),
        ):
            with self.subTest(raw_reply=raw_reply):
                source = SyntheticCurrentSource()
                source.raw_reply = raw_reply
                bridge, _, rpc, _ = self.composition(source)
                with self.assertRaises(CallerRoutingRejected):
                    bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
                self.assertEqual(rpc.calls, [])
        bridge, source, rpc, _ = self.composition()
        source.resolve_current = lambda *args, **kwargs: {"material_json": "{}", "accepted": True}
        with self.assertRaises(CallerRoutingRejected):
            bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
        self.assertEqual(rpc.calls, [])

    def test_direct_replay_reopened_conflict_and_fabricated_receipt_denial(self):
        bridge, source, rpc, observer = self.composition()
        receipt = bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
        before = list(rpc.calls)
        self.assertIs(bridge.run_once(request(), prompt=PROMPT, cwd=CWD), receipt)
        self.assertEqual(rpc.calls, before)
        reopened, _, new_rpc, _ = self.composition()
        with self.assertRaisesRegex(ObservationRejected, "already reserved"):
            reopened.run_once(request(), prompt=PROMPT, cwd=CWD)
        self.assertEqual(new_rpc.calls, [])
        with self.assertRaisesRegex(ObservationRejected, "conflicts"):
            reopened.run_once(request("owner-luna6"), prompt=PROMPT, cwd=CWD)
        self.assertEqual(new_rpc.calls, [])
        # A value-equal copied dataclass is not the direct observation object.
        observer.run_once = lambda intent: replace(receipt)
        with self.assertRaisesRegex(CallerRoutingRejected, "direct owned-observer"):
            bridge.run_once(request(), prompt=PROMPT, cwd=CWD)

    def test_lost_turn_reply_never_retries_in_same_or_new_composition(self):
        bridge, _, rpc, _ = self.composition()
        rpc.fail_method = "turn/start"
        receipt = bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
        self.assertEqual(receipt.status, "unknown_after_reservation")
        before = list(rpc.calls)
        with self.assertRaisesRegex(ObservationRejected, "already reserved"):
            bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
        self.assertEqual(rpc.calls, before)
        reopened, _, new_rpc, _ = self.composition()
        with self.assertRaises(ObservationRejected):
            reopened.run_once(request(), prompt=PROMPT, cwd=CWD)
        self.assertEqual(new_rpc.calls, [])

    def test_host_config_or_readback_identity_mismatch_never_claims_success(self):
        for failure in ("config", "thread", "turn", "prompt", "ephemeral"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as temporary:
                self.ledger = Path(temporary) / "ledger"
                rpc = ScriptedRpc()
                if failure == "config":
                    rpc.start_mutation = lambda started: started.update(reasoningEffort="xhigh")
                elif failure == "thread": rpc.read["thread"]["id"] = "other-thread"
                elif failure == "turn": rpc.read["thread"]["turns"][0]["id"] = "other-turn"
                elif failure == "prompt": rpc.read["thread"]["turns"][0]["items"][0]["content"][0]["text"] = "substituted"
                else: rpc.start_mutation = lambda started: started["thread"].update(ephemeral=True)
                bridge, _, _, _ = self.composition(rpc=rpc)
                receipt = bridge.run_once(request(), prompt=PROMPT, cwd=CWD)
                self.assertEqual(receipt.status, "configured_route_rejected")
                self.assertIsNone(receipt.observed_actual)
                if failure in {"config", "ephemeral"}:
                    self.assertNotIn("turn/start", [method for method, _ in rpc.calls])

    def test_two_compositions_race_one_stable_ledger_only_one_host_starts(self):
        first = self.composition()
        second = self.composition()
        barrier = threading.Barrier(2)
        outcomes = []
        def run(composition):
            barrier.wait()
            try:
                outcomes.append(composition[0].run_once(request(), prompt=PROMPT, cwd=CWD))
            except ObservationRejected as error:
                outcomes.append(error)
        threads = [threading.Thread(target=run, args=(composition,)) for composition in (first, second)]
        for thread in threads: thread.start()
        for thread in threads: thread.join(timeout=5)
        self.assertTrue(all(not thread.is_alive() for thread in threads))
        self.assertEqual(len(outcomes), 2)
        self.assertEqual(sum(isinstance(value, ObservationRejected) for value in outcomes), 1)
        calls = first[2].calls + second[2].calls
        self.assertEqual(sum(method == "thread/start" for method, _ in calls), 1)
        self.assertEqual(sum(method == "turn/start" for method, _ in calls), 1)


if __name__ == "__main__":
    unittest.main()
