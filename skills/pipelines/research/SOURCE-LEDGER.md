# Standalone Research source ledger

Native pipeline: `slice.research`. Initial native version: `0.5.0-native.inquiry.1`. Semantic methods and adaptation decisions authored personally by the primary Astra for Tony's approved Research12 design on 2026-09-15. This is a new executable definition; no prior combined Research definition is rewritten.

## Source and ownership

The substantive source is the complete set of 22 retained Tect V1 bodies in [the combined Research package](../research-to-durable-knowledge/v1/), originating at Tect V1 commit `ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8`. Their preserved native history, corrections and publisher boundaries are recorded in [its ledger](../research-to-durable-knowledge/SOURCE-LEDGER.md). Every source body was read, including collection/custody secondary outputs and publication/index/result duties.

The new methods are native authored adaptations, not renamed copies or references to missing V1 runtime paths. They run under [inquiry-boundary](../shared/inquiry-boundary.md) and the actual backend phase/output/knowledge/checkpoint contracts. Their internal IDs are `tect:<method filename without .md>`. Exact bodies are delivered in phase.skills, with version and SHA-256; each requires a skill-read receipt. Shared native instructions and adapter resources have separate exact identities. No global auto-trigger skill is installed.

R03 also receives the retained `superpowers:writing-plans` 6.3.0 source body and the existing `superpowers-v6-native-boundary` / `superpowers-v6-planning-and-execution` adapters. R11 receives the retained `superpowers:verification-before-completion` body and native boundary. Existing upstream attribution/license files remain in the reference packages. Current package paths and exact hashes are authoritative; this ledger makes no claim that a future upstream release has been fetched.

R04 does not inject dispatching-parallel-agents into every run. Its assignment/custody obligations survive, but the primary agent is the normal intellectual worker under Tony's policy. Delegation requires explicit task authority and actual bounded evidence-return handling. A source reference to dispatch methodology is not an instruction to dispatch.

## Lossless obligation mapping

| Retained V1 phase | New owner | Preserved obligation or deliberate architectural relocation |
|---|---|---|
| P01 entry gate | R01 | Fit, question, source scope, parent context, authority/freshness, missing-input routes and no implicit execution/publication |
| P02 context loader | R01/R02 | Parent decisions and DK/prior research, source roles, stale/generated/memory/restricted limits, missing context and next evidence needs |
| P03 contract writer | R01 | Bounded purpose, source/proof/freshness/use policy, artifact responsibility, sufficiency and handoff; mandatory early durable-lane/field modeling deliberately moves to separately requested KC |
| P04 question framer | R02 | Question IDs, decision links, assumptions, acceptance/proof/confidence, exclusions and disconfirmation cues |
| P05 source map | R03 | Source families/roles/locators, authority/currentness/access, allowed/excluded sources, negative/contradiction probes and explicit gaps |
| P06 evidence plan | R03 | Ordered bounded permitted reads, acceptance/rejection and citation policy, freshness/access, negative evidence, context budget and stops |
| P07 task decomposition | R04 | Question/source partitions, dependencies, proof/allowed reads, output/return contracts, risks, fan-in and coverage |
| P08 assignments | R04 | Actual owner or solo pass, non-overlap/coordination, source limits, isolated outputs, provenance-return and intake; dispatch is not required or implicitly authorized |
| P09 collection | R05 | Citation at collection, source date versus read time, safe raw/derived pointers, restrictions, negative searches/access failures, source-map deltas and all returned work accounted |
| P10 ingestion | R06 | Stable IDs, immutable raw pointers, aliases and all accepted/rejected/restricted/blocked/deferred dispositions; balanced intake counts; both evidence-log and normalized corpus-index |
| P11 qualification | R06 | Every evidence row labeled; separate provenance, derivation, freshness, authority, scope and allowed use; source-parent links and exact refresh/review blockers |
| P12 extraction | R07 | Atomic typed claims with evidence, modality, conditions/exceptions, confidence/freshness/scope, conflict/gap posture and publication limits |
| P13 ledger | R07 | Every extracted row accounted, separate support/status/use limits, exact evidence/gap links, counts, forbidden-publication reasons and next proof |
| P14 negative knowledge | R08 | Inspected scope even when empty, narrow absence/disproof/dead-end claims, exclusions, inaccessible versus inspected distinction, freshness/use limits and revisit triggers |
| P15 contradictions/gaps | R08 | Typed factual/temporal/scope/authority/provenance/version conflicts, per-claim coverage, blocking versus limits, missing proof and exact owner; no convenient winner without evidence |
| P16 synthesis | R10 after R09 | Answers derived from ledger, explicit inference, all limitations/negative/blocked/deferred material, provenance and applicable recommendation; R09 adds finite information-value-based follow-up |
| P17 durable object model | KC05 | Identity/type/profile/applicability, owner, field-to-claim mapping, merge/update/new/retire decisions, lifecycle and review needs; not required merely to finish Research |
| P18 seed proposal | KC05 | Exact proposed RDF changeset and wording from selected operational sources, citations, exclusions, conditions, target placement and publication prerequisites |
| P19 promotion edge | KC05/KC07 | Source-to-unit trace, claim/evidence/currentness/use limits, replacement/bindings/consumer impact and non-promotable material |
| P20 promotion gate | R11 and KC04/06/08/09 | R11 verifies the research answer; KC separately checks publication evidence/profile/authority/review/reconciliation and obtains real backend receipts |
| P21 index/front door | KC11/effects | Actual discoverability/exact delivery and separate lexical/vector readiness; accountable deferred effects remain explicit |
| P22 result/handoff | R12 plus KC result | Highest supported answer and exact output links, limitations, rejected/source-only/restricted/deferred work, next owner and optional reuse candidates; publication proof belongs to actual KC |

