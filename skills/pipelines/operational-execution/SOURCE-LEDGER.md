# Operational Execution source ledger

## Identity and native contract

- V1 source revision `ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8`; manifest `capabilities/pipelines/slice-variants/operational-execution.pipeline.json` SHA-256 `47ce8f063ad47fdb88f7e9b96f10ce101ed97de605333c29acc2630edd0db178`; ordered body-set digest `d7eb82e75b47c5dff813506cbc8dabdd62d6763ad033e7d44a0269386f631027`.
- Native `slice.operational-execution@0.1.0-native.1`: `crates/host/pipeline-definitions/operational-execution.json`, semantic digest `5c56887c41cfe9b6cae3309101059b4d643bb47e3db56ceb7d7ec80ae7214882`, physical SHA-256 `be7b7d7583b3288301ea0522cbd647a439e961ac825446ce5abdcbb15a96821d`.
- Accepted delivery is `phasewise` only. V1 did not define delivery transport.
- Coverage: 18/18 exact internal bodies, 72 verdict routes, 21 artifact requirements, and six phase skill reads representing five unique Superpowers bodies. Unresolved: zero.

## Exact internal bodies

| Body | SHA-256 |
|---|---|
| `slice-op-exec-entry-gate.step.md` | `ea2de460eba86dd482ada273f27fba7ba9299de2865ab3e1782f28c73dd6cb0c` |
| `slice-op-exec-authority-confirmation.step.md` | `1644c67f496e2e5994918f6b57756120d4cbba6141031ab81ce3de71313fe66b` |
| `slice-op-exec-context-loader.step.md` | `8c835080a9b65c5a1fc05878f9c39fbfac995b0936b980c6e2fd440b244282ba` |
| `slice-op-exec-contract-writer.step.md` | `ef2aca95a7c23e6bc5a946e06e9738572186525445f3ffd4c819282ee79b5ad4` |
| `slice-op-exec-current-state-baseliner.step.md` | `dcf5d7cb774b401dfde744f205f4aa82395361f95adc3b8f6474cd2e4abf72cd` |
| `slice-op-exec-preflight-runner.step.md` | `3c3e6811d456cff5463a189646b48caefd0db14146eaf5d1f6e67c78fae1168f` |
| `slice-op-exec-risk-stop-condition-checker.step.md` | `707830dd303d350d297c3a97f2dd787fa81ff3cfdbee6d8ed4d877476a1db841` |
| `slice-op-exec-final-approval-gate.step.md` | `d9e02fac200327cd3ab3918599bb1cdf73f5c85db90eebe26dd4184f145aacdd` |
| `slice-op-exec-command-ledger-builder.step.md` | `4cb52c4058bd10e5b2319596f72ea4176244dea95d32b94ed1e87d6e615e2f47` |
| `slice-op-exec-action-runner.step.md` | `d683fc101f7ed4208a95366a15b84ebbb67263c1f8d7644ff3ea2c4e903832a6` |
| `slice-op-exec-checkpoint-verifier.step.md` | `57a4a51669165114af94f70edd63445bdd508ce21963035ea6820b80835a3c55` |
| `slice-op-exec-post-action-validator.step.md` | `5e07ca21b5a9bedc18ceb0c76e69270ba41497012343349f9252b8ee5307c3e8` |
| `slice-op-exec-observation-window-manager.step.md` | `6ebf87de943dc8e387f3f1c3128ca0820bb0b8ef40b0d5d1da62797cd44fbdf4` |
| `slice-op-exec-rollback-or-recovery-runner.step.md` | `f5e6c8bbb134d1c4374937270377e06cdaf7b6b8adfb853c183caffcfd8264c8` |
| `slice-op-exec-recovery-notes-writer.step.md` | `8564f921dea2f1b7a182e3ed7ba46ebabdbc7e6a46ab41711c2d038edcea76e8` |
| `slice-op-exec-result-writer.step.md` | `1c330cc58afc4de577a017a016434c636670e7a6be34f2f56e5dbb6ecfe5938c` |
| `slice-op-exec-promotion-router.step.md` | `92ea6d1b35a36df11d25ca57386e3c6292bbcaa07284d6df756d7148fa9fdfd0` |
| `slice-op-exec-maintenance-and-handoff.step.md` | `8772fef3d8843da0d1f0bf3b82b6f965bf23a97edf5632f22a5533f29d7d9087` |

Each file under `v1/` is byte-exact. Native `output_contract` overlays keep the source operational obligations while making retired V1 control-plane commands non-actionable.

