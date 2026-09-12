from __future__ import annotations

import copy
import unittest
import uuid

import scope_candidate_cycles as cycles
from fixture import CANDIDATE_AMENDMENT
from scope_candidates import candidate_model_prompt


def candidate(identifier: str, revision: int, title: str) -> dict:
    return {
        "id": identifier, "revision": revision, "title": title, "outcome": title,
        "trigger": "trigger", "delivered_behavior": title, "proof": "proof",
        "includes": [], "excludes": [], "dependencies": [],
        "coverage_goal_ids": [], "evidence_ids": [],
    }


class CandidateCycleTests(unittest.TestCase):
    def test_prompt_preserves_exact_amendment_without_prescribing_candidate_categories(self) -> None:
        prompt = candidate_model_prompt(str(uuid.uuid4()))
        self.assertIn(CANDIDATE_AMENDMENT, prompt)
        self.assertIn("Only after the first planning cycle reaches Ready", prompt)
        self.assertNotIn("four candidates", prompt.lower())
        self.assertNotIn("database candidate", prompt.lower())
        self.assertNotIn("api candidate", prompt.lower())
        self.assertNotIn("ui candidate", prompt.lower())

    def test_delta_partition_accepts_flexible_grouping_and_exact_revisions(self) -> None:
        a, b, c, d = [str(uuid.uuid4()) for _ in range(4)]
        old_a, old_b, old_c = candidate(a, 1, "A"), candidate(b, 1, "B"), candidate(c, 1, "C")
        new_a, new_b, new_d = copy.deepcopy(old_a), candidate(b, 2, "B changed"), candidate(d, 1, "D")
        previous = {"candidates": [old_a, old_b, old_c]}
        current = {"candidates": [new_a, new_b, new_d], "delta": {
            "added": [{"candidate_id": d, "revision": 1}],
            "changed": [{"candidate_id": b, "from_revision": 1, "to_revision": 2,
                         "rationale": "The amendment changes this result."}],
            "unchanged": [{"candidate_id": a, "revision": 1}],
            "superseded": [{"prior": old_c, "reason": "The amendment replaces this result.",
                            "replacement_candidate_ids": [d]}],
        }}
        cycles._assert_delta(previous, current)
        broken = copy.deepcopy(current)
        broken["delta"]["superseded"] = []
        with self.assertRaises(AssertionError):
            cycles._assert_delta(previous, broken)
        duplicated = copy.deepcopy(current)
        duplicated["delta"]["unchanged"].append(copy.deepcopy(duplicated["delta"]["unchanged"][0]))
        with self.assertRaises(AssertionError):
            cycles._assert_delta(previous, duplicated)

    def test_cycle_bindings_require_stale_refresh_and_a_new_snapshot(self) -> None:
        first = {"candidate_set": {"revision": 2, "input_cursor": 1}, "snapshot": {"id": "first"}}
        first_ready = {"candidate_set": {"revision": 3}}
        recorded = {"candidate_set": {"revision": 4, "input_cursor": 1, "latest_input": 2}}
        refreshed = {"candidate_set": {"revision": 5, "input_cursor": 1, "status": "review_required"},
                     "snapshot": {"id": "second"}}
        second = {"candidate_set": {"revision": 6, "input_cursor": 2}, "snapshot": {"id": "second"}}
        second_ready = {"candidate_set": {"revision": 7}, "snapshot": {"id": "second"}}
        first_ready["snapshot"] = {"id": "first"}
        recorded["snapshot"] = {"id": "first"}
        cycles._assert_cycle_bindings(first, first_ready, recorded, refreshed, second, second_ready)
        broken = copy.deepcopy(refreshed)
        broken["candidate_set"]["status"] = "ready"
        with self.assertRaises(AssertionError):
            cycles._assert_cycle_bindings(first, first_ready, recorded, broken, second, second_ready)
        switched = copy.deepcopy(recorded)
        switched["snapshot"] = {"id": "unexpected"}
        with self.assertRaises(AssertionError):
            cycles._assert_cycle_bindings(first, first_ready, switched, refreshed, second, second_ready)

    def test_tail_reads_require_every_call_after_manual_history_to_be_offered(self) -> None:
        def read(view: str, actions: list[dict] | None = None, **params: object) -> dict:
            return {"tool": "query", "arguments": {"route": "scope.candidates.context",
                    "params": {"view": view, **params}}, "payload": {"actions": actions or []}}
        historical_args = {"route": "scope.candidates.context", "params": {
            "view": "historical", "draft_revision": 2, "limit": 25,
        }}
        fragment_args = {"route": "scope.candidates.context", "params": {
            "view": "fragment", "draft_revision": 2, "source_ref_id": "source", "cursor": 0,
        }}
        overview_args = {"route": "scope.candidates.context", "params": {"view": "overview"}}
        action = lambda arguments: [{"kind": "ready_call", "tool": "query", "arguments": arguments}]
        calls = [
            read("history", action(historical_args)),
            read("historical", action(fragment_args), draft_revision=2, limit=25),
            read("fragment", action(overview_args), draft_revision=2, source_ref_id="source", cursor=0),
            read("overview"),
        ]
        self.assertEqual(cycles._tail_reads(calls, -1), calls)
        broken = copy.deepcopy(calls)
        broken[-1]["arguments"]["params"]["unexpected"] = True
        with self.assertRaises(AssertionError):
            cycles._tail_reads(broken, -1)

    def test_history_preserves_status_reason_and_replacements(self) -> None:
        a, b, c, d = [str(uuid.uuid4()) for _ in range(4)]
        old_a, old_b, old_c = candidate(a, 1, "A"), candidate(b, 1, "B"), candidate(c, 1, "C")
        new_a, new_b, new_d = copy.deepcopy(old_a), candidate(b, 2, "B changed"), candidate(d, 1, "D")
        continuation = {"candidates": [new_a, new_b, new_d], "delta": {
            "added": [{"candidate_id": d, "revision": 1}],
            "changed": [{"candidate_id": b, "from_revision": 1, "to_revision": 2, "rationale": "changed"}],
            "unchanged": [{"candidate_id": a, "revision": 1}],
            "superseded": [{"prior": old_c, "reason": "replaced", "replacement_candidate_ids": [d]}],
        }}
        def entry(identifier: str, revision: int, status: str, reason: str | None = None,
                  replacements: list[str] | None = None) -> dict:
            return {"history": {"candidate_id": identifier, "candidate_revision": revision,
                    "status": status, "superseded_reason": reason,
                    "replacement_candidate_ids": replacements or []}}
        history = [{"payload": {"items": [entry(a, 1, "active"), entry(b, 1, "prior"),
                    entry(b, 2, "active"), entry(c, 1, "superseded", "replaced", [d]),
                    entry(d, 1, "active")]}}]
        cycles._assert_history({"candidates": [old_a, old_b, old_c]}, [continuation], history)
        broken = copy.deepcopy(history)
        broken[0]["payload"]["items"][3]["history"]["replacement_candidate_ids"] = []
        with self.assertRaises(AssertionError):
            cycles._assert_history({"candidates": [old_a, old_b, old_c]}, [continuation], broken)
        wrong_status = copy.deepcopy(history)
        wrong_status[0]["payload"]["items"][1]["history"]["status"] = "active"
        with self.assertRaises(AssertionError):
            cycles._assert_history({"candidates": [old_a, old_b, old_c]}, [continuation], wrong_status)

    def test_historical_fragment_reconstruction_requires_exact_revision_and_byte_order(self) -> None:
        source = str(uuid.uuid4())
        calls = [{
            "tool": "query", "arguments": {"route": "scope.candidates.context", "params": {
                "view": "fragment", "source_ref_id": source, "cursor": 0, "draft_revision": 2,
            }},
            "payload": {"fragment": {"source_ref": {"id": source}, "cursor": 0,
                                     "text": "exact", "next_cursor": None}},
        }]
        self.assertEqual(cycles._fragments(calls, 2), {source: "exact"})
        self.assertEqual(cycles._fragments(calls, None), {})
        calls[0]["arguments"]["params"]["cursor"] = 1
        with self.assertRaises(AssertionError):
            cycles._fragments(calls, 2)


if __name__ == "__main__":
    unittest.main()
