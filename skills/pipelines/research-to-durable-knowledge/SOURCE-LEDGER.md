# Research to Durable Knowledge source ledger

## Identity and native contract

- V1 source revision `ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8`; manifest `capabilities/pipelines/slice-variants/research-to-durable-knowledge.pipeline.json` SHA-256 `b0193f7aa0384b3b859214cad9b0a9c5f42bca0628fe353669c9dd013a160b3b`; ordered body-set digest `a41c19d278d182c120500f9473f9f5f3f5c0675ac23c1b0bf8a7e678930de5f2`.
- Native `slice.research-to-durable-knowledge@0.1.0-native.1`: `crates/host/pipeline-definitions/research-to-durable-knowledge.json`, semantic digest `94fa395b21023c45e803043b3aa2489dac9fe5032ebe1a6ddfd509e48a023c52`, physical SHA-256 `760b239cd61de2706aff3f9f1247335da1f748853ace3c05e40d4ba68e9ba87c`.
- Accepted default is `phasewise`; bounded `whole` and `phasewise` are allowed. Whole qualification requires finite questions, source set, evidence volume and durable output at begin. V1 did not define delivery transport.
- Coverage: 22/22 exact internal bodies, 114 verdict routes, 25 artifact requirements, 46 exact backward routes, and three phase skill reads representing three unique Superpowers bodies. Unresolved: zero.

## Exact internal bodies

| Body | SHA-256 |
|---|---|
| `slice-research-kb-entry-gate.step.md` | `3efabd6a1e719bf15d58faa6dd3f4de57b0b6146f9abc1921d61a3287c5f74fe` |
| `slice-research-kb-context-loader.step.md` | `8b3c03990f16a9cf1fc620c34fe0c24a639e76c591bc5cc9ab92ed5efbcebd70` |
| `slice-research-kb-contract-writer.step.md` | `219ab7fad45a723ceb8bc0f3df8ad2afec1596c71ad49a10ce1240372be25030` |
| `slice-research-question-framer.step.md` | `e135adce2578cfb5c1f5c1c2b37ed99fa2d5f5cd3f77aaf9772b7788de2b3028` |
| `slice-research-source-map-builder.step.md` | `cdd6c3e745032d84db680bd8eae8497ae6d41e3b93f8ab1772c8b015766a8ad3` |
| `slice-research-evidence-plan-builder.step.md` | `0d0ec9bc4421d7eace5e6c2a6e79f44f886647a4ffb070fbe0c4bf14e2c27b38` |
| `slice-research-task-decomposer.step.md` | `b5558053b160d13f08207ed4af6893886d05118ba6eb2b95e68a39453a3e6662` |
| `slice-research-subagent-assignment-manager.step.md` | `1743e1f54a96f25b77efb5becd555c78b42715a683e74f41e43cab813b0a888d` |
| `slice-research-evidence-collector.step.md` | `ce38870cf889fb3da1a294613d7f62c6278e71e7e6ed1341013664a1bee380d3` |
| `slice-research-evidence-ingester.step.md` | `b502dcbb9231efe1751c97419e22649b9aa4444b11c0ce4d4fb1a472d48eada7` |
| `slice-research-provenance-and-freshness-labeler.step.md` | `76655e0bda1a77e06ba82d9c653c9d06dcb9da83ed1e4544482a57e8e2040768` |
| `slice-research-claim-extractor.step.md` | `9a07868d70abbfc66b4a653b9e2cf98fe103988c940bd9d26e604aeb7032b9c7` |
| `slice-research-claim-ledger-builder.step.md` | `21ddd628bd0ba4b4062a4da92d35059fcb5e92503c69d06ce8099c029f611000` |
| `slice-research-negative-knowledge-capturer.step.md` | `88bb395864e421250cb12bb4257301d0ae86140a5333fccefa408e735c6299d2` |
| `slice-research-contradiction-and-gap-classifier.step.md` | `a19b41b8908efccc50650c3fb600b4ca08067bcbca966c3f5f197cd39a9bd4b7` |
| `slice-research-synthesis-builder.step.md` | `ce162e1f76d6eb3ae3a095c1d102887d78cdf3a46d41993d720983b90ba622e4` |
| `slice-research-durable-object-modeler.step.md` | `f27ab70178d0e663886246b84a6ccc4f5bfb1030f6177fffbfd83ce73783d7fd` |
| `slice-research-kb-seed-proposal-writer.step.md` | `1b21d3a1f206546a340db3d7bd63db5953eefdfbc77b5567936a4a81bcfe9faa` |
| `slice-research-promotion-edge-writer.step.md` | `540d65b1f6329c083e57fd94fd0358ee16739a92a2b01ec8ae2bb2a5caaf70a4` |
| `slice-research-promotion-gate.step.md` | `05439faf7a28627c1fb63e3df83c4ef30b35661df7876630006bb3d55d7ae588` |
| `slice-research-index-front-door-checker.step.md` | `5e95095efc08e8dcaa0df4b4cd05a27e23daf55fcd76e37b1bd2a4f7df04fed3` |
| `slice-research-result-and-handoff-writer.step.md` | `54e2446e6cc675d41b7d892260a55ff3228ae09803baf0060a61a3de29acf584` |

