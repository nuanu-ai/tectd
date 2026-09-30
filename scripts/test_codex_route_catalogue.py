"""Offline policy/binding tests; no model or host dispatch."""

import hashlib
import json
import unittest
from dataclasses import FrozenInstanceError, replace

from scripts.codex_route_catalogue import (
    SelectionRejected, development_catalogue, select_route, one_off_catalogue,
    one_off_prompt, select_one_off_route,
)


class CatalogueTests(unittest.TestCase):
    def test_separate_fixed_one_off_policy_never_extends_default(self):
        self.assertEqual(development_catalogue().digest,
                         "6bb52b041cda0ad8effbe18f71854d18a0a007b12f5d3f616e56ae3a887850be")
        catalogue = one_off_catalogue()
        self.assertNotEqual(catalogue.schema, development_catalogue().schema)
        self.assertEqual(len(catalogue.routes), 1)
        digest = hashlib.sha256(one_off_prompt().encode()).hexdigest()
        selection = select_one_off_route(task_input_digest=digest)
        self.assertEqual((selection.model, selection.effort, selection.purpose),
                         ("gpt-5.6-luna", "xhigh", "routine"))
        for changed in ({"task_input_digest": "0" * 64}, {"model": "gpt-6-luna"},
                        {"catalogue_digest": development_catalogue().digest},
                        {"catalogue_version": True}, {"policy_source": "arbitrary authority"}):
            with self.subTest(changed=changed), self.assertRaises(SelectionRejected):
                replace(selection, **changed)
        with self.assertRaises(SelectionRejected):
            select_one_off_route(task_input_digest="0" * 64)
        with self.assertRaises(SelectionRejected):
            select_route(**dict(self.fields, model="gpt-5.6-luna", effort="xhigh"))

    def setUp(self):
        self.catalogue = development_catalogue()
        self.task_digest = hashlib.sha256(b"Exact bounded task input").hexdigest()
        route = self.catalogue.routes[0]
        self.fields = dict(route_id=route.route_id, model=route.model, effort=route.effort,
                           purpose=route.purpose, task_input_digest=self.task_digest,
                           catalogue_version=self.catalogue.version,
                           catalogue_digest=self.catalogue.digest)

    def test_exact_finite_owner_routes_and_unverified_availability(self):
        self.assertEqual([(r.model, r.effort, r.purpose) for r in self.catalogue.routes],
                         [("gpt-6.1-sol", "medium", "ordinary_implementation"),
                          ("gpt-6-luna", "xhigh", "routine")])
        self.assertEqual(self.catalogue.host_transport, "codex-app-server-stdio")
        self.assertEqual(self.catalogue.model_availability, "unverified")
        self.assertTrue(self.catalogue.policy_source)
        self.assertFalse(any(r.route_id == "route-a" for r in self.catalogue.routes))

    def test_canonical_catalogue_digest_and_configuration_drift(self):
        decoded = json.loads(self.catalogue.canonical_json)
        canonical = json.dumps(decoded, sort_keys=True, separators=(",", ":"),
                               ensure_ascii=False, allow_nan=False)
        self.assertEqual(canonical, self.catalogue.canonical_json)
        self.assertEqual(hashlib.sha256(canonical.encode()).hexdigest(), self.catalogue.digest)
        for changes in ({"version": 2}, {"policy_source": "different policy"},
                        {"host_transport": "agents.spawn_agent"},
                        {"routes": tuple(reversed(self.catalogue.routes))}):
            with self.subTest(changes=changes):
                self.assertNotEqual(replace(self.catalogue, **changes).digest, self.catalogue.digest)

    def test_both_explicit_routes_select_without_automatic_choice(self):
        for route in self.catalogue.routes:
            selection = select_route(**dict(self.fields, route_id=route.route_id,
                                           model=route.model, effort=route.effort,
                                           purpose=route.purpose))
            self.assertEqual(selection.route_id, route.route_id)
            self.assertEqual(selection.task_input_digest, self.task_digest)
            self.assertEqual(selection.catalogue_digest, self.catalogue.digest)

    def test_unlisted_route_model_effort_and_purpose_reject(self):
        for field, value in (("route_id", "route-a"), ("model", "gpt-6-sol"),
                             ("model", "gpt-6-astra"), ("effort", "high"),
                             ("effort", "ultra"), ("purpose", "routine")):
            with self.subTest(field=field, value=value), self.assertRaises(SelectionRejected):
                select_route(**dict(self.fields, **{field: value}))

    def test_catalogue_version_digest_drift_reject(self):
        for field, value in (("catalogue_version", 2), ("catalogue_version", True),
                             ("catalogue_version", "1"), ("catalogue_digest", "0" * 64)):
            with self.subTest(field=field, value=value), self.assertRaises(SelectionRejected):
                select_route(**dict(self.fields, **{field: value}))

    def test_digest_format_reject(self):
        for value in (None, "", "x" * 64, "A" * 64, "a" * 63, "a" * 64 + "\n"):
            with self.subTest(value=value), self.assertRaises(SelectionRejected):
                select_route(**dict(self.fields, task_input_digest=value))

    def test_selection_binding_changes_with_task_or_route_and_is_immutable(self):
        selection = select_route(**self.fields)
        changed = select_route(**dict(self.fields, task_input_digest="0" * 64))
        self.assertNotEqual(selection.digest, changed.digest)
        routine = self.catalogue.routes[1]
        changed = select_route(**dict(self.fields, route_id=routine.route_id,
                                     model=routine.model, effort=routine.effort,
                                     purpose=routine.purpose))
        self.assertNotEqual(selection.digest, changed.digest)
        with self.assertRaises(FrozenInstanceError):
            selection.purpose = "routine"
        with self.assertRaises(FrozenInstanceError):
            self.catalogue.routes[0].model = "other"

    def test_direct_selection_construction_or_replacement_cannot_bypass_policy(self):
        selection = select_route(**self.fields)
        for changes in ({"model": "gpt-6-astra"}, {"purpose": "routine"},
                        {"host_transport": "agents.spawn_agent"}, {"catalogue_schema": "other"}):
            with self.subTest(changes=changes), self.assertRaises(SelectionRejected):
                replace(selection, **changes)


if __name__ == "__main__":
    unittest.main()
