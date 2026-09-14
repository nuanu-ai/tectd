# Full Design-to-Execution source ledger

## Identity

- Native kind/version: `slice.full-design-to-execution@0.1.0-native.1`.
- V1 source revision: `ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8`.
- V1 manifest: `capabilities/pipelines/slice-variants/full-design-to-execution.pipeline.json`, SHA-256 `20f0f6022bf0aae10527b8d60449207648a9dc009170c7b886418210a8aaccfd`.
- Ordered reachable V1 body-set digest: `275ffeb9ecef9538d3132ead86328cad836c55fa1a2f3f6331c99051a0f41c84`.
- Native definition: `crates/host/pipeline-definitions/full-design-to-execution.json`, semantic digest `5e4fd372ee9c07c720732f444056a32a512171c202c2d2c53af74f81568a98c7`, physical SHA-256 `5ad905f9153b780b2f6303fa3214b1a23f72471902bf0f1acc71397076891919`.
- Accepted V2 delivery setting: default `phasewise`, allowed modes `phasewise` only. V1 did not define this transport setting.

## Exact V1 instruction snapshots

The files under `v1/` are immutable UTF-8 provenance. Their active native adapter is the corresponding phase `output_contract`; the source bodies themselves are byte-exact and are never edited to hide a retired command.

| Phase | Vendored file | SHA-256 |
|---:|---|---|
| 1 | `v1/tect-full-dev-entry-gate.step.md` | `b617a99b224317bd09c24ce7e4fb4804bbefd61984c495c787cc28f3cab2d158` |
| 2 | `v1/slice-workspace-preflight.manifest-step.json` | `06938425e6906408cea9d5832bfb67388d6a9d640b0c6d044d624ba4541fcdc4` |
| 3 | `v1/tect-design-spec-shaper.step.md` | `baff45029b56483c50405d1731ecd7873788ce405df4881a23c87cb9dc1b0def` |
| 4 | `v1/tect-slice-contract-writer.step.md` | `bddc82a2ce30dd808f8cc8238e10c09fe1cf293686510e53528e01a04931df96` |
| 9 | `v1/tect-spec-readiness-checker.step.md` | `71c49a20e96a324bc668979eecd3b3f62e5c0f1c05479018295198e83222c1d9` |
| 11 | `v1/tect-human-decision-queue-manager.step.md` | `c3a6247269e9d8f4e2f662e835532bc62eeeb8b098f746a7b80be7493329e13f` |
| 14 | `v1/tect-validation-deployment-contract-shaper.step.md` | `bbe00e4c6d9e3ead17d13ee8a97e79b42a95c913b9e49fcc249090037063644e` |
| 15 | `v1/tect-deployment-or-handoff-gate.step.md` | `cc278ea932261cdab47a5c5507b7e6aea14318a504c84918860860fffb008f04` |
| 16 | `v1/tect-live-validation-runner.step.md` | `03f3b9b0679ff5d9c6916a099fd7f0fb66b4b14c3fdb34f77e424ea02b8766fb` |
| 17 | `v1/tect-result-writer.step.md` | `213c13e48aca28d703644f1aa076f1e429ec41034a57e4073d8b770040027159` |
| 18 | `v1/tect-promotion-and-deferred-router.step.md` | `f6f3d974403f7aedb1de119e77a5d1b41db9b3a9f85db50e7b6e416012047b41` |
| 19 | `v1/tect-maintenance-check-requester.step.md` | `3774afc7d6a7dfff652154e5aa0d4a0326f8469ae86d925ab2f3f6838282ed12` |
| 20 | `v1/tect-handoff-builder.step.md` | `b44582b0d9c7c7fe734ecbf2591831a1c1ec1ca04b0e9f909ae398588b0828d0` |

Phase 2 additionally pins exact service resolution provenance at the same V1 revision: `capabilities/registry/route-materialization-map.json#route-materialization-map:service:workspace-map`, `capabilities/services/workspace-map/service.json`, `capabilities/registry/manifest-invocation-service-aliases.json#git-worktree-read-only-service`, and `capabilities/services/git-worktree/service.json`. Native phase 2 requires concrete read-only commands, exit codes, observations, and blockers. The backend validates receipt shape and types; repository facts remain caller-observed and are not converted from a V1 descriptor into assertions.