## Intentional changes

Research12 answers questions independently of publication. It may complete without a candidate RDF schema, seed, publisher receipt or index mutation. Publication responsibilities are not discarded: they belong to the already executable Knowledge Change when separately requested. Optional source selection in R12 does not itself perform object modeling or satisfy domain obligations.

The old seven-heading convention, stale HTML/manifest paths, V1 validator commands, per-artifact magic gate lines and mandatory subagent-themed filenames were packaging for a different runtime. They are replaced by the real native required artifacts/fields, exact read receipts, routes and preserved semantic methods. New methods do not instruct agents to run nonexistent V1 tools.

R09 allows a targeted backward return with a named expected information gain and stop condition. The immutable inquiry controls bounded inconclusive completion. R11 checks a useful answer rather than counting documents; R12 cannot turn retrieval failure into disproof or operational output into durable authority.

## Packaging and verification evidence

The provider must load `crates/host/pipeline-definitions/research.json` as a typed PipelineDefinitionSnapshot. Compute its semantic digest after clearing only the top-level digest and serializing with the native Rust type; compute body digests from exact UTF-8 bytes. Physical JSON file SHA-256 is distinct. The packaging receipt records these values and the 12 method/resource placements; schema success alone is not proof of sound research.

Behavioral verification includes question/result contract, negative and inconclusive boundaries, current exact output consumption, source checkpoint return, topic-level DK projection and preserved legacy runs. No separate perpetual audit harness is introduced.

## Exact packaged identities

- Definition version: `0.5.0-native.inquiry.1`
- Semantic digest: `d3425b463b589897cc4c66157fa8d1bfc05ef200f7593ca46e7e4f566073e612`
- Physical JSON SHA-256: `cbcc8d377e7227990711e7630d9e71dd991f478b8c24efefb741f81f123efd06`