Each file under `v1/` is byte-exact. Native `output_contract` overlays keep the source research obligations while making retired V1 control-plane commands non-actionable.

## Skill closure and placement

The V1 manifest names three pipeline-level required skill refs but assigns none in `step_skill_refs`; every graph step invokes only its Tect-owned internal body. Native delivery binds each general obligation to the single closest owning phase so required skills are read rather than left as catalog metadata:

- `writing-plans` `90056bad3d5f196fa7c9fec0ffe592e6d9c86bc983e406642a51d1a4198b7024` at phase 6, limited to bounded sequencing, checkpoint and proof-plan discipline under the Research evidence-plan body;
- `dispatching-parallel-agents` `76806091c7f923ba2596546b19cccd98a08e57a68745df77c3a7b998fe838e2b` at phase 8, limited to independence qualification, self-contained packets, non-overlap, review and fan-in. Phase 8 records the external dispatch owner and handoff and must report `direct_dispatch_performed=false`; any actual authorized dispatch belongs to that parent/orchestrator owner, outside the assignment artifact. Phase 9 records the returned report count and exact dispatch evidence before phase 10 ingests every return into normal custody;
- `verification-before-completion` `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c` at phase 20, requiring fresh promotion-gate proof against the exact corpus, provenance, freshness, authority, contradiction, negative-knowledge, synthesis, seed and edge inputs.

All bodies are exact Superpowers `5.0.7` at `obra/superpowers@e7a2d16476bf042e9add4699c9d018a90f86e4a6`. The dispatch body was recovered from the exact original upstream revision; no mirror or newer revision was substituted. None of these reads grants external-source, subagent, code-execution or durable-write authority.

The current semantic digest is a post-runtime source-ownership refinement of the earlier `1ad098fde1f7dfffc3d2ff2ce43c297215c1f8bc012f58d97f93ee076b4a80c5` definition. The earlier definition passed the P6 lifecycle, but the refined definition requires a fresh rerun. It names the active parent/orchestrator and available native agent-dispatch action as the actual separately authorized dispatch owner, requires phase 9 to bind real returned reports before evidence custody, and names the unavailable durable-domain execution boundary explicitly. Backend receipt persistence is not dispatch or publication. A promoted verdict now requires separate non-`none` `external_promotion_owner`, `external_promotion_reference`, `external_promotion_digest`, and `external_promotion_authority_evidence` fields in addition to the source summary field; non-promoted routes report their absence as `none`.

## Evidence custody, claim truth and loop preservation

Phases 1-8 freeze the research target, question/source boundary, durable-lane candidate, evidence classes, source authority/freshness/access/safety, negative and contradiction probes, collection methods, tasks, assignment partitions and fan-in before collection. A no-subagent route is explicit and progress-complete; assignment contracts are not dispatch or evidence.

Phases 9-15 preserve each planned source as collected evidence or a named negative/gap record, ingest every pointer as accepted/alias/rejected/restricted/blocked/deferred custody, label every evidence row for provenance/freshness/authority, extract only atomic cited claims, account for every claim in the ledger, retain negative knowledge, and classify contradictions/gaps without silently resolving them. Phase 10 updates both `evidence-log.md` and the existing corpus index because its V1 body requires the secondary custody-index content even though the graph's primary `produces` list names only the log.

Phases 16-22 synthesize only ledgered claims, preserve inference labels and all limits, propose object/seed/edge records, make an explicit promotion decision, record index/front-door fit or deferral, and write the highest research truth. No phase imports canonical source corpus, writes canonical durable knowledge, rebuilds an index, executes researched code, or converts a proposal into accepted publication.

Source-supported rework routes return question/source/evidence gaps to the owning planning or collection phase; custody/provenance/claim defects to ingestion, labeling or ledger phases; contradictions to evidence or human review; and synthesis/object/seed/promotion defects to the earliest owning phase. The request must name the exact route-bound phase. Revisit stales that phase and successors. Gap-recorded, no-subagent, no-negative-knowledge, no-seed, no-edge, promotion-blocked and index-deferred remain explicit completed dispositions so the ordered pipeline never skips them.

