# Custom Procedure Capture source and native delivery ledger

## Source identity and exact assets

The source authority is `plugins/tect/capabilities/pipelines/slice-variants/procedure-capture.pipeline.json` at Tect V1 revision `ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8`, SHA-256 `a8b936604f09e1e3c9b0e37656fb0110e4ab688b72b9eba68ad9082f0763ce07`. The native definition is `crates/host/pipeline-definitions/procedure-capture.json`, version `0.1.0-native.1`. It preserves the accepted delivery policy: default `whole`, allowed `whole` and `phasewise`.

All 17 selected internal bodies are byte-exact snapshots under `skills/pipelines/procedure-capture/v1/`. Their ordered body-set digest is `fd89364a3894fe962ff5384572a2aa7d54f226b72b3575ef142b149217695289`.

| Phase | Body | SHA-256 |
|---:|---|---|
| 1 | `slice-procedure-capture-entry-gate.step.md` | `0d56dcdbf3d17e7a90bd98aca1f56ec71152f6d3db4e3f7d792b10fedaf10717` |
| 2 | `slice-procedure-source-context-loader.step.md` | `a5e3c7ef4cbab8c72825701735d3e2754541a5c896c2ba71cd6109f0ea916189` |
| 3 | `slice-procedure-event-extractor.step.md` | `e140bee9fa5f4da4b91948474f50299eab210ff18a5186d598e2d9747da497f0` |
| 4 | `slice-procedure-step-normalizer.step.md` | `53ff33f66e245a9919a15d521eb105b4c9b3c5efc1a466dbb8baaf00cc8ad96f` |
| 5 | `slice-procedure-generalization-shaper.step.md` | `6310cbab8d116079deaaeb6f0cae1e0f726f67d9e55e12b37f16e2e237147ad4` |
| 6 | `slice-procedure-existing-match-checker.step.md` | `0a80fbd39f115ee5ae3da716d290498f7ef6c15cd96efdc9070956ce808c1d64` |
| 7 | `slice-procedure-durable-target-selector.step.md` | `7b9281a715eb1084dcf748e65b221436830ce792086767cc9b5179eec8530909` |
| 8 | `slice-procedure-authority-and-risk-classifier.step.md` | `3740c0529857022240eff573c6acb1da6df217a7916f0c6a1e0645888d7466b5` |
| 9 | `slice-procedure-proof-contract-builder.step.md` | `cfc5dfff3e588e5ded22443aab8381a2f181c06f50da3c9d3312c7690a03db3c` |
| 10 | `slice-procedure-secret-safety-scrubber.step.md` | `197b719f60bb8abb5ff5b149c9a5db86e343d38c7ee65e7630c5522f84e1473d` |
| 11 | `slice-procedure-reuse-fit-evaluator.step.md` | `5a1b5282af1b055b75e6b736084f73c59a155253e4467c8619f1835c08ef04a6` |
| 12 | `slice-procedure-validation-runner.step.md` | `103573cdc24fb8b3f0e478d339361094b030b95a33271f34c08a6f96b60d7e0a` |
| 13 | `slice-procedure-proposal-writer.step.md` | `b96528c6eb7e36878fd08eb8069ef648a6194ba79f9862f8407e4710985855a1` |
| 14 | `slice-procedure-skill-candidate-router.step.md` | `7be25c3b41e2ff71ff02f4bfbba2ba5abc122759eaa7965286b4621ada7750cd` |
| 15 | `slice-procedure-promotion-gate.step.md` | `77679e7588b32cb16cb4fb5da72ae534383a93fb3d581bb61f06d1d4dd127634` |
| 16 | `slice-procedure-result-writer.step.md` | `2aaf6f91d577b28cb90e50a6730ef3a1ef0788396e10b398dc72821a5a5a8fd5` |
| 17 | `slice-procedure-maintenance-and-handoff.step.md` | `8c22cb3548e3c576e9d96fe88cc8de4b2675880dac60178291984bcdfd09220a` |

