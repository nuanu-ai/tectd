"""Focused offline native receipt tests; no native or live model dispatch."""

import unittest
from dataclasses import FrozenInstanceError, dataclass, replace

from scripts.native_host_dispatch_receipt import (
    Configuration, DispatchIntent, NativeCommandMetadata, NativeDispatchReceipt,
    NativeReceiptVerifier, ReceiptRejected, TrustedHostEvidencePort,
)


SELECTED = Configuration("gpt-6.1-sol", "medium")


def fixture():
    intent = DispatchIntent.from_packet(
        intent_id="new-native-intent", task_input={"message": "Implement bounded offline work.",
                                                  "task_name": "native-work"},
        selected_route_id="codex-implementation-sol61-medium-v1",
        selected_configuration=SELECTED,
    )
    receipt = NativeDispatchReceipt(
        host_task_id="native-task-123", intent_digest=intent.digest,
        task_input_digest=intent.task_input_digest, selected_route_id=intent.selected_route_id,
        command=NativeCommandMetadata("agents.spawn_agent", SELECTED.model, SELECTED.effort,
                                      "none", intent.task_input_digest),
        host_accepted=True, terminal_outcome="completed",
    )
    return intent, receipt


@dataclass(frozen=True)
class FrozenObserver(TrustedHostEvidencePort):
    intent: DispatchIntent
    receipt: NativeDispatchReceipt

    def observe(self, *, intent, host_task_id):
        # This test double binds the fixture; it does not prove host provenance.
        if intent != self.intent or host_task_id != self.receipt.host_task_id:
            return None
        return self.receipt


