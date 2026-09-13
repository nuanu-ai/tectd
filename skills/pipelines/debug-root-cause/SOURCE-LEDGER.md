# Debug Root-Cause source ledger

## Identity and coverage

- Native kind/version: `slice.debug-root-cause@0.1.0-native.1`.
- V1 source revision: `ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8`.
- V1 manifest: `capabilities/pipelines/slice-variants/debug-root-cause.pipeline.json`, SHA-256 `1edbef064f62246ae6fa08837631baad9a68ea5c15b7243aa27fb0e3d41725ff`.
- Ordered 18-body source-set digest: `84778ad788bea8cb9ea397f513cd333bdaf049a13f962508a26091b1029300de`.
- Native definition: `crates/host/pipeline-definitions/debug-root-cause.json`, semantic digest `4bdb3a67d910784007d0dd9c52f6112dddc24c7c317e512a6ec6bd441f347457`, physical SHA-256 `d258b543f16637eec9192e7600052bff7fdc062e17a234211a3b77ea868a8010`.
- Accepted V2 delivery setting: default `whole`; allowed `whole` and `phasewise`. V1 did not define this transport setting.
- Native coverage: 18/18 phases, 18 exact internal bodies, 59 verdict routes, 24 artifact requirements, 11 phase skill snapshots representing seven unique skill bodies, and 14 resource-read snapshots. Unresolved source bodies: zero.

## Exact V1 assets

Every file under `v1/` is byte-exact provenance from the V1 revision. The phase `output_contract` is a versioned native adapter; it does not alter these bodies.

| Phase | Body SHA-256 |
|---:|---|
| 1 `slice-debug-entry-gate` | `81f08de58dd341b4646db7ea2ddf7d957e9062ea0fec2dd3de5abaa70d74bf8f` |
| 2 `slice-debug-context-loader` | `d3d9bbf9e58a3f861c086039c2a38129abe4b942639db8411998ce08253c9774` |
| 3 `slice-debug-contract-writer` | `413e28304a1a4e250374955beab2cbb1e9562b5e3c97f6ff09cd59f6c6439777` |
| 4 `slice-symptom-capture` | `1f6508719415792b0d46e0a046cdb1219adf661515236db61a724e49808990fd` |
| 5 `slice-reproduction-builder` | `fc38e047ab5edf4594de3df317a67e0a5ff5fcef104b3460128e2321a7187e40` |
| 6 `slice-evidence-order-planner` | `495ff8949c2b7277864fa6d84c12ea85c1ddebc19c80e540fec66ebce326a882` |
| 7 `slice-recent-change-inspector` | `5b0f2af2a7bf1365b1522d0a86d9209fcf2148a224d63afdd0aa9323e41f9b7e` |
| 8 `slice-working-example-comparator` | `861cecf10d16cf338e55db979095d4eda43f4644cdd639422076ef7181f3aba0` |
| 9 `slice-data-flow-tracer` | `a5f008931a151b7b66be44216c06a01c4adde18808df3d97c3dc914ad996c97e` |
| 10 `slice-hypothesis-ledger` | `224354db6b53522600122b259d1b7959c51b0006113f388c2ca4a700850e6737` |
| 11 `slice-root-cause-decision` | `4ae769c6770525b5d76ec6a1244373cdae0205e0a24387399d975dd2752fc0c3` |
| 12 `slice-debug-fix-strategy` | `7fb3a51156c7145496e03229b098567412a80d35e9f55f6d8cb5737022b6610d` |
| 13 `slice-regression-test-writer` | `ca47a6f0473bc6bfcc7c20b287d2c339d9af70dcbd03dc701ee42a74f95fbb69` |
| 14 `slice-debug-fix-runner` | `4c24a5ad5b7c6016008dccd6433e797a70e07482bf78774f0f7e57c2650525f6` |
| 15 `slice-debug-verification-runner` | `6cb0d0ceb5353854f0b86641b839589df435fdddc916bfb785a042f836eab6f5` |
| 16 `slice-debug-result-writer` | `f5060ad0f8a02c20dab71454a087784559d01a2cb13bf8f8184f5a421adf4c49` |
| 17 `slice-debug-promotion-router` | `7a23137036ad1cc1955bee8e8bffa605eaf903d37525516136aa9415dc9da3e7` |
| 18 `slice-debug-handoff-builder` | `40a66b225cab3fce677e93c501cf301f0f7d68ad655afb19569db38a73daeb38` |

Phase 9 also pins `v1/debug-investigation.subagent.json` SHA-256 `b487578c4ac1933afbc217d1dc31ac00c276893aa6f5fc6e9a5f25256d96c865`. It remains proposal-only: no parallel dispatch, source mutation, completion, or approval. The controller must review sequential fan-in.

## Referenced skills