Nine same-directory `slice-*.step.md` files conflict with selected manifest/registry invocations and are inactive provenance: `slice-component-decision-interrogator`, `slice-contract-writer`, `slice-cross-cutting-reviewer`, `slice-design-spec-shaper`, `slice-full-dev-entry-gate`, `slice-implementation-spec-synthesizer`, `slice-plan-builder`, `slice-reconciliation-runner`, and `slice-validation-deployment-contract-shaper`. Bounded manifest, registry, and nested-reference reachability found no active edge to them.

## Exact external skills and nested resources

The four spec skills have no source repository revision, so they are pinned to the exact current installed bytes:

| Skill | Phase | SHA-256 |
|---|---:|---|
| `spec-interrogation` | 5 | `93115b5d2d3b3219ffd216fa8f75ade380016dc532f0aaabe5f17078d38e3fe7` |
| `spec-cross-cutting-review` | 6 | `a7669954c672a6a1e486e03d926b62cfa8b97584f6633ee83185b6c1a9d3ddc3` |
| `spec-reconciliation` | 7 | `2a1d430b8fe4e356290b9842b1730dcc5f2743039a232443fff1e90e251e1065` |
| `spec-synthesis` | 8 | `5787378c8fc046f876366afa08550a1b7452897f7e30325a9ba41da45ba90f3e` |

Superpowers snapshots are exact plugin `5.0.7` bodies from `obra/superpowers@e7a2d16476bf042e9add4699c9d018a90f86e4a6`: `brainstorming` `bba47904a7f6bbee3bf8a107ebbe84e65d392be683bbb898ded736b29e415f90`, `writing-plans` `90056bad3d5f196fa7c9fec0ffe592e6d9c86bc983e406642a51d1a4198b7024`, `subagent-driven-development` `081ad3869e55c80bf8f890b4768a90c0e8057daf94b1b6fadebfc85ea5b8304a`, `executing-plans` `a711f83fb762e2ea0fa151f598893da9911a408895c91cc7a7e0770dd59a27b3`, `using-git-worktrees` `dcd1a83a2488bd557ceb7f14f2b6384ec209f551d18752dd9ceb70b9089dfb3b`, `requesting-code-review` `a5ff68586ccf62d1803cedeb71d60fd96ec05591d29c8d123196117eefd34cd0`, `finishing-a-development-branch` `dd2f82c6dc8582b621f9eb57fcb65f557f88eadf872727ac81d0840ae12c504e`, `test-driven-development` `7dee67b4af6bdccc7a914ca34533184d64592d0f5b23aeae631538168db14994`, and `verification-before-completion` `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c`. Exact mandatory or conditional companion resources are `brainstorming/visual-companion.md` `0d3a8e7d63c665361f6a40b8dc48a259dceb98edb0c79a5931175f910373d6c1`; subagent prompts `a416193f881e5a712c988fffabfe1d5a97bffcd091eb95577c84fe2136588617`, `631980e472eec5394de8b89b69d432e54fd3f7f523ecf9afa7eb4cda0c9b2baf`, `06d1e7c2287e5a00bd1809bf39038abd5f413b4cf917c37d30f00c48f6293421`; `requesting-code-review/code-reviewer.md` `7f5328dca12cb200005ae9d4386f63a9b0acb735ece57f82db206b4a3189ccae`; and `test-driven-development/testing-anti-patterns.md` `bde453bc258f06543987477c837939afaa774ea2acbd9f308d702fc452bc4283`.

The definition contains 15 phase-local skill snapshots representing 13 unique skill bodies. Phase 12 explicitly expands the nested skill closure instead of treating V1 catalogue metadata as a read: subagent execution, batch execution, worktree, planning, review, branch finishing, TDD, and all five prompt/template/anti-pattern resources. Phase 3 binds the visual companion as conditional policy input; no visual is required unless the source trigger and user choice apply.

## Specification artifacts and validators

Shared resources are exact current bytes: `skills/references/spec-pipeline-shared/CONTRACT.md` SHA-256 `d9f4c2a7e1fd8daffbb28f6662ae91fe973090d6779ed1f76e134b9785dd0096` and `validate-spec-pipeline.js` SHA-256 `35d5cfb5c746706df9d93e69ada502f43beb8f6aea3147821bac29b45f0401e6`.