## Promotion and terminal semantics

Phase 20 records `promotion_approved`, `promotion_not_required`, or `promotion_blocked` and always preserves excluded/restricted claims, target owner, conditions and next route. It owns proposal review and durable-domain handoff only. The selected downstream durable-domain pipeline owns authority/freshness reassessment, canonical write, provenance/index work and the promotion result. Missing material authority or restricted-source decisions may wait for human input; silence is not approval. Phase 21 records discoverability/index fit or an explicit deferral but performs no repair.

Phase 22 supports every exact completion-contract terminal token. `promoted_to_durable_kb` and `promoted_with_restrictions` are permitted only with non-`none` external promotion evidence from that downstream owner; the Research Slice itself must still report `durable_write_performed=false`. Otherwise it reports synthesized/proposed/blocked/rejected/handoff/escalated truth without upgrading the proposal boundary. All supported, gapped, blocked, rejected, source-only, restricted and deferred claims remain visible, with zero unowned follow-up.

Backend run/attempt/revision, exact artifact binding, skill-read receipts, typed constraints, transition and stale-on-revisit state replace only duplicate lifecycle carriers. Source observations, citations, authority, freshness, claim semantics, actual source access, subagent work and external promotion proof remain caller-reported evidence obligations. No current external research, subagent dispatch, durable publication or installation is performed by these assets.

## Superpowers v6.3.0 native adaptation

- Update class: selected upstream bodies only; native phase order and output contracts are unchanged.
- Previous active snapshot: `crates/host/pipeline-definitions/research-to-durable-knowledge-0.2.0-native.dk2.1.json`, version `0.2.0-native.dk2.1`, semantic digest `dcbfce20667d3c72157278df5948829fb212a7a8fca24dde8cdec4fca07467e3`, physical SHA-256 `8df9d96c5e973bfc234e0e3de8a87e9dc62804eb69161d35d69b674d5bbb3b06`.
- Current snapshot: `crates/host/pipeline-definitions/research-to-durable-knowledge.json`, version `0.4.0-native.skills.1`, semantic digest `374987b7516fe57c4de4282ace1a0fd80712e0bcac664ee08057ab340fc0b8ce`, physical SHA-256 `c454f774641337cb790ef9fa8c52e0908909865b3ece7259e5c2b947f0190317`.
- Exact selected source package: `skills/references/superpowers-v6.3.0-b36e0829`, upstream commit `b36e0829c6d0140e93cfef2ca599b1b07d4a7797`, tag `v6.3.0`, package manifest SHA-256 `2973ac33ed683d9c50e15fab14a5045f768b6b765d29849237f95371d99b5dde`, MIT license SHA-256 `a37e0e9697144819e1d965176ac4ae5bc3fa02d11e7812036bbcadf6dafe2400`.

### Selected body replacements

| Previous ID | Previous version | Previous digest | Current ID | Current version | Current digest | Current source |
|---|---:|---|---|---:|---|---|
| `superpowers:writing-plans` | `5.0.7` | `90056bad3d5f196fa7c9fec0ffe592e6d9c86bc983e406642a51d1a4198b7024` | `superpowers:writing-plans` | `6.3.0` | `48508f44bbfd7d24b029fbf3a314f3cd14c9615599059366e922f47b8dc08cf2` | `skills/references/superpowers-v6.3.0-b36e0829/skills/writing-plans/SKILL.md` |

### Native adapters

| Phase | Resource ID | Version | Digest | Source |
|---|---|---:|---|---|
| `slice-research-evidence-plan-builder` | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| `slice-research-evidence-plan-builder` | `tect:superpowers-v6-planning-and-execution` | `0.4.0-native.skills.1` | `260e0fea1363583d2b201f1022f25a5b870749972a0cfa7cedfefcd1bf8d2ad0` | `skills/pipelines/shared/superpowers-v6-planning-and-execution.md` |

### Deliberately retained Superpowers bodies

| ID | Version | Digest | Preserved source |
|---|---:|---|---|
| `superpowers:dispatching-parallel-agents` | `5.0.7` | `76806091c7f923ba2596546b19cccd98a08e57a68745df77c3a7b998fe838e2b` | `skills/references/superpowers/dispatching-parallel-agents/SKILL.md` |
| `superpowers:verification-before-completion` | `5.0.7` | `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c` | `skills/references/superpowers/verification-before-completion/SKILL.md` |

The archived snapshot remains the exact carrier for stored runs created against the previous definition version.
