# Lightweight TDD pipeline source ledger

## Native package identity

- Native pipeline kind: `slice.lightweight-tdd-development`
- Current compact native definition: `crates/host/pipeline-definitions/lightweight-tdd-0.7.1-native.k1k5.json`, version `0.7.1-native.k1k5`
- Delivery modes: `whole`, `phasewise`; current compact default `phasewise`
- V1 Tect source revision: `ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8`
- V1 manifest: `capabilities/pipelines/slice-variants/lightweight-tdd.pipeline.json`
- V1 manifest SHA256: `c321f36b637abf1be4d2169eacbc8a4a06af92fc6507dd984e37611eafdfb5ab`
- V1 ordered fourteen-body set SHA256: `227c2f3e222db39400459687e4124b4a55321174f019af31386dcd72bf1d048e`
- Previous compact snapshot: `crates/host/pipeline-definitions/lightweight-tdd-0.7.0-native.k1k5.json`, version `0.7.0-native.k1k5`, semantic digest `7f5dd6a4503078538d45d0c90c83fdcd896ff1216167556ff9bd0424f826aab0`, physical-file SHA256 `251f68a4bede1857e7a35776081ead3084401765199d997b2d8ca3d291ee9306`
- Current compact semantic digest: `93df97f4cb4458a18411b76005b29025a56234dc47650e4147ac5fdab3d30d89`
- Current compact physical-file SHA256: `f3c09714c8040d08d5aff64026cf9dceb5ad1e343033641f27f5629cc0e4e2c5`

The compact definition intentionally changes the default from V1 `whole` to `phasewise`: backend gates, resumability, revision invalidation, and state/receipt bindings remain visible at each K checkpoint. `whole` remains allowed and obeys the same ordered K1–K5 semantics. This is a declared native runtime adaptation, not a claim that V1 used phasewise by default.

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

## Superpowers v6.3.0 native adaptation

- Update class: selected upstream bodies only; native phase order and output contracts are unchanged.
- Previous active snapshot: `crates/host/pipeline-definitions/lightweight-tdd-0.1.0-native.1.json`, version `0.1.0-native.1`, semantic digest `5a5152233000b741f6364666a539917395a72cfa8e592271a96216ec2e69c40a`, physical SHA-256 `a844609332cfd35943cf77599b1140eba5a4102a8e15afc57136dd33c350bb1c`.
- Current snapshot: `crates/host/pipeline-definitions/lightweight-tdd.json`, version `0.4.0-native.skills.1`, semantic digest `b80b3472ebf4acc38996fa1946a2fe76e1b17fbcc39c6594f87a00e63a437768`, physical SHA-256 `66983d90c2fc8f17f91cec02a29dcd3bc382c2967f683a7392dc1378561fb921`.
- Exact selected source package: `skills/references/superpowers-v6.3.0-b36e0829`, upstream commit `b36e0829c6d0140e93cfef2ca599b1b07d4a7797`, tag `v6.3.0`, package manifest SHA-256 `2973ac33ed683d9c50e15fab14a5045f768b6b765d29849237f95371d99b5dde`, MIT license SHA-256 `a37e0e9697144819e1d965176ac4ae5bc3fa02d11e7812036bbcadf6dafe2400`.

### Selected body replacements