| Phase | Required artifact set | Machine validator |
|---:|---|---|
| 5 | `decisions/README.md`, at least two `decisions/*.md` matches in total (the README plus at least one component decision file), `requirements-ledger.json`, `decision-traceability.json`, `acceptance-obligations.json` | None. Interrogation requires artifact construction and semantic checks, not this shared runnable validator. |
| 6 | `cross-cutting-review.md` from an independent reviewer context and exact producer inputs | None. Cross review is a read-only adversarial review, not a shared-validator stage. |
| 7 | Updated first three JSON sidecars, `reconciliation-closure.json`, and amended `cross-cutting-review.md` | `node skills/references/spec-pipeline-shared/validate-spec-pipeline.js <artifact-directory> --stage reconciliation`; required on `reconciled`, `not_required`, and `blocked_unreconciled_findings`; success verdicts are only `reconciled` and `not_required`; receipt binds the exact four JSON artifact names and digests plus validator id/version/digest. |
| 8 | On `implementation_ready`: `implementation-ready-spec.md`, exact carried first four JSON sidecars, and `synthesis-traceability.json` | `node skills/references/spec-pipeline-shared/validate-spec-pipeline.js <artifact-directory> --stage synthesis`; success receipt binds the exact five JSON artifact names and digests plus validator id/version/digest. A source-defined early `blocked_not_ready` does not invent a validator run. |

The backend structurally verifies declared artifact names/patterns, minimum matches, media types, body SHA-256, JSON parsing, exact schema resource binding for JSON, exact skill/resource reads, validator resource identity, receipt-to-artifact name/digest equality, success exit `0` and `valid: true`, typed phase fields, route dispositions, consumed revisions, and revisit targets. The phase caller remains responsible for the semantic truth of Markdown and JSON contents, actual validator command execution, reported stdout/output digest, and read-only source/authority/proof observations.

Minimal positive validator proof uses one covered in-scope `REQ-001`, one decision trace referencing one positive and one negative obligation, a ready empty-unresolved closure, and a modality-preserving synthesis row. The vendored validator returns `valid:true` at both reconciliation and synthesis stages. Changing the ledger modality to `INVALID` returns exit `1` with `REQUIREMENT_MODALITY_INVALID` and `SYNTHESIS_MODALITY_MISMATCH`; this separately proves the vendored validator rejects corrupted source-contract material rather than merely satisfying backend receipt shape.

## Native projection and loops

The definition preserves all 20 ordered V1 phases, 73 typed verdict routes, 33 artifact requirements, 12 rework routes, and the sole independent-review phase at ordinal 6. Required order is Shaper (3) -> Contract (4) -> Interrogation (5) -> Cross Review (6) -> Reconciliation (7) -> Synthesis (8) -> Readiness (9) -> Plan (10). Rework targets are exact route-owned predecessors; a backward disposition cannot accidentally advance. Phase 12 uses `reconciliation_required` retry after partial source mutation; other phases are repeatable under native request-id replay and revision rules.

Native output body, fields, typed artifacts, references, and route receipts replace V1 Markdown progress-carrier and control-plane bookkeeping only. Phase 7 maps legacy `reconciliation.md` progress to output body while still requiring synchronized sidecars, closure, amended review, validator proof, and gate. Backend Slice/admission state replaces duplicate `slice.md`/FSM persistence. It does not replace semantic design, decision, spec, plan, execution, proof, Result, deployment, deferred, maintenance, or handoff content.

Blocked and waiting routes never receive success dispositions. Existing authorization and no-question paths continue autonomously; unresolved material human decisions wait or block. Deployment, live validation, promotion, cleanup, merge, and branch operations require their own current authority. At phase 15, ordinary handoff remains distinct from the V1 Hybrid replacement. The current-run graph may satisfy exactly one Full node only when bound to consumed execution-runner and verification-runner outputs; a newly discovered implementation scope keeps all nodes future candidates. Both graph variants require implementation and Operational Preparation/Execution kind coverage plus a truthful ordered or unresolved dependency, preserve trigger/evidence/owner/authority/proof conditions, and neither invents nor activates a primary successor.

Phase 19 preserves the union of the selected body and manifest maintenance contracts. The selected body names front-door synchronization while the manifest receipt names repair-proposal routing; native fields require both, plus variant shape, Result presence, promotion readiness, stale projection, and validation/deployment proof shape. Maintenance remains request/routing only and cannot claim executed repair.

## Superpowers v6.3.0 native adaptation

