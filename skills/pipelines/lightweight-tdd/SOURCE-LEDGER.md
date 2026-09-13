# Lightweight TDD pipeline source ledger

## Native package identity

- Native pipeline kind: `slice.lightweight-tdd-development`
- Native definition version: `0.1.0-native.1`
- Delivery modes: `whole`, `phasewise`; default `whole`
- V1 Tect source revision: `ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8`
- V1 manifest: `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json`
- V1 manifest SHA256: `c321f36b637abf1be4d2169eacbc8a4a06af92fc6507dd984e37611eafdfb5ab`
- V1 ordered fourteen-body set SHA256: `227c2f3e222db39400459687e4124b4a55321174f019af31386dcd72bf1d048e`
- Native definition semantic digest: `5a5152233000b741f6364666a539917395a72cfa8e592271a96216ec2e69c40a`
- Native definition physical-file SHA256: `e653592e8ab7ccf460ee3b9abf16b245a874148529d2054c8770cb0eb7a93307`

All files in `v1/` are byte-for-byte snapshots from that V1 revision. Their bodies remain immutable provenance. The native definition delivers those exact bodies together with a phase-specific `output_contract`. The native contract has precedence only for the carrier and runtime boundary: backend fields and output bindings replace V1 control-plane Markdown/FSM persistence, retired Tect route/tool calls, and package self-validator commands. Substantive scope, evidence, authority, TDD, verification, escalation, result, promotion, and handoff obligations remain mandatory. Product source and test edits are allowed only in the TDD phase and only within the run's recorded authority and preflight boundary.

## V1 instruction snapshots

| Phase | Vendored file | SHA256 |
|---:|---|---|
| 1 | `v1/slice-lightweight-entry-gate.step.md` | `c1485a06030c2efbec9aaedb51f6f98c9e42036cd947967ee886b92745f79ba7` |
| 2 | `v1/slice-lightweight-intent-capture.step.md` | `afff0ce793e43752c6dbc692b928eb2bcb15db80453f401ba8e8dda01d49ac4d` |
| 3 | `v1/slice-lightweight-context-loader.step.md` | `c88a014734d7cd246bdfbb3bce6907356ba0b73bc814f6f6e75946407f4c620a` |
| 4 | `v1/slice-workspace-preflight-lite.step.md` | `6a84c91c0e91e783ead0e563b11366284600ebe5e3de80c473a93f4011c1f355` |
| 5 | `v1/slice-lightweight-contract-writer.step.md` | `dd166e0d5f59b6a9a92c0150794ccc78d590180c63564c94f5d9de491e6ca019` |
| 6 | `v1/slice-lightweight-escalation-checker.step.md` | `c1c54be6601b80f7e9d50e0a0375d5d0b7ab8299c2e72aba0248e7d26f8e37a8` |
| 7 | `v1/slice-test-target-selector.step.md` | `7de99e39e41ee720cc553287e90232739af42afe43abb577a616da00d4ab45a0` |
| 8 | `v1/slice-tdd-cycle-runner.step.md` | `552f9485c6a2d25b60f33832ddca56a2bf5f911bb24db73087867de51380c247` |
| 9 | `v1/slice-implementation-note-writer.step.md` | `e9b5dbc849fc60465383a26da6c874104f55dfb3b84d7ad17a1ba0ee1ca8f7af` |
| 10 | `v1/slice-lightweight-verification-runner.step.md` | `9aecd4247dc1b8e0b670b40c02dc983310db33279061b6e137592799dc77fe7f` |
| 11 | `v1/slice-deploy-impact-checker.step.md` | `92903b541aae4e4aadc60236454c6c1671270dd2a9615d1142269b8d15807381` |
| 12 | `v1/slice-lightweight-result-writer.step.md` | `fe5d6d2be593e390e0dad42d8e1966e25022412beac9b5ba7a4c0c3f34b8604a` |
| 13 | `v1/slice-lightweight-promotion-router.step.md` | `999ed8b2bff656691d2b39345edb08293aff1d5647090492861a2e550af346da` |
| 14 | `v1/slice-lightweight-maintenance-and-handoff.step.md` | `2057ba176e7c9d22f1a34d134881d40a88f56f145966423083a3c4b35b8cc1e6` |

