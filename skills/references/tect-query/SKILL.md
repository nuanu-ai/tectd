---
name: tect-query
description: Use when a request needs current truth, evidence lookup, workspace state, knowledge lookup, artifact location, source provenance, or memory/session context without mutation.
---

# tect-query

Use this public front door to route read-only questions through Kernel and Runtime gates. It classifies query intent, source class, freshness, authority posture, required reads, and the next owner before any answer is trusted.

Canonical public surface mapping: `capabilities/registry/public-surfaces.json`.
Shared packet contract: `capabilities/contracts/public-route-packet.schema.json`.

## Route Materialization Requirement

Public route `tect-query` is a front-door owner, not concrete process identity. Before consequential advice, the agent must materialize an exact route through `capabilities/registry/route-materialization-map.json` or `tools/materialize-layer12-route.mjs`.

The materialized packet must conform to `capabilities/contracts/route-materialization.schema.json` and include the selected `pipeline_id` or `service_id`, source manifest or service descriptor path, entry gate or service contract status, exact `internal-instruction.*` refs, source refs, and all forbidden claims set to `false`.

If no exact route materialization record exists, stop at ask, blocked, handoff, or proposal-only route packet. Do not guess a pipeline, do not treat `public_route` as a concrete process, and do not proceed with consequential advice.

Do not inline internal instruction body prose into this public skill. Internal bodies are loaded only by exact `instruction_ref` after route materialization, and only as needed for the selected owner.


## Query Boundary

This surface handles query-shaped requests: current state, prior decision, source location, artifact provenance, workspace root, generated projection, durable knowledge, runbook/protocol lookup, memory/session context, stale claim, missing proof, or contradiction search.

Treat generated projections and local indexes as routing aids, not truth. Treat memory/session material as historical or derived unless a later owner refreshes it or accepts it into canonical source.

Setup-first gate: if Tect control state is missing, invalid, partial, unknown, or shows a legacy `workspace/` overlay, route to `tect-adopt` before work/query/build/maintain behavior. Canonical tracked root is `tect/`; generated/local root is `.tect/`.

Setup profile semantics gate: setup profile value is only `lightweight` or `full`; profile state is separate: `proposed`, `approved`, `applied`, `verified`, `partial`, or `blocked`. Route missing, partial, invalid, hollow, blocked, or unverified setup profile state to `tect-adopt`. Do not claim `full` unless setup-profile semantics proof would pass: exact full surfaces plus canonical structure proof, setup ledger, package pin, and setup profile source refs. Keep setup profile `lightweight/full` separate from slice lifecycle depth `lightweight/full` and team add-ons.

Workspace front-door boundary gate: classify read targets before answering. Durable facts, runbooks, protocols, procedures, rules, research, domain knowledge, claim ledgers, and promotion logs read from `tect/knowledge/...`; current work state, Program/Scope/Slice/Result status, deferred work, handoff, and promotion edges read from `tect/programs/...`. `.tect/...` indexes can help locate sources but are not source truth.

Context-bounded source lookup gate: every non-trivial query must be routed through a source family and a compact output budget before material is loaded into the main conversation. Use front doors, indexes, and targeted snippets. Do not use broad plugin-wide `rg`, grep, all-docs, all-files, whole-workspace, dump-everything, or `X more internal shards` style output as a public answer. If the requested proof needs more than the budget allows, report omitted sources and route to source-family extraction, research slice, maintenance, or context-handoff.

## Route Selection

For read-side durable knowledge lookup, use `next_owner: tect-query`. Name durable knowledge sources in `required_reads`, `source_refs`, `blocked_by`, or `next_action.label`; do not select durable domain owners directly from this surface.

For source lookup, include the source family and budget source in `required_reads` or `next_action`. Valid source families come from `capabilities/registry/source-families.json` or the workspace-local source-family registry. Generated indexes and projections can narrow the route, but the answer must preserve source class, freshness, source distance, citations, and omitted-source notes.

Workspace root, location, placement, and index-scope questions may use `next_owner: workspace-map`. Artifact provenance, proof/freshness, current/live/deployment/git/CI/filesystem/package/browser/DB/chain/user-visible evidence, cleanup paths, and promotion targets stay visible through `required_reads`, `blocked_by`, `source_refs`, or `next_action.label` under a compatible query owner.

Maintenance-shaped requests, including stale projections, stale indexes, missing results, front-door drift, cleanup pressure, and contradictions, switch to `public_route: tect-maintain` or remain represented as required reads or blockers. Route memory/session material through privacy and freshness custody: consent requests go to `user`; hard refusals use `public_route: blocked` with `next_owner: none`.

If the request becomes development, debugging, operations, research execution, procedure capture, durable domain write, domain repair, result promotion, or stateful-domain pipeline entry, switch to `public_route: tect-work` instead of answering inside this surface.

## Route Packet

```text
target:
abstraction_level:
public_route:
next_owner:
required_reads:
authority_posture:
truth_freshness:
blocked_by:
next_action:
```

## Hard Boundaries

- This skill does not execute pipelines.
- This skill does not mutate source.
- This skill does not deploy.
- This skill does not promote.
- This skill does not delete.
- This skill does not clean branches or worktrees.
- This skill does not claim completion.
- This skill does not perform unbounded plugin-wide or workspace-wide search output.
- This skill does not hide over-budget, omitted, stale, or generated-only source material.