- Update class: selected upstream bodies only; native phase order and output contracts are unchanged.
- Previous active snapshot: `crates/host/pipeline-definitions/full-design-to-execution-0.1.0-native.1.json`, version `0.1.0-native.1`, semantic digest `5e4fd372ee9c07c720732f444056a32a512171c202c2d2c53af74f81568a98c7`, physical SHA-256 `5ad905f9153b780b2f6303fa3214b1a23f72471902bf0f1acc71397076891919`.
- Current snapshot: `crates/host/pipeline-definitions/full-design-to-execution.json`, version `0.4.0-native.skills.1`, semantic digest `13fd152337abc76d7bbfa15c0875d7b6fbe4719ccfd6fadab5f31825cd39769b`, physical SHA-256 `eb36e20697b38204a5a10261f5454e854538b6c719213f9d9b6be0303663bab1`.
- Exact selected source package: `skills/references/superpowers-v6.3.0-b36e0829`, upstream commit `b36e0829c6d0140e93cfef2ca599b1b07d4a7797`, tag `v6.3.0`, package manifest SHA-256 `2973ac33ed683d9c50e15fab14a5045f768b6b765d29849237f95371d99b5dde`, MIT license SHA-256 `a37e0e9697144819e1d965176ac4ae5bc3fa02d11e7812036bbcadf6dafe2400`.

### Selected body replacements

| Previous ID | Previous version | Previous digest | Current ID | Current version | Current digest | Current source |
|---|---:|---|---|---:|---|---|
| `superpowers:executing-plans` | `5.0.7` | `a711f83fb762e2ea0fa151f598893da9911a408895c91cc7a7e0770dd59a27b3` | `superpowers:executing-plans` | `6.3.0` | `c4c3d8b628c51114cd165fb8246fe02744cd8be180032328391252e653028d9b` | `skills/references/superpowers-v6.3.0-b36e0829/skills/executing-plans/SKILL.md` |
| `superpowers:finishing-a-development-branch` | `5.0.7` | `dd2f82c6dc8582b621f9eb57fcb65f557f88eadf872727ac81d0840ae12c504e` | `superpowers:finishing-a-development-branch` | `6.3.0` | `8db5a922b242dd4e1bf824cb91c13b3e8d8e8a86d6ceaf7f0774eb9cce909d65` | `skills/references/superpowers-v6.3.0-b36e0829/skills/finishing-a-development-branch/SKILL.md` |
| `superpowers:requesting-code-review` | `5.0.7` | `a5ff68586ccf62d1803cedeb71d60fd96ec05591d29c8d123196117eefd34cd0` | `superpowers:requesting-code-review` | `6.3.0` | `d71cc01ba56d2325cf8af5f7c11837819b63ecd57de0bfdb812f7f3ff7751df8` | `skills/references/superpowers-v6.3.0-b36e0829/skills/requesting-code-review/SKILL.md` |
| `superpowers:requesting-code-review/code-reviewer` | `5.0.7` | `7f5328dca12cb200005ae9d4386f63a9b0acb735ece57f82db206b4a3189ccae` | `superpowers:requesting-code-review/code-reviewer` | `6.3.0` | `bd0a281a0d983e53bca422eaa02fe496003fde40d5d5561775ad5f82a344416a` | `skills/references/superpowers-v6.3.0-b36e0829/skills/requesting-code-review/code-reviewer.md` |
| `superpowers:test-driven-development` | `5.0.7` | `7dee67b4af6bdccc7a914ca34533184d64592d0f5b23aeae631538168db14994` | `superpowers:test-driven-development` | `6.3.0` | `bf1b8216e523851a411e91d429a7c1c2a173e79d88957bc78e348218d50edd54` | `skills/references/superpowers-v6.3.0-b36e0829/skills/test-driven-development/SKILL.md` |
| `superpowers:test-driven-development/testing-anti-patterns` | `5.0.7` | `bde453bc258f06543987477c837939afaa774ea2acbd9f308d702fc452bc4283` | `superpowers:test-driven-development/writing-good-tests` | `6.3.0` | `51471c853306ff92ca8bb41dcaea05f31c0e46b03651f8f3c99754b7172f4ae1` | `skills/references/superpowers-v6.3.0-b36e0829/skills/test-driven-development/writing-good-tests.md` |
| `superpowers:using-git-worktrees` | `5.0.7` | `dcd1a83a2488bd557ceb7f14f2b6384ec209f551d18752dd9ceb70b9089dfb3b` | `superpowers:using-git-worktrees` | `6.3.0` | `8cfb86f121269e8f7f12361e6795c4f6738828340e28964c9229d365666c9edd` | `skills/references/superpowers-v6.3.0-b36e0829/skills/using-git-worktrees/SKILL.md` |
| `superpowers:writing-plans` | `5.0.7` | `90056bad3d5f196fa7c9fec0ffe592e6d9c86bc983e406642a51d1a4198b7024` | `superpowers:writing-plans` | `6.3.0` | `48508f44bbfd7d24b029fbf3a314f3cd14c9615599059366e922f47b8dc08cf2` | `skills/references/superpowers-v6.3.0-b36e0829/skills/writing-plans/SKILL.md` |