| Previous ID | Previous version | Previous digest | Current ID | Current version | Current digest | Current source |
|---|---:|---|---|---:|---|---|
| `superpowers:test-driven-development` | `5.0.7` | `7dee67b4af6bdccc7a914ca34533184d64592d0f5b23aeae631538168db14994` | `superpowers:test-driven-development` | `6.3.0` | `bf1b8216e523851a411e91d429a7c1c2a173e79d88957bc78e348218d50edd54` | `skills/references/superpowers-v6.3.0-b36e0829/skills/test-driven-development/SKILL.md` |
| `superpowers:test-driven-development/testing-anti-patterns` | `5.0.7` | `bde453bc258f06543987477c837939afaa774ea2acbd9f308d702fc452bc4283` | `superpowers:test-driven-development/writing-good-tests` | `6.3.0` | `51471c853306ff92ca8bb41dcaea05f31c0e46b03651f8f3c99754b7172f4ae1` | `skills/references/superpowers-v6.3.0-b36e0829/skills/test-driven-development/writing-good-tests.md` |
| `superpowers:using-git-worktrees` | `5.0.7` | `dcd1a83a2488bd557ceb7f14f2b6384ec209f551d18752dd9ceb70b9089dfb3b` | `superpowers:using-git-worktrees` | `6.3.0` | `8cfb86f121269e8f7f12361e6795c4f6738828340e28964c9229d365666c9edd` | `skills/references/superpowers-v6.3.0-b36e0829/skills/using-git-worktrees/SKILL.md` |

### Native adapters

| Phase | Resource ID | Version | Digest | Source |
|---|---|---:|---|---|
| `slice-workspace-preflight-lite` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-workspace-preflight-lite` | `tect:superpowers-v6-workspace-and-finish` | `0.4.0-native.skills.1` | `1be702b53fa3c813bb67d09780fd117d2d1b4d12c416b9ffeb4f8248da7dae21` | `skills/pipelines/shared/superpowers-v6-workspace-and-finish.md` |
| `slice-tdd-cycle-runner` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-tdd-cycle-runner` | `tect:superpowers-v6-test-quality` | `0.4.0-native.skills.1` | `892f63a5253c9d6517f9345bc1c102428d0abe50389e6347bff917556a7bbe39` | `skills/pipelines/shared/superpowers-v6-test-quality.md` |

### Deliberately retained Superpowers bodies

| ID | Version | Digest | Preserved source |
|---|---:|---|---|
| `superpowers:verification-before-completion` | `5.0.7` | `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c` | `skills/references/superpowers/verification-before-completion/SKILL.md` |

The archived snapshot remains the exact carrier for stored runs created against the previous definition version.

## Compact K1–K5 anchors and fourteen-step disposition

These anchors are the stable origin references used by v0.7. “Backend-replaced” means the substantive obligation is retained while ordered state, bindings, revisions, receipts, or the carrier is owned by the native backend. The compact definition does not recreate the fourteen runtime steps or eight Markdown lifecycle artifacts.

## K1

Read-only entry and intent gate. It owns V1 steps 1 (`entry-gate`) and 2 (`intent-capture`) plus the early escalation decision from step 6. It requires current parent, preflight, authority, bounded fit, request, and observable acceptance. Significant unknowns or conflicts block, rework, or escalate; K1 performs no mutation or action.

## K2

Read-only context, preflight, contract, and test-target preparation. It owns V1 steps 3 (`context-loader`), 4 (`workspace-preflight-lite`), 5 (`contract-writer`), and 7 (`test-target-selector`). It records source/worktree provenance, isolation, ownership, overlap, target/proof plan, and the test target. Missing or unsafe facts use explicit defer/escalate routing; K2 does not install, mutate, implement, deploy, or call live systems.

## K3

**NEW/STRENGTHENED; not a V1 origin.** K3 derives from the v0.6 `slice-lightweight-pre-implementation-review` and the pre-implementation engineering-review decision recorded before v0.7 implementation. It reviews the current K1/K2 plan with reviewer-context identity and backend revision binding. Before a positive verdict it applies the mandatory consumer-path counterfactual: evidence must exercise the accepted path or directly assess an explicitly requested artifact, and proof that could pass while a directly unit-tested contribution remains disconnected cannot complete the Result. A supported API or public library contract counts only when that contract is the requested deliverable. Rework returns to K2 and makes downstream evidence stale. K3 must never be described as one of the fourteen V1 steps.

## K4

