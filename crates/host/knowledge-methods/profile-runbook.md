# Runbook knowledge: fit, ordered procedure and proof

Apply General and this method to procedures/runbooks. Publishing instructions does not execute them, authorize them or prove that they work. Preserve the distinction between documented, statically verified and runtime verified. Use existing authorized evidence; a publication gap does not authorize a deployment, incident experiment or new audit harness.

Evidence: identify purpose, eligible target environments, version/context, non-use conditions, operator prerequisites and authority. Every asserted runtime result needs a dated, scoped source describing the actual action and observable result. Static checks support only their stated static claims. Record unknown fit explicitly. Parameters name values to supply, their constraints and custody/reference locations; they must not contain credentials, private keys or secret values.

Create: specify prerequisites and preflight; ordered actions with expected results and verification; stop conditions; failure diagnosis and recovery/rollback boundaries; final proof and handoff. Steps must be usable in the declared environment and preserve dependency order. Commands are parameterized where local values vary. Classify read/write/execute/deploy/destructive or sensitive effects so existing authority can be evaluated. Recognize authority already granted; do not invent a repeated approval ceremony. A documented unverified procedure may be retained as such, but must not be delivered as an approved runtime-verified recipe.

Revise: show which step, parameter, dependency, target, authority, stop condition or recovery path changed and why. Re-evaluate downstream steps and proof invalidated by that change. Carry forward only compatible evidence; successful execution of the old version does not establish the new version. Preserve prior procedure versions for scoped history until legitimately erased.

Revalidate: obtain new suitable evidence for the unchanged procedure in the declared environment. Recheck versions, parameters, access assumptions, dependencies and proof freshness. If only the document was reread, do not claim runtime revalidation. If the required evidence cannot be obtained under current authority, leave the required fit unresolved or retain its explicitly weaker documented status. Changed steps or operational meaning require revise.

Supersede: compare successor purpose/eligibility, parameters, expected results, authority, failure/recovery and proof level. Enumerate the bindings and environments the successor actually replaces. Preserve a transition path for consumers with different requirements. An alternative procedure is not automatically equivalent. Do not silently redirect a pinned operational execution.

Retract: identify unsafe, unsupported or obsolete use and prevent current procedure delivery for the affected scope. Preserve evidence explaining withdrawal. Record affected running/planned operations and an owner for their handling; a knowledge withdrawal does not stop or undo external operations by itself.

Erase: apply the General ownership inventory to command recipes, parameter examples, execution evidence, source fragments and derivative procedure copies. Preserve unrelated operational records outside the approved ownership scope. If sensitive values were accidentally recorded, enumerate the controlled copies without repeating the values in the plan or result. Erasure is not credential rotation; any required rotation is a separately owned operational action.

Impact and terminal checks: account for environment/dependency changes, stale proof, consumers and any proposed skill candidate. Capturing a reusable skill is a separate reviewed task; publication never installs it. Report the exact procedure revision and its actual proof level, plus unresolved execution requirements. Passing the publication lifecycle means the procedure is correctly represented and qualified, not that its commands ran.

Source: design §7.7, §7.12, §7.16; Tect V1 runbook-library family at ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8, including authority/risk, command recipe, proof order, provenance, secret safety and skill-candidate boundaries.