Five phase skill reads bind four unique exact bodies. `writing-plans` at phase 1 is `90056bad3d5f196fa7c9fec0ffe592e6d9c86bc983e406642a51d1a4198b7024`; `verification-before-completion` at phases 10 and 12 is `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c`; `using-git-worktrees` at phase 17 is `dcd1a83a2488bd557ceb7f14f2b6384ec209f551d18752dd9ceb70b9089dfb3b`. These are exact Superpowers `5.0.7` snapshots from `obra/superpowers@e7a2d16476bf042e9add4699c9d018a90f86e4a6`.

Phase 6 also requires `tect-query`. The manifest declares `skills/tect-query/SKILL.md`, but that path does not exist at the selected V1 revision: commit `1c1f503` removed the old owner skill during public-surface folding. `capabilities/instructions/public-surface-archive/tect-query.skill.md` is byte-identical to the last historical body at `1c1f503^`, SHA-256 `b317a2cc21ce01dfd5ff56b97c6f21294c7f0462623988ead006cc264185041b`, and is vendored at `skills/references/tect-query/SKILL.md`. Its source/provenance/freshness/negative-finding obligations are active. Its retired V1 route materialization and governance tool commands are provenance only; the native phase requires the active agent to perform the bounded read-only comparison through available file, repository, document or query actions and name the actual owner/action/evidence. The backend persists receipts; it does not perform the search.

## Ordered native contracts and source adaptations

| Phase | Native output contract and critical routing |
|---:|---|
| 1 | Classify a real source event and candidate lane. Route existing-runbook execution, research, adoption, operations or direct skill authoring away. `slice.md` records authority, secret posture and no durable write. |
| 2 | `README.md` plus `source-slice-link.md` bind the exact source event, source availability, freshness, authority and sensitivity. |
| 3-5 | `source-event.md`, `captured-steps.md`, and `normalized-procedure.md` preserve chronology, evidence, failed variants, parameters/invariants, preconditions, proof, stop/recovery and unknowns. Exact rework returns only to source/context/normalization phases. |
| 6 | `existing-match-check.md` must classify `no_match`, `exact_match`, `partial_match`, `stale_or_superseded_match`, `conflict`, or a truthful block. Every classification records search terms, source classes, inspected/unavailable locations, freshness, negative findings, match refs, route and read-only execution evidence. `no_match` alone advances. Exact/partial/stale/conflict stops at this phase and hands off to the real existing-artifact, maintenance, domain or human owner without inventing a callable route. |
| 7 | `procedure-proposal.md` holds the semantic candidate target. `normalized-procedure.md` is also returned as the explicit native projection of the V1 progress carrier, whose target marker lived there. Both must agree. Candidate choices include procedure/runbook/recipe/proof/KB/domain/skill/update/follow-up/no-target; none performs publication. |
| 8-9 | `authority-risk.md` classifies every read/write/execute/deploy/install/migrate/credential/live/promotion action and required owner/approval. `proof-contract.md` defines positive/negative, freshness, authority, local/live, stop, recovery and missing-proof obligations without execution. |
| 10 | `secret-safety.md` applies fresh verification to the exact candidate. `safe_to_continue` structurally requires integer `leak_count=0`; secret-bearing or unreviewable inputs block. No secret retrieval or target execution is allowed. |
| 11 | `reuse-fit.md` preserves all ten source verdicts and the seven-check score. `reuse_fit_recorded` means the evaluation exists; it is not a positive reuse finding. Only five positive candidate verdicts advance. Duplicate/update-only, context-bound, unsafe, vague, not-worth-capture and blocked verdicts stop at this phase and name the external existing-artifact, source/domain, safety/authority or decision owner. There is no execution-based reuse proof in this pipeline. |
| 12 | `proof-order-template.md` is required only for `candidate-ready-for-proposal`, the only advancing verdict. Every verdict also returns `proof-contract.md` as the explicit V1 progress-carrier projection with an honest pass/not-required/blocked disposition. Rejected, missing-source/proof, existing-update, handoff and blocked verdicts stop; missing authority waits at this same phase for a real human decision. Validation is review/simulation of proposal readiness and never executes the captured procedure. |
| 13 | `procedure-proposal.md` is always the Slice-local packet. `runbook-draft.md` is required only for runbook/update verdicts; `command-recipe.md` only for the recipe verdict. A rejected or context-bound candidate uses an explicit no-promote proposal, not durable guidance. |
| 14 | `skill-candidate.md` exists only when skill intake is justified. `procedure-proposal.md` carries the required `skill_candidate_recorded` or `skill_candidate_not_required` disposition. `no_activation=true` is enforced. This phase does not create, edit, register, install, activate or promote a skill. |
| 15 | `promotion-gate.md` records eligible, review, not-eligible, blocked, decision-wait or exact rework. `durable_write_performed=false` is enforced. Eligibility is a candidate for a separate owner, not approval or publication. |
| 16 | `result.md` records one manifest terminal truth. This Slice enforces `durable_write_performed_by_this_slice=false`, `skill_activation_performed_by_this_slice=false`, and `procedure_execution_performed_by_this_slice=false`. `procedure_promoted_after_approval` additionally requires external promotion owner, reference, digest and authority evidence, each different from literal `none`; absent that proof, use proposed/rejected/blocked/handoff truth. |
| 17 | `result.md` carries the final V1 progress disposition. Conditional `handoff.md`/`deferred.md` preserve next owner/action/evidence, resume/stale rules, maintenance and cleanup candidates. Worktree reading is inventory only; `cleanup_performed=false` and `maintenance_performed=false` are enforced. |