| Phase | Role | ID | Version | Body SHA-256 | Source |
|---|---|---|---|---|---|
| overview | overview | `tect:research-overview` | `0.5.0-native.inquiry.1` | `4233735d61430482725fdb6b2ac0ea73b83f1a04e0ff99480a94ab99d83f78cc` | `skills/pipelines/research/overview.md` |
| R01 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R01 | skill | `tect:research-contract` | `0.5.0-native.inquiry.1` | `761e9d1e691bc56817bf9b999aa4889521ada0438c322d54fc8bc27d5a46b420` | `skills/pipelines/research/methods/research-contract.md` |
| R02 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R02 | skill | `tect:research-questions-context` | `0.5.0-native.inquiry.1` | `c997754bc5bb8ace136481e99bcdc0a54d73607233abaf4d7e716f74a7b4ffba` | `skills/pipelines/research/methods/research-questions-context.md` |
| R03 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R03 | skill | `tect:research-source-evidence-plan` | `0.5.0-native.inquiry.1` | `0f428f3e0b5549d40d0b6ff44c211d8c0b53a1393c7041366301a4797ca66cb3` | `skills/pipelines/research/methods/research-source-evidence-plan.md` |
| R03 | skill | `superpowers:writing-plans` | `6.3.0` | `48508f44bbfd7d24b029fbf3a314f3cd14c9615599059366e922f47b8dc08cf2` | `https://github.com/obra/superpowers/blob/b36e0829c6d0140e93cfef2ca599b1b07d4a7797/skills/writing-plans/SKILL.md`<br>`skills/references/superpowers-v6.3.0-b36e0829/skills/writing-plans/SKILL.md` |
| R03 | resource | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| R03 | resource | `tect:superpowers-v6-planning-and-execution` | `0.4.0-native.skills.1` | `260e0fea1363583d2b201f1022f25a5b870749972a0cfa7cedfefcd1bf8d2ad0` | `skills/pipelines/shared/superpowers-v6-planning-and-execution.md` |
| R04 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R04 | skill | `tect:research-work-custody-plan` | `0.5.0-native.inquiry.1` | `1daaea54fee7aee71381edbcedc9723769abacb56eb4e27b59c70f4c7adcd6c2` | `skills/pipelines/research/methods/research-work-custody-plan.md` |
| R05 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R05 | skill | `tect:research-collection` | `0.5.0-native.inquiry.1` | `f592b762645980e7b3ae57453f79f7c310532da7256bc626cc48bde17555e61b` | `skills/pipelines/research/methods/research-collection.md` |
| R06 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R06 | skill | `tect:research-source-qualification` | `0.5.0-native.inquiry.1` | `12bafbea7bf8e6dc9ca0b1c33a88dcd13eeda9c1bcf2e1ffbe90c17e1cbb559e` | `skills/pipelines/research/methods/research-source-qualification.md` |
| R07 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R07 | skill | `tect:research-claims-ledger` | `0.5.0-native.inquiry.1` | `1ef7a2af3a7986fbc0a6545a8887827e7621707ae7efa58b085a11b7962e3c5d` | `skills/pipelines/research/methods/research-claims-ledger.md` |
| R08 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R08 | skill | `tect:research-contradictions-negative` | `0.5.0-native.inquiry.1` | `54588cf164e047ed047a5bc656552b3e3ef3f3ba0757e29d08aaad6a0ddc176d` | `skills/pipelines/research/methods/research-contradictions-negative.md` |
| R09 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R09 | skill | `tect:research-sufficiency` | `0.5.0-native.inquiry.1` | `fbde2b96da2628ac5915f21ce39b1c04b0066e757a5080c7c8a86484592b4aa8` | `skills/pipelines/research/methods/research-sufficiency.md` |
| R10 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R10 | skill | `tect:research-synthesis` | `0.5.0-native.inquiry.1` | `c69292588a5b112bed9bfa6a76187689c8cca23d47e2123f9eaadf3d394de76d` | `skills/pipelines/research/methods/research-synthesis.md` |
| R11 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R11 | skill | `tect:research-result-verification` | `0.5.0-native.inquiry.1` | `0075eadb8d6d01383976b8081b14f0605d94c07caed43073d9e8b28c5b0f6043` | `skills/pipelines/research/methods/research-result-verification.md` |
| R11 | skill | `superpowers:verification-before-completion` | `5.0.7` | `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c` | `https://github.com/obra/superpowers/tree/e7a2d16476bf042e9add4699c9d018a90f86e4a6/skills/verification-before-completion/SKILL.md`<br>`skills/references/superpowers/verification-before-completion/SKILL.md` |
| R11 | resource | `tect:superpowers-v6-native-boundary` | `0.4.0-native.skills.1` | `13012dd0ec17ae94125d0cd3ce78baca13302a728a066ba3bd0e5d4fc73c302f` | `skills/pipelines/shared/superpowers-v6-native-boundary.md` |
| R12 | instruction | `tect:inquiry-boundary` | `0.5.0-native.inquiry.1` | `09de122d23367442c85624c1ae7aa9c2b823915691ad0751c46706afcb31547e` | `skills/pipelines/shared/inquiry-boundary.md` |
| R12 | skill | `tect:research-result` | `0.5.0-native.inquiry.1` | `c7056dc245ffab7f618342cd30a44162bbf681288d9fb2821d49e083754d12c2` | `skills/pipelines/research/methods/research-result.md` |