## External skill snapshots

The V1 external-skill registry recorded primary paths under `skills/references/superpowers/`, but those paths are no longer present. The original public upstream declared by the installed plugin is `https://github.com/obra/superpowers`. Revision `e7a2d16476bf042e9add4699c9d018a90f86e4a6` (`superpowers` plugin version `5.0.7`) is the latest common upstream revision whose four blobs exactly match every V1-selected digest. Later locally available 6.3.0 bodies have different hashes and are not used.

| V1 reference | Vendored file | V1 selected / upstream SHA256 | Reachability |
|---|---|---|---|
| `superpowers:using-git-worktrees` | `../../references/superpowers/using-git-worktrees/SKILL.md` | `dcd1a83a2488bd557ceb7f14f2b6384ec209f551d18752dd9ceb70b9089dfb3b` | Phase 4 direct invocation |
| `superpowers:test-driven-development` | `../../references/superpowers/test-driven-development/SKILL.md` | `7dee67b4af6bdccc7a914ca34533184d64592d0f5b23aeae631538168db14994` | Phase 8 direct invocation |
| mandatory TDD include `@testing-anti-patterns.md` | `../../references/superpowers/test-driven-development/testing-anti-patterns.md` | `bde453bc258f06543987477c837939afaa774ea2acbd9f308d702fc452bc4283` | Required 8 nested mandatory reference; its obligations are included in the phase output contract |
| `superpowers:verification-before-completion` | `../../references/superpowers/verification-before-completion/SKILL.md` | `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c` | Phase 10 direct invocation |

## Native projection rules

1. Each phase returns one native output body, typed fields, one allowed verdict, and a reference when an external artifact or source change exists. The backend owns durable phase state, ordering, revision checks, idempotency, current-phase selection, and output bindings.
2. V1 artifact names remain carrier obligations. They appear in phase fields and references so a cold resume can reconstruct the source contract, but the agent does not create duplicate lifecycle Markdown solely to advance the pipeline.
3. Optional graph phases 11, 13, and 14 remain progress-mandatory. Each must return an explicit disposition such as no deployment needed, no promotion, or no handoff/maintenance needed; omission never satisfies progress.
4. V1 Hybrid escalation is projected to a typed follow-up proposal with separate implementation and operational Slice nodes. Phase 6 cannot satisfy a node with the current run. Phase 11 may bind the current Lightweight implementation only to exact consumed TDD and verification outputs; phase 14 may bind it only to exact consumed verification and Result outputs. New-scope variants keep all nodes as future candidates. Every graph preserves trigger, target, authority gap, proof need, and a truthful ordered or unresolved dependency. It cannot select retired Hybrid, invent a primary successor, force a universal implementation-to-operations order, or activate a successor.
5. Every verdict is bound to an explicit outcome, transition, and exact disposition set. Successful continuation/completion routes carry the applicable V1 gate or accepted progress marker. Waiting-input, blocked, and escalation routes carry their honest V1 exit or accepted branch marker and never satisfy a success gate. Backward transitions are limited to the explicit phase ids in the native definition. Every Lightweight phase is repeatable; exact request-id replay remains a separate backend guarantee.
6. Phase 8 alone may mutate the bounded product source/test surface. Phase 8 still requires observable RED, minimal GREEN, same-target binding, and the mandatory testing anti-pattern check. Phase 10 requires fresh focused and affected verification evidence before any local-verification claim.
7. Package self-validation commands in V1 bodies are provenance for the old package. They are not instructions to invoke retired validators during a native run.

## Definition digest rule

The top-level definition digest is lowercase SHA256 of the `serde_json` serialization of `PipelineDefinitionSnapshot` after setting only its top-level `digest` field to the empty string. Instruction and skill digests are lowercase SHA256 of their exact body bytes.