The definition has 17 required phases, 128 typed verdict routes, 32 artifact requirements, five phase skill reads, and zero unresolved required bodies. All retry policies are `repeatable`; this proposal-only pipeline has no external effect to replay. Source stop routes at phases 6, 11 and 12 use backend `blocked` state at the same phase. For an actual outgoing handoff, the caller publishes the existing managed blocked Result with exact summary, evidence, scope impact and remaining work. Resumption accepts only a new exact input at the current run revision and retries that same phase as a new immutable attempt. A missing-authority decision at phase 12 waits at that phase without a Result. No stopped output advances into target selection, validation, proposal or promotion eligibility.

## Registration, publication, promotion and reuse ownership

No phase in Custom Procedure Capture owns actual durable registration, publication, activation or procedure execution. Phase 11 evaluates reuse fit from the captured source event and does not prove a future reuse. Phase 12 validates proposal readiness without executing the procedure. Phase 14 produces an executable intake packet for a separate skill-authoring/adoption owner. Phase 15 produces a promotion-review packet for the named runbook-library, durable-domain, domain-object or skill owner. Phase 16 may report promotion only after separately authorized external work returns exact owner, target/reference, digest and authority evidence. Phase 17 preserves that packet and its resume trigger.

At the original seven-pipeline migration, downstream publication was outside the native catalogue. DK-2 subsequently connected this producer to the actual Knowledge Change publisher through exact receipts; the current revision4 catalogue contains nine choices including Promotion. Procedure Capture itself still does not own publication, activation or procedure execution. Proposal, rejection, block or handoff remains the honest outcome unless the separately authorized owner returns actual evidence. A handoff names the real owner and target, source/proof/authority/conditions and expected return; it must not invoke a nonexistent V1 route. The new Research/Brainstorming delivery does not add a skill-authoring or activation owner.

## Verification

The native definition passed direct `tect_domain::PipelineDefinitionSnapshot` deserialization, blank-digest serde SHA-256 recomputation and `validate()` against the current compile-green DTO. The semantic digest is `30cbe0da5f26c8703341f4203c270ef158babf0672d07bbbbd637a06dbcdb1bc`; the physical JSON SHA-256 is `d546eb4dd143bc969c3ae383b07282c9edd699692daa50565441b61d88c98e9c`.