Exact Superpowers `5.0.7` snapshots come from `obra/superpowers@e7a2d16476bf042e9add4699c9d018a90f86e4a6`: `writing-plans` `90056bad3d5f196fa7c9fec0ffe592e6d9c86bc983e406642a51d1a4198b7024`; `systematic-debugging` `4999cb851360485eca5074e727bbdd62ef20549c5d5b01216fcbf5831badb473`; `test-driven-development` `7dee67b4af6bdccc7a914ca34533184d64592d0f5b23aeae631538168db14994`; `executing-plans` `a711f83fb762e2ea0fa151f598893da9911a408895c91cc7a7e0770dd59a27b3`; `using-git-worktrees` `dcd1a83a2488bd557ceb7f14f2b6384ec209f551d18752dd9ceb70b9089dfb3b`; `finishing-a-development-branch` `dd2f82c6dc8582b621f9eb57fcb65f557f88eadf872727ac81d0840ae12c504e`; and `verification-before-completion` `ea52d15aabaf72bc6b558efe2c126f161b53961090ddcd712000273bfe8c7b6c`.

The systematic-debugging phase reads bind its exact conditional companion resources: `root-cause-tracing.md` `e2a58a6c36d2381c12514b45922711746cf2352a03c896889ff934ae15bbb08f`, `defense-in-depth.md` `1e175fb86fc357e58c6aebf5441e481e1b7868b4380c0456b63a17eefbd18ba7`, and `condition-based-waiting.md` `e89fec8400d6cd50f43407cec9fab50976ba4d55d0ec2eb51c0bd68036b54c26`. Phase 13 binds TDD's mandatory conditional anti-pattern resource `bde453bc258f06543987477c837939afaa774ea2acbd9f308d702fc452bc4283`. Phase 14 binds executing-plans and its mandatory worktree/finish skill closure even though the native adapter converts all execution commands into a future-Slice contract.

## Approved V2 semantic adaptation

V1 phases 13-15 permit a regression test write, fix attempt, and local fix verification inside Debug. The accepted V2 boundary ends Debug at diagnosis, cause-level fix strategy, exact regression proof target, and a typed follow-up handoff:

| V1 obligation | Native projection |
|---|---|
| Phase 12 chooses no-fix/minimal/TDD/full/operational/Hybrid/architecture lane and produces exactly one payload. | Still mandatory. Minimal/TDD chooses a separate Lightweight Slice; cross-component work chooses Full; operations choose Operational Preparation/Execution; Hybrid becomes linked implementation and operational Slices. |
| Phase 13 may create test-only RED and defines exact RED/GREEN contract. | `regression-target.md` retains root-cause binding, target path/command/test/setup/assertion, expected RED, invalid-RED conditions, GREEN expectation, affected proof, authority, fallback, risk, and owner. `test_source_written=false`; creation/execution moves to the implementation Slice. |
| Phase 14 performs or defers a bounded patch with attempt/checkpoint/limit/rollback/proof bookkeeping. | `patch.md` remains mandatory as the compatibility carrier and includes every source field, but records a proposed patch plus separate Slice identity. Native `implementation_slice_prepared` maps the V1 `fix_attempt_recorded` branch without claiming mutation. Three failed attempts route to architecture/Full. |
| Phase 15 verifies reproduction/no-repro truth and regression/fix proof. | Freshly verifies the diagnostic evidence path and regression contract. `implementation_fix_verified=false`; GREEN and fix proof remain due in the linked implementation Slice. The Debug result cannot say fixed or `completed_local_verified`. |
| Phase 18 may end `completed_local_verified`. | Native completion is `implementation_handoff_complete` or proof-backed `diagnosis_complete`; the source state is retained as `source_terminal_projection`. Actual local-fix completion can be claimed only by the linked Lightweight/Full Slice. |

The carrier obligation and semantic obligation both survive. Backend run state replaces duplicate Markdown FSM/lifecycle bookkeeping only; all symptom, reproduction, evidence, hypotheses, root cause, strategy, regression, patch-handoff, verification, Result, promotion/deferred, and handoff artifacts remain required by their routes.

Optional graph phases 7, 8, 9, 14, 17, and 18 remain progress-mandatory. Each has an explicit checked, not-applicable, unavailable, gap, deferred, no-promotion, handoff, or blocked disposition; omission cannot advance. Blocked/rework routes never receive a success marker. Rework is route-bound to exact earlier phase IDs. Every phase asserts `source_mutation_performed=false` under this native Debug boundary.

The backend validates body/skill/resource hashes, complete reads, typed fields, integer/boolean constraints, artifact name/media/body digests, route dispositions, input/output revisions, and exact revisit targets. The agent remains responsible for factual evidence, actual read-only commands, cause mechanism truth, authority, freshness, and future-Slice identity. No native validator is invented because this pipeline has no mandatory runnable external validator contract.