### Native adapters

| Phase | Resource ID | Version | Digest | Source |
|---|---|---:|---|---|
| `slice-plan-builder` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-plan-builder` | `tect:superpowers-v6-planning-and-execution` | `0.4.0-native.skills.1` | `260e0fea1363583d2b201f1022f25a5b870749972a0cfa7cedfefcd1bf8d2ad0` | `skills/pipelines/shared/superpowers-v6-planning-and-execution.md` |
| `slice-execution-runner` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-execution-runner` | `tect:superpowers-v6-test-quality` | `0.4.0-native.skills.1` | `892f63a5253c9d6517f9345bc1c102428d0abe50389e6347bff917556a7bbe39` | `skills/pipelines/shared/superpowers-v6-test-quality.md` |
| `slice-execution-runner` | `tect:superpowers-v6-workspace-and-finish` | `0.4.0-native.skills.1` | `1be702b53fa3c813bb67d09780fd117d2d1b4d12c416b9ffeb4f8248da7dae21` | `skills/pipelines/shared/superpowers-v6-workspace-and-finish.md` |
| `slice-execution-runner` | `tect:superpowers-v6-planning-and-execution` | `0.4.0-native.skills.1` | `260e0fea1363583d2b201f1022f25a5b870749972a0cfa7cedfefcd1bf8d2ad0` | `skills/pipelines/shared/superpowers-v6-planning-and-execution.md` |
| `slice-execution-runner` | `tect:superpowers-v6-code-review` | `0.4.0-native.skills.1` | `0bec6a0936effcf74f978747f84a880ad2067a2333d6de397f8ef44d865143e6` | `skills/pipelines/shared/superpowers-v6-code-review.md` |
| `slice-handoff-builder` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-handoff-builder` | `tect:superpowers-v6-workspace-and-finish` | `0.4.0-native.skills.1` | `1be702b53fa3c813bb67d09780fd117d2d1b4d12c416b9ffeb4f8248da7dae21` | `skills/pipelines/shared/superpowers-v6-workspace-and-finish.md` |

### Deliberately retained Superpowers bodies

| ID | Version | Digest | Preserved source |
|---|---:|---|---|
| `superpowers:brainstorming` | `5.0.7` | `bba47904a7f6bbee3bf8a107ebbe84e65d392be683bbb898ded736b29e415f90` | `skills/references/superpowers/brainstorming/SKILL.md` |
| `superpowers:brainstorming/visual-companion` | `5.0.7` | `0d3a8e7d63c665361f6a40b8dc48a259dceb98edb0c79a5931175f910373d6c1` | `skills/references/superpowers/brainstorming/visual-companion.md` |
| `superpowers:subagent-driven-development` | `5.0.7` | `081ad3869e55c80bf8f890b4768a90c0e8057daf94b1b6fadebfc85ea5b8304a` | `skills/references/superpowers/subagent-driven-development/SKILL.md` |
| `superpowers:subagent-driven-development/code-quality-reviewer-prompt` | `5.0.7` | `06d1e7c2287e5a00bd1809bf39038abd5f413b4cf917c37d30f00c48f6293421` | `skills/references/superpowers/subagent-driven-development/code-quality-reviewer-prompt.md` |
| `superpowers:subagent-driven-development/implementer-prompt` | `5.0.7` | `a416193f881e5a712c988fffabfe1d5a97bffcd091eb95577c84fe2136588617` | `skills/references/superpowers/subagent-driven-development/implementer-prompt.md` |
| `superpowers:subagent-driven-development/spec-reviewer-prompt` | `5.0.7` | `631980e472eec5394de8b89b69d432e54fd3f7f523ecf9afa7eb4cda0c9b2baf` | `skills/references/superpowers/subagent-driven-development/spec-reviewer-prompt.md` |
| `superpowers:verification-before-completion` | `5.0.7` | `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c` | `skills/references/superpowers/verification-before-completion/SKILL.md` |

The archived snapshot remains the exact carrier for stored runs created against the previous definition version.