## Superpowers v6.3.0 native adaptation

- Update class: selected upstream bodies only; native phase order and output contracts are unchanged.
- Previous active snapshot: `crates/host/pipeline-definitions/procedure-capture-0.2.0-native.dk2.1.json`, version `0.2.0-native.dk2.1`, semantic digest `3c89a8ccc9f4f932946da4494b97aeadaaf1199c38371fa85fb0516ed5ff817f`, physical SHA-256 `a7efc016eed71d14aef218535fb77c537b8b7076b97ec4ba65676feda2ab302e`.
- Current snapshot: `crates/host/pipeline-definitions/procedure-capture.json`, version `0.4.0-native.skills.1`, semantic digest `b8bb5affd153f1642f120625f71fd6f0b0a1b5dd877f2e46cc0cb589f8153b8c`, physical SHA-256 `1625e8951c19abd5b28032c5d8482bce8dfe745c5155d985b1ca0ad6b807b968`.
- Exact selected source package: `skills/references/superpowers-v6.3.0-b36e0829`, upstream commit `b36e0829c6d0140e93cfef2ca599b1b07d4a7797`, tag `v6.3.0`, package manifest SHA-256 `2973ac33ed683d9c50e15fab14a5045f768b6b765d29849237f95371d99b5dde`, MIT license SHA-256 `a37e0e9697144819e1d965176ac4ae5bc3fa02d11e7812036bbcadf6dafe2400`.

### Selected body replacements

| Previous ID | Previous version | Previous digest | Current ID | Current version | Current digest | Current source |
|---|---:|---|---|---:|---|---|
| `superpowers:using-git-worktrees` | `5.0.7` | `dcd1a83a2488bd557ceb7f14f2b6384ec209f551d18752dd9ceb70b9089dfb3b` | `superpowers:using-git-worktrees` | `6.3.0` | `8cfb86f121269e8f7f12361e6795c4f6738828340e28964c9229d365666c9edd` | `skills/references/superpowers-v6.3.0-b36e0829/skills/using-git-worktrees/SKILL.md` |
| `superpowers:writing-plans` | `5.0.7` | `90056bad3d5f196fa7c9fec0ffe592e6d9c86bc983e406642a51d1a4198b7024` | `superpowers:writing-plans` | `6.3.0` | `48508f44bbfd7d24b029fbf3a314f3cd14c9615599059366e922f47b8dc08cf2` | `skills/references/superpowers-v6.3.0-b36e0829/skills/writing-plans/SKILL.md` |

### Native adapters

| Phase | Resource ID | Version | Digest | Source |
|---|---|---:|---|---|
| `slice-procedure-capture-entry-gate` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-procedure-capture-entry-gate` | `tect:superpowers-v6-planning-and-execution` | `0.4.0-native.skills.1` | `260e0fea1363583d2b201f1022f25a5b870749972a0cfa7cedfefcd1bf8d2ad0` | `skills/pipelines/shared/superpowers-v6-planning-and-execution.md` |
| `slice-procedure-maintenance-and-handoff` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-procedure-maintenance-and-handoff` | `tect:superpowers-v6-workspace-and-finish` | `0.4.0-native.skills.1` | `1be702b53fa3c813bb67d09780fd117d2d1b4d12c416b9ffeb4f8248da7dae21` | `skills/pipelines/shared/superpowers-v6-workspace-and-finish.md` |

### Deliberately retained Superpowers bodies

| ID | Version | Digest | Preserved source |
|---|---:|---|---|
| `superpowers:verification-before-completion` | `5.0.7` | `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c` | `skills/references/superpowers/verification-before-completion/SKILL.md` |

The archived snapshot remains the exact carrier for stored runs created against the previous definition version.