Authorized bounded TDD mutation and implementation notes. It owns V1 steps 8 (`tdd-cycle-runner`) and 9 (`implementation-note-writer`). Pass requires a meaningful fresh RED receipt, minimal GREEN on the same target, mandatory testing-anti-pattern review, authorized scope, recorded changes/deviations, and no missing proof. It stops before final verification, Result, deployment, promotion, or maintenance.

## K5

Authorized local verification, deploy-impact classification, exact local Result, no-promotion/defer routing, and handoff checkpoint. It owns V1 steps 10 (`verification-runner`), 11 (`deploy-impact-checker`), 12 (`result-writer`), 13 (`promotion-router`), and 14 (`maintenance-and-handoff`). Pass requires fresh focused and affected proof; one receipt may cover both only when it declares both scopes. Stale, failed, skipped, missing, or wrong-scope proof blocks. K5 never edits source, deploys, validates live systems, writes canonical knowledge, promotes, or performs cleanup/maintenance. Active handoff or deferred owner action is nonterminal.

| V1 step | Current owner | Primary disposition |
|---:|---|---|
| 1 Entry gate | K1 | carry/enforce |
| 2 Intent capture | K1 | carry/enforce |
| 3 Context loader | K2 | carry/enforce; carrier backend-replaced |
| 4 Workspace preflight | K2 | carry/enforce; worktree creation removed from this read-only phase |
| 5 Contract writer | K2 | carry/enforce; compact fields replace `slice.md` |
| 6 Escalation checker | K1/K2/K5 at trigger point | escalation-owned |
| 7 Test-target selector | K2 | carry/enforce |
| 8 TDD cycle | K4 | carry/enforce with production receipt validation |
| 9 Implementation note | K4 | carry/enforce; compact fields replace `implementation-notes.md` |
| 10 Verification | K5 | carry/enforce with production receipt validation |
| 11 Deploy impact | K5 | carry/enforce; execution remains external |
| 12 Result writer | K5 | carry/enforce; native terminal Result replaces `result.md` |
| 13 Promotion router | K5 | escalation-owned; promotion execution remains external |
| 14 Maintenance/handoff | K5 | escalation-owned; cleanup execution remains external |

## Required reference and adapter map

The v0.7 runtime rejects agent-supplied skill/resource read receipts and derives dependency evidence from backend state, so its `skills` and `resources` arrays intentionally stay empty. The following traceable adapters preserve the required methods without adding broken runtime dependencies:

| Required reference | Trigger/phase | Native adaptation or owner |
|---|---|---|
| `superpowers:using-git-worktrees` | K2 when repo/worktree identity or isolation matters | Read-only provenance, ignore-rule, isolation, ownership, and overlap assessment; no worktree creation/cleanup. |
| `superpowers:test-driven-development` | K4 | Meaningful test-first RED, minimal GREEN, same target, refactor only after GREEN. |
| nested `testing-anti-patterns.md` | K4, mandatory with TDD | Explicit anti-pattern review; mock-only, unexplained over-mocking, incomplete mocks, or test-after-code blocks pass. |
| `superpowers:verification-before-completion` | K5 before local success | Fresh focused and affected receipt validation precedes the local Result claim. |
| `superpowers:writing-plans` | K2 | Compressed to the bounded target/proof plan; no second plan lifecycle. |
| `superpowers:executing-plans` | K4 | Execute only the current reviewed bounded target; no batch/hidden lifecycle. |
| `superpowers:systematic-debugging` | K1/K2/K4 on unknown cause or repeated failure | Escalate to `slice.debug-root-cause`; Lightweight does not perform the debugging lifecycle. |
| `superpowers:writing-skills` | K5 when reusable skill work is discovered | External skill-authoring owner; record handoff/defer, do not author or publish here. |
| `superpowers:finishing-a-development-branch` | K5 when cleanup/branch finalization remains | External cleanup/maintenance owner; record handoff/defer, do not finish or clean the branch here. |