## Skill closure and placement

Exact Superpowers `5.0.7` bodies are pinned to `obra/superpowers@e7a2d16476bf042e9add4699c9d018a90f86e4a6`: `writing-plans` `90056bad3d5f196fa7c9fec0ffe592e6d9c86bc983e406642a51d1a4198b7024` at phase 4; `verification-before-completion` `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c` at phase 6; `executing-plans` `a711f83fb762e2ea0fa151f598893da9911a408895c91cc7a7e0770dd59a27b3`, mandatory nested `using-git-worktrees` `dcd1a83a2488bd557ceb7f14f2b6384ec209f551d18752dd9ceb70b9089dfb3b`, and closure `finishing-a-development-branch` `dd2f82c6dc8582b621f9eb57fcb65f557f88eadf872727ac81d0840ae12c504e` at phase 10; and direct `using-git-worktrees` at phase 18.

Skill reads expose process obligations only when their phase is active. Native target, authority, stop, checkpoint, action, rollback and proof rules remain controlling. No skill read grants deployment, mutation, rollback, cleanup, commit, merge or branch authority.

## Authority, effect and proof preservation

Every phase binds the exact source artifact, target identity and environment, authority basis, freshness basis, next owner and disposition. Authority confirmation records approver/source/time, action class, exact limits and rollback authority. Baseline, preflight, risk, final approval and action-ledger phases preserve current state, stale-after, drift, positive and negative checks, blast radius, irreversible effects, actors, exact actions, expected effects, checkpoints, timeouts, stop rules and rollback contract. Already-authorized work may proceed when target, risk and scope remain unchanged; material drift invalidates that authority.

Phase 10 executes at most one exact authorized ledger action and phase 11 must verify its checkpoint before another action. Phase 14 chooses no action, rollback, forward recovery, manual handoff or incident route from proven state and requires separate exact authority for an effectful response. Both phases use `reconciliation_required`: request IDs and backend replay make the recorded phase attempt idempotent, but never repeat an external effect. Missing response or uncertain effect records `unknown_external_outcome`, blocks, and requires a fresh reconciliation of the target before any retry decision. Exit zero, an action log or an attempted rollback is not final-state proof.

Post-action validation retains positive and negative checks, proof class, exact target binding and missing proof. Observation records signal, cadence, threshold and pass/fail/inconclusive/not-required truth. Recovery notes preserve before/after state, proof refs, missing proof and residual risk. Result uses the exact V1 terminal truths plus an explicit partial result; promotion remains candidate/deferred/no-promotion classification without durable write.

## Partial continuation and native adaptation

V1 requires promotion and maintenance/handoff after `result.md`, then detects `Outcome: partial.` and uses an absent handoff pointer to default to authority confirmation or exactly one `Resume step: <step-id>` naming an existing phase. Native phase 16 therefore records the immutable partial truth and continues. Phase 17 records promotion/deferred disposition. Phase 18 records all five maintenance checkpoints and then selects one of 18 typed revisit routes: default authority confirmation or one exact earlier phase. The reported `resume_step` field, count and request `revisit_phase_id` are bound to the same route; invalid, duplicate or future pointers cannot advance. Every revisited phase reruns its own freshness, authority, preflight, stop and proof gates. Partial never satisfies pipeline completion.

Backend run/attempt/revision, output binding, replay and stale-on-revisit state replace only duplicate V1 lifecycle carriers. Target facts, authority, commands, results, observations and evidence remain caller-reported and must be independently supported. V1 Hybrid escalation is represented as a future Slice graph with separate implementation and Operational Execution Slices; its target, trigger, evidence, order, authority, rollback and proof obligations remain intact. No current installation, deployment or operation is performed by these assets.

## Superpowers v6.3.0 native adaptation

- Update class: selected upstream bodies only; native phase order and output contracts are unchanged.
- Previous active snapshot: `crates/host/pipeline-definitions/operational-execution-0.1.0-native.1.json`, version `0.1.0-native.1`, semantic digest `5c56887c41cfe9b6cae3309101059b4d643bb47e3db56ceb7d7ec80ae7214882`, physical SHA-256 `be7b7d7583b3288301ea0522cbd647a439e961ac825446ce5abdcbb15a96821d`.
- Current snapshot: `crates/host/pipeline-definitions/operational-execution.json`, version `0.4.0-native.skills.1`, semantic digest `8a1be05166244cffae5456f476d9748051f4786f3c9c2f4744b1abace4facdb9`, physical SHA-256 `be3e6e5d2ae29e84b1bf3e1818b50e7587f5128b4c5686a153538898361facbb`.
- Exact selected source package: `skills/references/superpowers-v6.3.0-b36e0829`, upstream commit `b36e0829c6d0140e93cfef2ca599b1b07d4a7797`, tag `v6.3.0`, package manifest SHA-256 `2973ac33ed683d9c50e15fab14a5045f768b6b765d29849237f95371d99b5dde`, MIT license SHA-256 `a37e0e9697144819e1d965176ac4ae5bc3fa02d11e7812036bbcadf6dafe2400`.