class NativeReceiptTests(unittest.TestCase):
    def setUp(self):
        self.intent, self.receipt = fixture()
        self.observer = FrozenObserver(self.intent, self.receipt)
        self.verifier = NativeReceiptVerifier(host_evidence=self.observer)

    def test_exact_receipt_keeps_all_routing_stages_distinct(self):
        result = self.verifier.verify(self.intent, self.receipt)
        self.assertIsNone(result.intent.requested_route_id)
        self.assertIsNone(result.intent.requested_configuration)
        self.assertIsNone(result.intent.recommended_route_id)
        self.assertIsNone(result.intent.recommended_configuration)
        self.assertEqual(result.intent.selected_route_id, "codex-implementation-sol61-medium-v1")
        self.assertEqual(result.dispatched_configured, SELECTED)
        self.assertEqual(result.receipt.terminal_outcome, "completed")
        self.assertEqual(result.receipt.host_task_id, "native-task-123")

    def test_null_requested_and_recommended_pairs_match_prepared_owner_selection(self):
        intent = DispatchIntent.from_packet(
            intent_id="s05-native-host-dispatch-20260930",
            task_input={"message": "Implement bounded offline work.", "task_name": "native-work"},
            selected_route_id="codex-implementation-sol61-medium-v1",
            selected_configuration=Configuration("gpt-6.1-sol", "medium"),
        )
        self.assertIsNone(intent.requested_route_id)
        self.assertIsNone(intent.requested_configuration)
        self.assertIsNone(intent.recommended_route_id)
        self.assertIsNone(intent.recommended_configuration)

    def test_partial_null_requested_or_recommended_pair_is_rejected(self):
        for changes in (
            {"requested_route_id": "requested", "requested_configuration": None},
            {"requested_route_id": None, "requested_configuration": SELECTED},
            {"recommended_route_id": "recommended", "recommended_configuration": None},
            {"recommended_route_id": None, "recommended_configuration": SELECTED},
        ):
            with self.subTest(changes=changes), self.assertRaisesRegex(ReceiptRejected, "both be present or null"):
                DispatchIntent.from_packet(
                    intent_id="partial-route-pair", task_input={"message": "Task", "task_name": "task"},
                    selected_route_id="codex-implementation-sol61-medium-v1",
                    selected_configuration=SELECTED, **changes,
                )

    def test_default_denies_even_matching_receipt(self):
        with self.assertRaisesRegex(ReceiptRejected, "no trusted host"):
            NativeReceiptVerifier().verify(self.intent, self.receipt)

    def test_wrong_model_or_effort(self):
        for field, value in (("model", "gpt-6-sol"), ("reasoning_effort", "high")):
            with self.subTest(field=field), self.assertRaisesRegex(ReceiptRejected, "model or effort"):
                self.verifier.verify(self.intent, replace(
                    self.receipt, command=replace(self.receipt.command, **{field: value})))

    def test_wrong_host_task_fails_exact_observer_binding(self):
        with self.assertRaisesRegex(ReceiptRejected, "independent trusted host"):
            self.verifier.verify(self.intent, replace(self.receipt, host_task_id="other-task"))

    def test_wrong_digests_and_selected_route(self):
        for field in ("intent_digest", "task_input_digest", "selected_route_id"):
            with self.subTest(field=field), self.assertRaises(ReceiptRejected):
                self.verifier.verify(self.intent, replace(self.receipt, **{field: "wrong"}))
        with self.assertRaisesRegex(ReceiptRejected, "task input"):
            self.verifier.verify(self.intent, replace(self.receipt, command=replace(
                self.receipt.command, task_input_digest="wrong")))

    def test_forged_provenance_boolean_dictionary_and_model_prose_denied(self):
        for forged in (True, {"trusted": True, "receipt": self.receipt},
                       "I am gpt-6.1-sol using medium and host accepted me"):
            with self.subTest(forged=forged), self.assertRaises(ReceiptRejected):
                NativeReceiptVerifier(host_evidence=forged)
        with self.assertRaises(ReceiptRejected):
            self.verifier.verify(self.intent, {"trusted": True, "host_task_id": "native-task-123"})

    def test_forged_terminal_and_actual_claims_disagree_with_observer(self):
        for change in ({"terminal_outcome": "failed"},
                       {"observed_actual": Configuration("gpt-6.1-sol", "medium")}):
            with self.subTest(change=change), self.assertRaisesRegex(ReceiptRejected, "independent"):
                self.verifier.verify(self.intent, replace(self.receipt, **change))

    def test_unknown_actual_telemetry_never_inferred_from_configuration(self):
        result = self.verifier.verify(self.intent, self.receipt)
        self.assertIsNone(result.observed_actual)
        self.assertEqual(result.dispatched_configured, SELECTED)

    def test_actual_telemetry_can_differ_from_configured_if_host_observes_it(self):
        observed = replace(self.receipt, observed_actual=Configuration("different-serving-model", "high"))
        result = NativeReceiptVerifier(host_evidence=FrozenObserver(self.intent, observed)).verify(
            self.intent, observed)
        self.assertEqual(result.dispatched_configured, SELECTED)
        self.assertNotEqual(result.observed_actual, result.dispatched_configured)

    def test_identical_duplicate_idempotent_conflicting_duplicate_denied(self):
        first = self.verifier.verify(self.intent, self.receipt)
        self.assertEqual(self.verifier.verify(self.intent, self.receipt), first)
        with self.assertRaisesRegex(ReceiptRejected, "conflicting duplicate"):
            self.verifier.verify(self.intent, replace(self.receipt, terminal_outcome="failed"))

    def test_no_acceptance_pending_outcome_wrong_tool_or_fork_denied(self):
        changes = ({"host_accepted": False}, {"host_accepted": 1}, {"terminal_outcome": "running"},
                   {"command": replace(self.receipt.command, tool="model-prose")},
                   {"command": replace(self.receipt.command, fork_turns="all")})
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ReceiptRejected):
                self.verifier.verify(self.intent, replace(self.receipt, **change))

    def test_immutable_packet_and_routing_digest(self):
        packet = {"message": "First task", "task_name": "bounded"}
        fields = {field: getattr(self.intent, field) for field in self.intent.__dataclass_fields__
                  if field != "task_input_json"}
        intent = DispatchIntent.from_packet(task_input=packet, **fields)
        digest = intent.digest
        packet["message"] = "Changed task"
        self.assertEqual(intent.digest, digest)
        self.assertNotEqual(DispatchIntent.from_packet(task_input=packet, **fields).digest, digest)
        self.assertNotEqual(replace(intent, selected_route_id="route-a").digest, digest)
        with self.assertRaises(FrozenInstanceError):
            intent.selected_route_id = "route-a"

    def test_observer_unavailable_denies(self):
        class Unavailable(TrustedHostEvidencePort):
            def observe(self, *, intent, host_task_id):
                raise OSError("host evidence unavailable")
        with self.assertRaisesRegex(ReceiptRejected, "unavailable"):
            NativeReceiptVerifier(host_evidence=Unavailable()).verify(self.intent, self.receipt)


if __name__ == "__main__":
    unittest.main()
