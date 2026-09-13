# Operational Preparation source ledger

## Identity and native contract

- V1 source revision `ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8`; manifest `capabilities/pipelines/slice-variants/operational-preparation.pipeline.json` SHA-256 `5e37d583b69d357b9eb1355f61893294d7d9f248dd426dc366dcca8ab4775b54`; ordered body-set digest `9a47f58a04c6abf64f15ab101247cf4b3b34468167a3f1f2822ceafed330c2f9`.
- Native `slice.operational-preparation@0.1.0-native.1`: `crates/host/pipeline-definitions/operational-preparation.json`, semantic digest `dbe2313ec9b8ccb06768edc330c92d91520c0d5fb61aaa3717a2e0daa2202d30`, physical SHA-256 `bfb00041921d1e0bdc37fe588ebf1f7a056285ead17c390cc93d9f4dab49ce14`.
- Accepted delivery default `whole`; `whole` and `phasewise` allowed. V1 did not define delivery transport.
- Coverage: 16/16 exact internal bodies, 57 verdict routes, 17 artifact requirements, and seven phase skill reads representing five unique Superpowers bodies. Unresolved: zero.

## Exact internal bodies

| Body | SHA-256 |
|---|---|
| `slice-op-prep-entry-gate.step.md` | `a465a836ac86842fe45c1b95cb4638ba539bd9f56d0fa8f41b65518146861e02` |
| `slice-op-prep-intent-capture.step.md` | `04b44c223691e9a91ad2f8c3b87f2a80aaa21640c49c8cc29c1aac70cf733a22` |
| `slice-op-prep-context-loader.step.md` | `b4ba0b3d9c7a6b3770a497d99a43d2996cefc7fadf3767b2064b598530eb8698` |
| `slice-op-prep-contract-writer.step.md` | `094bb7301aadc2fdd8a29c16912af601d179c60e8b696efc632ff28ba89df4b3` |
| `slice-op-authority-boundary-declarer.step.md` | `b0f7844f56a3602df5b48aac1db2190a390a1e209d675c52b805778dc86ae156` |
| `slice-op-target-state-baseliner.step.md` | `5e308ef511ee7d199bda0e08417f57c8097aa2f477ba3ccc3b96434e451d044c` |
| `slice-op-risk-and-impact-modeler.step.md` | `22e3aa01ef4300ced6e724f998ae2d0df3445cfc2eca8bb1e2dbc3477db4c290` |
| `slice-op-preflight-check-builder.step.md` | `98daf7720a907c7a0a08890f17972233b1fce704779a82be2e6c74c3a1601b24` |
| `slice-op-command-plan-builder.step.md` | `433fb6215d534b0cc84b8a25e08f6a62da65e57efb4fe0b8abb13842bd7d6bbc` |
| `slice-op-rollback-plan-builder.step.md` | `b989b865ad7df81ce3d70ab3205ff92f459754d73fa95396d7a5770b98911248` |
| `slice-op-proof-contract-builder.step.md` | `06d1a49ee2ac6188a4f108ba6c44f52916ae795b58234de4300a20bbb0a2af56` |
| `slice-op-dry-run-or-readonly-validator.step.md` | `ac97c21cc32e524828f8e86f6a53f730174797b12f92da31c0f28e9ae314dd3c` |
| `slice-op-user-handoff-package-builder.step.md` | `73c8f3fbd1164e25de87d6fa7d0d1f978312c20804ae8b9076e0fc7a5c907069` |
| `slice-op-prep-result-writer.step.md` | `ac28ce635ac06b04327f38331648d7b6221bcd95e3419733b1c93c52190d002c` |
| `slice-op-prep-promotion-router.step.md` | `e1a3f6adbf68bfd8205d432e964b43eefe39f909cca8d197d17e1eabf3d3a172` |
| `slice-op-prep-maintenance-check-requester.step.md` | `fee0f96fc26e5e5412b08e02b3a798e200a614c5b2d5983b598b593975873464` |

Each file under `v1/` is byte-exact. Native `output_contract` overlays keep the source analysis and artifact obligations while making retired V1 control-plane commands non-actionable.

## Skill closure and placement

Exact Superpowers `5.0.7` bodies are pinned to `obra/superpowers@e7a2d16476bf042e9add4699c9d018a90f86e4a6`: `writing-plans` `90056bad3d5f196fa7c9fec0ffe592e6d9c86bc983e406642a51d1a4198b7024` at phases 4 and 9; `using-git-worktrees` `dcd1a83a2488bd557ceb7f14f2b6384ec209f551d18752dd9ceb70b9089dfb3b` at phases 6 and 9; `verification-before-completion` `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c` at phase 11; `executing-plans` `a711f83fb762e2ea0fa151f598893da9911a408895c91cc7a7e0770dd59a27b3` and its mandatory `finishing-a-development-branch` closure `dd2f82c6dc8582b621f9eb57fcb65f557f88eadf872727ac81d0840ae12c504e` at phase 9.

V1 `reference_skill_map.required_skill_refs` says `executing-plans` is required but supplies no phase in `step_skill_refs` and no step invocation. The accepted delivery plan also requires it. Native phase 9 is the only semantically valid binding: it uses plan-review/checkpoint/stop discipline while the phase adapter makes every action/commit/branch command inert. This is an explicit location-mismatch resolution, not an invented execution phase.

## Preservation and truth boundary

Every phase requires `operation_executed=false` and `mutating_command_executed=false`. Exact commands in `operation-plan.md` are plan text. Phase 12 may run only an already-authorized read-only inspection or documented dry-run; `dry-run.md` is required even for not-required or blocked dispositions so omission cannot masquerade as progress. No dry run, plan, access credential, approval note, or handoff proves the target changed.

Authority fields separate read, prepare, execute, deploy, write/delete, rollback, and cleanup. Current-state evidence binds exact target, source, timestamp, freshness/stale-after, drift, dirty ownership, and isolation. Risk and preflight cover blast radius, reversibility, users/data/security, dependencies, expected results, stop conditions, blockers, and owners. Rollback and proof contracts include action-time authority and fresh positive and negative checks. The user/team handoff carries prerequisites, exact commands, checkpoints, timeouts, stop/rollback rules, proof ladder, resume trigger, and return-proof contract.

Phases 12, 15, and 16 are optional in V1 but progress-mandatory natively: each reports read-only/not-required/blocked; promotion/no-promotion/procedure/deferred/authority; and prepared/handoff/preflight-blocked dispositions respectively. Blocked, waiting, and rework routes never receive success markers. Rework targets name exact prior phases.

Execution authority creates a separate Operational Execution Slice. Implementation creates a Lightweight or Full Slice; unknown cause creates Debug. Phase 1 splits those single-target routes from the V1 Hybrid replacement. Only `escalate_to_implementation_operational_graph` requires a proposal containing future implementation and operational nodes plus a truthful ordered or unresolved dependency. Operational Preparation cannot satisfy the implementation node; the graph preserves target, evidence, authority, owner, dynamic order, rollback, and proof without selecting retired Hybrid, inventing a primary successor, or activating a successor. Backend run/attempt/revision state replaces duplicate V1 lifecycle carriers only. The agent remains responsible for factual current-state/read-only command evidence and authority truth.
