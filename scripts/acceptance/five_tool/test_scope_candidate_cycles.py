from __future__ import annotations

import copy
import unittest
import uuid

import scope_candidate_cycles as cycles
import scope_candidates
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
        worktree = str(uuid.uuid4())
        prompt = candidate_model_prompt(str(uuid.uuid4()), worktree)
        self.assertIn(CANDIDATE_AMENDMENT, prompt)
        self.assertIn(worktree, prompt)
        self.assertIn("query source.list with limit 25", prompt)
        self.assertIn("session.select_worktrees with exactly that one ID", prompt)
        self.assertIn("Only after the first planning cycle reaches Ready", prompt)
        self.assertNotIn("four candidates", prompt.lower())
        self.assertNotIn("database candidate", prompt.lower())
        self.assertNotIn("api candidate", prompt.lower())
        self.assertNotIn("ui candidate", prompt.lower())

    def test_recovery_requires_the_exact_backend_action_before_continuation(self) -> None:
        def call(tool: str, arguments: dict, *, failed: bool = False,
                 source: str = "mcp_wire", actions: list[dict] | None = None) -> dict:
            return {
                "source": source, "response_source": source, "forwarded": source == "mcp_wire",
                "server": "tectd", "tool": tool, "arguments": arguments,
                "status": "failed" if failed else "completed", "is_error": failed,
                "error_code": "invalid_arguments" if failed else None,
                "payload": {"actions": actions or []},
            }
        get_state = {"kind": "ready_call", "tool": "get_state", "arguments": {}}
        clean = [call("help", {"mode": "describe", "tool": "query"})]
        self.assertEqual(scope_candidates.recovered_backend_failures(clean), set())
        scope_candidates.assert_successful_calls(clean, {"help"})
        recovered = [
            call("help", {"mode": "describe"}, failed=True, actions=[get_state]),
            call("get_state", {}),
            *clean,
        ]
        indexes = scope_candidates.recovered_backend_failures(recovered)
        self.assertEqual(indexes, {0})
        scope_candidates.assert_successful_calls(recovered, {"get_state", "help"}, indexes)
        with self.assertRaises(AssertionError):
            scope_candidates.recovered_backend_failures([recovered[0], *clean])
        mutation_first = [recovered[0], call("command", {"route": "scope.candidates.save", "params": {}})]
        with self.assertRaises(AssertionError):
            scope_candidates.recovered_backend_failures(mutation_first)
        blocked = [
            call("help", {"mode": "describe"}, failed=True,
                 source="fixture_capture", actions=[get_state]),
            call("get_state", {}),
        ]
        with self.assertRaises(AssertionError):
            scope_candidates.recovered_backend_failures(blocked)

    def test_owned_source_setup_is_exact_ordered_and_bound_to_child(self) -> None:
        actor, worktree = str(uuid.uuid4()), str(uuid.uuid4())
        def item(tool: str, arguments: dict, payload: dict) -> dict:
            return {"tool": tool, "arguments": arguments, "payload": payload,
                    "actor_thread_id": actor}
        calls = [
            item("get_state", {}, {}),
            item("command", {"route": "workspace.open", "params": {}}, {}),
            item("query", {"route": "source.list", "params": {"limit": 25}},
                 {"items": [{"id": worktree}], "next_after": None}),
            item("command", {"route": "session.select_worktrees",
                 "params": {"worktree_ids": [worktree]}},
                 {"selected_worktrees": [{"id": worktree}]}),
            item("query", {"route": "scope.candidates.context", "params": {"view": "overview"}}, {}),
        ]
        expected = cycles._assert_source_setup(calls, len(calls), {"worktree_id": worktree})
        self.assertEqual(expected[0][1]["route"], "source.list")
        wrong = copy.deepcopy(calls)
        wrong[3]["arguments"]["params"]["worktree_ids"] = [str(uuid.uuid4())]
        with self.assertRaises(AssertionError):
            cycles._assert_source_setup(wrong, len(wrong), {"worktree_id": worktree})
        out_of_order = [*calls[:2], calls[4], calls[2], calls[3]]
        with self.assertRaises(AssertionError):
            cycles._assert_source_setup(out_of_order, len(out_of_order), {"worktree_id": worktree})
        for duplicate in (calls[2], calls[3]):
            with self.assertRaises(AssertionError):
                cycles._assert_source_setup([*calls, duplicate], len(calls), {"worktree_id": worktree})
        early_refresh = [*calls[:3], item("command", {
            "route": "scope.candidates.refresh", "params": {},
        }, {}), *calls[3:]]
        with self.assertRaises(AssertionError):
            cycles._assert_source_setup(early_refresh, len(early_refresh), {"worktree_id": worktree})
        snapshots = [
            {"payload": {"context": {"snapshot": {"selected_worktree_ids": [worktree]}}}},
            {"payload": {"historical": {"snapshot": {"selected_worktree_ids": [worktree]}}}},
        ]
        cycles._assert_source_snapshots(snapshots, {"worktree_id": worktree})
        snapshots[1]["payload"]["historical"]["snapshot"]["selected_worktree_ids"] = []
        with self.assertRaises(AssertionError):
            cycles._assert_source_snapshots(snapshots, {"worktree_id": worktree})

    def test_lineage_accepts_explicit_source_setup_and_backend_offered_early_refresh(self) -> None:
        actor, worktree = str(uuid.uuid4()), str(uuid.uuid4())
        def action(tool: str, route: str | None = None) -> list[dict]:
            arguments = {} if route is None else {"route": route, "params": {}}
            return [{"kind": "ready_call", "tool": tool, "arguments": arguments}]
        listed = {"route": "source.list", "params": {"limit": 25}}
        selected = {"route": "session.select_worktrees", "params": {"worktree_ids": [worktree]}}
        context = {"route": "scope.candidates.context", "params": {"view": "overview"}}
        calls = [
            {"tool": "get_state", "arguments": {}, "payload": {"actions": action("command", "workspace.open")}},
            {"tool": "command", "arguments": {"route": "workspace.open", "params": {}}, "payload": {}},
            {"tool": "query", "arguments": listed, "payload": {}},
            {"tool": "command", "arguments": selected,
             "payload": {"actions": [{"kind": "ready_call", "tool": "query", "arguments": context}]}},
            {"tool": "query", "arguments": context,
             "payload": {"actions": action("command", "scope.candidates.refresh")}},
            {"tool": "command", "arguments": {"route": "scope.candidates.refresh", "params": {}},
             "payload": {}},
            {"tool": "help", "arguments": {"mode": "describe"},
             "payload": {"actions": action("get_state")}},
            {"tool": "get_state", "arguments": {}, "payload": {}},
        ]
        cycles._assert_offered_reads(calls, explicit=[("query", listed), ("command", selected)])
        broken = copy.deepcopy(calls)
        broken[5]["arguments"]["params"]["unexpected"] = True
        with self.assertRaises(AssertionError):
            cycles._assert_offered_reads(broken, explicit=[("query", listed), ("command", selected)])

    def test_amendment_requires_one_refresh_only_inside_its_window(self) -> None:
        refresh = {"tool": "command", "arguments": {"route": "scope.candidates.refresh", "params": {}}}
        calls = [copy.deepcopy(refresh), {"tool": "command", "arguments": {"route": "scope.candidates.record_input"}},
                 copy.deepcopy(refresh), {"tool": "command", "arguments": {"route": "scope.candidates.save"}}]
        self.assertEqual(cycles._amendment_refresh_index(calls, 1, 3), 2)
        calls.insert(3, copy.deepcopy(refresh))
        with self.assertRaises(AssertionError):
            cycles._amendment_refresh_index(calls, 1, 4)

    def test_authored_body_preserves_every_backend_control_and_declared_path(self) -> None:
        set_id, snapshot_id, request_id = [str(uuid.uuid4()) for _ in range(3)]
        action = {
            "kind": "needs_input", "tool": "command",
            "arguments": {"route": "scope.candidates.save", "params": {
                "kind": "draft", "candidate_set_id": set_id, "revision": 5,
                "snapshot_id": snapshot_id, "input_cursor": 2, "request_id": request_id,
            }},
            "input": {"fields": [{"path": "arguments.params.draft", "format": "draft"}]},
        }
        actual = {"tool": "command", "arguments": copy.deepcopy(action["arguments"])}
        actual["arguments"]["params"]["draft"] = {"boundary": "ongoing"}
        cycles._assert_command_template([actual], 0, [{"actions": [action]}], "draft")
        for key, value in (("request_id", str(uuid.uuid4())), ("unexpected", True)):
            broken = copy.deepcopy(actual)
            broken["arguments"]["params"][key] = value
            with self.assertRaises(AssertionError):
                cycles._assert_command_template([broken], 0, [{"actions": [action]}], "draft")
        undeclared = copy.deepcopy(action)
        undeclared["input"]["fields"] = []
        with self.assertRaises(AssertionError):
            cycles._assert_command_template([actual], 0, [{"actions": [undeclared]}], "draft")
        review_only = copy.deepcopy(action)
        review_only["arguments"]["params"]["kind"] = "review"
        review_only["arguments"]["params"]["review"] = {"protected_change_reviews": []}
        review_only["input"]["fields"] = [{"path": "arguments.params.review.verdict"}]
        with self.assertRaises(AssertionError):
            cycles._assert_command_template([actual], 0, [{"actions": [review_only]}], "draft")
        record = {"kind": "needs_input", "tool": "command", "arguments": {
            "route": "scope.candidates.record_input", "params": {
                "candidate_set_id": set_id, "revision": 5, "request_id": request_id,
            }}, "input": {"fields": [{"path": "arguments.params.input"}]}}
        invented = {"tool": "command", "arguments": copy.deepcopy(record["arguments"])}
        invented["arguments"]["params"].update(request_id=str(uuid.uuid4()), input="amendment")
        with self.assertRaises(AssertionError):
            cycles._assert_command_template([invented], 0, [{"actions": [record]}], "input")

    def test_failed_draft_requires_unchanged_head_and_same_backend_controls(self) -> None:
        set_id, snapshot_id, request_id = [str(uuid.uuid4()) for _ in range(3)]
        controls = {
            "kind": "draft", "candidate_set_id": set_id, "revision": 1,
            "snapshot_id": snapshot_id, "input_cursor": 1, "request_id": request_id,
        }
        action = {
            "kind": "needs_input", "tool": "command",
            "arguments": {"route": "scope.candidates.save", "params": controls},
            "input": {"fields": [{"path": "arguments.params.draft", "format": "draft"}]},
        }
        failed_params = {**controls, "draft": {}}
        corrected_params = {**controls, "draft": {"boundary": "ongoing"}}
        calls = [
            {"payload": {"context": {"candidate_set": {
                "id": set_id, "revision": 1, "snapshot_id": snapshot_id, "status": "draft",
            }}, "actions": [action]}},
            {"tool": "command", "arguments": {"route": "scope.candidates.save", "params": failed_params},
             "status": "failed", "is_error": True},
            {"tool": "get_state", "arguments": {}, "status": "completed", "is_error": False,
             "payload": {"candidate_sets": [{
                 "id": set_id, "revision": 1, "snapshot_id": snapshot_id, "status": "draft",
             }]}},
            {"tool": "command", "arguments": {"route": "scope.candidates.save", "params": corrected_params},
             "status": "completed", "is_error": False},
        ]
        cycles._assert_failed_body_recoveries(calls)
        stale = copy.deepcopy(calls)
        stale[2]["payload"]["candidate_sets"][0]["revision"] = 2
        with self.assertRaises(AssertionError):
            cycles._assert_failed_body_recoveries(stale)
        minted = copy.deepcopy(calls)
        minted[3]["arguments"]["params"]["request_id"] = str(uuid.uuid4())
        with self.assertRaises(AssertionError):
            cycles._assert_failed_body_recoveries(minted)

    def test_only_identical_unpinned_current_head_reads_are_reusable(self) -> None:
        set_id = str(uuid.uuid4())
        for view in ["overview", "program", "inputs", "candidates", "reviews"]:
            params = {"candidate_set_id": set_id, "view": view, "limit": 25}
            if view == "inputs":
                params["after"] = 0
            reusable = ("query", {"route": "scope.candidates.context", "params": params})
            read = {"tool": reusable[0], "arguments": reusable[1], "payload": {"actions": []}}
            issued = {"actions": [{
                "kind": "ready_call", "tool": reusable[0], "arguments": reusable[1],
            }]}
            cycles._assert_offered_reads([read, copy.deepcopy(read)], initial=issued)
            for key, value in [
                ("candidate_set_id", str(uuid.uuid4())), ("limit", 24),
                ("after", 1), ("draft_revision", 2),
            ]:
                controlled = copy.deepcopy(read)
                controlled["arguments"]["params"][key] = value
                with self.assertRaises(AssertionError):
                    cycles._assert_offered_reads([controlled], initial=issued)
        for view in ["history", "historical", "fragment"]:
            self.assertFalse(cycles._is_reusable_current_head_read(("query", {
                "route": "scope.candidates.context",
                "params": {"candidate_set_id": set_id, "view": view, "limit": 25},
            })))

    def test_recovered_draft_is_a_transition_that_emits_its_checkpoint(self) -> None:
        set_id = str(uuid.uuid4())
        rejected = {
            "tool": "command", "arguments": {"route": "scope.candidates.save", "params": {}},
            "status": "failed", "is_error": True, "payload": {"actions": [{
                "kind": "ready_call", "tool": "get_state", "arguments": {},
            }]},
        }
        checkpoint = {"tool": "get_state", "arguments": {}, "payload": {"actions": []}}
        cycles._assert_offered_reads(
            [rejected, checkpoint], initial={"actions": []}, recovered_transitions=[rejected],
        )
        with self.assertRaises(AssertionError):
            cycles._assert_offered_reads([rejected, checkpoint], initial={"actions": []})

    def test_repeated_input_page_must_preserve_the_immutable_record(self) -> None:
        value = {"id": str(uuid.uuid4()), "sequence": 2, "source_ref_id": str(uuid.uuid4())}
        def page(item: dict) -> dict:
            return {
                "arguments": {"params": {"view": "inputs"}},
                "payload": {"items": [{"input": item}]},
            }
        self.assertEqual(cycles._input_window([page(value), page(copy.deepcopy(value))]), {2})
        changed = copy.deepcopy(value)
        changed["source_ref_id"] = str(uuid.uuid4())
        with self.assertRaises(AssertionError):
            cycles._input_window([page(value), page(changed)])

    def test_ready_review_allows_only_exact_read_only_inspection(self) -> None:
        candidate_id, set_id = str(uuid.uuid4()), str(uuid.uuid4())
        draft = {"candidates": [{"id": candidate_id}], "blockers": []}
        call = {"payload": {
            "context": {"candidate_set": {"id": set_id, "revision": 3, "status": "ready"}},
            "latest_review": {"verdict": "ready", "candidate_decisions": [
                {"candidate_id": candidate_id, "decision": "accept"},
            ]},
            "actions": [{"kind": "ready_call", "tool": "query", "arguments": {
                "route": "scope.candidates.context", "params": {
                    "candidate_set_id": set_id, "view": "candidates", "limit": 25,
                },
            }}, {"kind": "needs_input", "tool": "command", "arguments": {
                "route": "scope.candidates.record_input", "params": {
                    "candidate_set_id": set_id, "revision": 3, "request_id": str(uuid.uuid4()),
                },
            }, "input": {"fields": [{
                "path": "arguments.params.input",
                "format": "Complete original amendment text without trimming or paraphrasing.",
            }]}}],
            "recommended_action": 0,
        }}
        cycles._assert_ready_review(call, draft)
        mutation = copy.deepcopy(call)
        mutation["payload"]["actions"][0]["tool"] = "command"
        with self.assertRaises(AssertionError):
            cycles._assert_ready_review(mutation, draft)
        missing = copy.deepcopy(call)
        missing["payload"]["actions"] = []
        missing["payload"]["recommended_action"] = None
        with self.assertRaises(AssertionError):
            cycles._assert_ready_review(missing, draft)
        minted = copy.deepcopy(call)
        minted["payload"]["actions"][1]["arguments"]["params"]["request_id"] = None
        with self.assertRaises(AssertionError):
            cycles._assert_ready_review(minted, draft)
        blocked = copy.deepcopy(draft)
        blocked["blockers"] = [{"id": str(uuid.uuid4())}]
        with self.assertRaises(AssertionError):
            cycles._assert_ready_review(call, blocked)

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