### Selected body replacements

| Previous ID | Previous version | Previous digest | Current ID | Current version | Current digest | Current source |
|---|---:|---|---|---:|---|---|
| `superpowers:executing-plans` | `5.0.7` | `a711f83fb762e2ea0fa151f598893da9911a408895c91cc7a7e0770dd59a27b3` | `superpowers:executing-plans` | `6.3.0` | `c4c3d8b628c51114cd165fb8246fe02744cd8be180032328391252e653028d9b` | `skills/references/superpowers-v6.3.0-b36e0829/skills/executing-plans/SKILL.md` |
| `superpowers:finishing-a-development-branch` | `5.0.7` | `dd2f82c6dc8582b621f9eb57fcb65f557f88eadf872727ac81d0840ae12c504e` | `superpowers:finishing-a-development-branch` | `6.3.0` | `8db5a922b242dd4e1bf824cb91c13b3e8d8e8a86d6ceaf7f0774eb9cce909d65` | `skills/references/superpowers-v6.3.0-b36e0829/skills/finishing-a-development-branch/SKILL.md` |
| `superpowers:using-git-worktrees` | `5.0.7` | `dcd1a83a2488bd557ceb7f14f2b6384ec209f551d18752dd9ceb70b9089dfb3b` | `superpowers:using-git-worktrees` | `6.3.0` | `8cfb86f121269e8f7f12361e6795c4f6738828340e28964c9229d365666c9edd` | `skills/references/superpowers-v6.3.0-b36e0829/skills/using-git-worktrees/SKILL.md` |
| `superpowers:writing-plans` | `5.0.7` | `90056bad3d5f196fa7c9fec0ffe592e6d9c86bc983e406642a51d1a4198b7024` | `superpowers:writing-plans` | `6.3.0` | `48508f44bbfd7d24b029fbf3a314f3cd14c9615599059366e922f47b8dc08cf2` | `skills/references/superpowers-v6.3.0-b36e0829/skills/writing-plans/SKILL.md` |

### Native adapters

| Phase | Resource ID | Version | Digest | Source |
|---|---|---:|---|---|
| `slice-op-exec-contract-writer` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-op-exec-contract-writer` | `tect:superpowers-v6-planning-and-execution` | `0.4.0-native.skills.1` | `260e0fea1363583d2b201f1022f25a5b870749972a0cfa7cedfefcd1bf8d2ad0` | `skills/pipelines/shared/superpowers-v6-planning-and-execution.md` |
| `slice-op-exec-action-runner` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-op-exec-action-runner` | `tect:superpowers-v6-workspace-and-finish` | `0.4.0-native.skills.1` | `1be702b53fa3c813bb67d09780fd117d2d1b4d12c416b9ffeb4f8248da7dae21` | `skills/pipelines/shared/superpowers-v6-workspace-and-finish.md` |
| `slice-op-exec-action-runner` | `tect:superpowers-v6-planning-and-execution` | `0.4.0-native.skills.1` | `260e0fea1363583d2b201f1022f25a5b870749972a0cfa7cedfefcd1bf8d2ad0` | `skills/pipelines/shared/superpowers-v6-planning-and-execution.md` |
| `slice-op-exec-maintenance-and-handoff` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-op-exec-maintenance-and-handoff` | `tect:superpowers-v6-workspace-and-finish` | `0.4.0-native.skills.1` | `1be702b53fa3c813bb67d09780fd117d2d1b4d12c416b9ffeb4f8248da7dae21` | `skills/pipelines/shared/superpowers-v6-workspace-and-finish.md` |

### Deliberately retained Superpowers bodies

| ID | Version | Digest | Preserved source |
|---|---:|---|---|
| `superpowers:verification-before-completion` | `5.0.7` | `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c` | `skills/references/superpowers/verification-before-completion/SKILL.md` |

The archived snapshot remains the exact carrier for stored runs created against the previous definition version.
