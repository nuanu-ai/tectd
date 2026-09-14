# DevOps knowledge: assets, topology, custody and observations

Apply General and this method to infrastructure knowledge. Existing files, an inventory or a configuration declaration do not prove deployed state or health. Use stable asset/environment identities; distinguish intended configuration, observed configuration, deployment evidence and dated health observations.

Evidence: identify each asset/environment, its owner, topology/dependencies, deployment surface and configuration custody. Pin configuration references and their meaning without copying secrets. State observation time, source, tested scope and limitations. Missing telemetry, an unavailable host and a failed health check are different facts. Preserve migration history relevant to identity; do not merge environments because names resemble each other.

Create: describe the bounded infrastructure object and relationships, ownership, configuration references/custody and available dated observations. Separate desired topology from observed topology. Include operational boundaries and unresolved ownership or state gaps. A current health statement needs current scoped evidence; otherwise store the bounded historical observation. Do not run a change to make documentation agree with reality.

Revise: identify actual asset/configuration/topology/ownership changes versus corrected documentation. Recheck affected dependencies and invalidate stale observations or recipes. Preserve previous identities and provenance where continuity is established; replacements get explicit relationships. Updating the inventory is not evidence of a successful deployment or migration.

Revalidate: obtain new suitable observations for the unchanged assertion in the exact environment. Recheck ownership, configuration custody, dependencies and declared validity. New observation time alone cannot replace the underlying evidence. Changed configuration/topology meaning requires revise. A permission or reachability gap remains explicit; no unrequested host operations are authorized by this method.

Supersede: compare successor asset/environment scope, topology/dependency roles, ownership, configuration custody and operational fit. Enumerate affected bindings and migration obligations. A replacement declaration is not proof of cutover, traffic movement, data migration or old-resource removal. Preserve these as separate evidence dimensions.

Retract: withdraw unreliable inventory/state assertions from current delivery and mark dependent operations as needing context. Retain history and explain the scope. Withdrawal does not shut down infrastructure, revert a deployment or remove a resource.

Erase: enumerate owned asset notes, configuration snapshots, logs/observations, source fragments and derived topology copies. Never include secret values in the erasure request or receipt. Preserve unrelated resources and independently owned operational evidence; unknown shared ownership blocks that deletion. Resource decommissioning, credential rotation and retention disposal require their own authorized operational actions.

Impact and terminal checks: reconcile topology links, runbooks, incident/issue follow-ups, ownership and consumers. Return declared-versus-observed state and evidence time in delivered knowledge. Never collapse source, deployment, migration, health and acceptance into a single ready flag. Automatic scheduling/collection is outside this publication method.

Source: design §7.7, §7.12, §7.16; Tect V1 devops-infra family at ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8.
