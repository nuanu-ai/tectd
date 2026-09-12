use crate::responses;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use tect_application::{CandidateGuidance, CandidateOutputGuard};
use tect_domain::{
    CandidateMethodSnapshot, CandidateRuleSnapshot, CandidateSnapshotMaterial, Error, Program,
    ResolvedCandidateDraft, Result, WorktreeSummary,
};

pub(crate) const METHOD_ID: &str = "tectd-scope-candidates";
pub(crate) const METHOD_REVISION: &str = "2";
pub(crate) const METHOD_BODY: &str =
    include_str!("../../../skills/tectd-scope-candidates/SKILL.md");
const REGISTRY_REVISION: &str = "1";
const RULE_SOURCE: &str = "plugins/tect/capabilities/registry/general-agent-work-rules-v1.json@ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8";

#[derive(Clone)]
struct RuleRecord {
    id: &'static str,
    revision: &'static str,
    text: &'static str,
    origins: &'static [&'static str],
}

#[derive(Clone)]
struct BindingRecord {
    id: &'static str,
    revision: &'static str,
    all: &'static [Predicate],
    rules: &'static [(&'static str, &'static str)],
}

#[derive(Clone, Copy)]
enum Predicate {
    Purpose(&'static str),
    Phase(&'static str),
    State(&'static str),
}

struct RuleContext<'a> {
    purpose: &'a str,
    phases: &'a [&'a str],
    states: &'a [&'a str],
}

const RULES: [RuleRecord; 4] = [
    RuleRecord {
        id: "vertical-provable-slices",
        revision: "1",
        text: "Each candidate must describe one vertical, demonstrable product result with its trigger, delivered behavior, boundaries, dependencies, and observable proof. Split work when those results cannot be reviewed and demonstrated independently.",
        origins: &[
            "plugins/tect/capabilities/registry/general-agent-work-rules-v1.json@ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8#vertical-provable-slices",
        ],
    },
    RuleRecord {
        id: "no-unrequested-or-unauthorized-work",
        revision: "1",
        text: "Keep candidates within the captured Program and current planning-request window. Do not include work that the user did not request or authorize, and preserve consequential authorization boundaries as explicit blockers or questions.",
        origins: &[
            "plugins/tect/capabilities/registry/general-agent-work-rules-v1.json@ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8#no-unrequested-or-unauthorized-work",
        ],
    },
    RuleRecord {
        id: "autonomous-local-technical-decisions",
        revision: "1",
        text: "Resolve ordinary local technical choices from the captured context and source evidence. Ask the user only when a consequential ambiguity changes product behavior, authority, safety, cost, or an irreversible external effect.",
        origins: &[
            "plugins/tect/capabilities/registry/general-agent-work-rules-v1.json@ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8#autonomous-local-technical-decisions",
        ],
    },
    RuleRecord {
        id: "no-product-test-harness-work",
        revision: "1",
        text: "Plan verification through existing product tests and direct native MCP acceptance. Do not create a separate product test harness or count harness construction as delivered product behavior.",
        origins: &[
            "plugins/tect/capabilities/registry/general-agent-work-rules-v1.json@ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8#no-product-test-harness-work",
        ],
    },
];

const BASE: [Predicate; 1] = [Predicate::Purpose("scope_candidates.design")];
const TECHNICAL: [Predicate; 2] = [
    Predicate::Purpose("scope_candidates.design"),
    Predicate::Phase("technical_decisions"),
];
const PROOF: [Predicate; 2] = [
    Predicate::Purpose("scope_candidates.design"),
    Predicate::Phase("proof_planning"),
];
const REVIEW: [Predicate; 2] = [
    Predicate::Purpose("scope_candidates.design"),
    Predicate::State("review"),
];
const BINDINGS: [BindingRecord; 4] = [
    BindingRecord {
        id: "scope-candidate-base",
        revision: "1",
        all: &BASE,
        rules: &[
            ("vertical-provable-slices", "1"),
            ("no-unrequested-or-unauthorized-work", "1"),
        ],
    },
    BindingRecord {
        id: "scope-candidate-technical",
        revision: "1",
        all: &TECHNICAL,
        rules: &[("autonomous-local-technical-decisions", "1")],
    },
    BindingRecord {
        id: "scope-candidate-proof",
        revision: "1",
        all: &PROOF,
        rules: &[("no-product-test-harness-work", "1")],
    },
    BindingRecord {
        id: "scope-candidate-review",
        revision: "1",
        all: &REVIEW,
        rules: &[("vertical-provable-slices", "1")],
    },
];

pub(crate) struct StaticCandidateGuidance;

impl CandidateGuidance for StaticCandidateGuidance {
    fn snapshot(
        &self,
        program: Program,
        mut selected_worktrees: Vec<WorktreeSummary>,
    ) -> Result<CandidateSnapshotMaterial> {
        selected_worktrees.sort_by_key(|worktree| worktree.id);
        let selected_sources_digest = digest(
            &serde_json::to_vec(
                &selected_worktrees
                    .iter()
                    .map(|worktree| (worktree.repository_id, worktree.id))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| Error::InternalInvariant)?,
        );
        let method = CandidateMethodSnapshot {
            id: METHOD_ID.into(),
            revision: METHOD_REVISION.into(),
            digest: digest(METHOD_BODY.as_bytes()),
            body: METHOD_BODY.into(),
            origin_refs: vec![format!(
                "skills/tectd-scope-candidates/SKILL.md@{METHOD_REVISION}"
            )],
        };
        let rules = resolve_rules(
            &RULES,
            &BINDINGS,
            &RuleContext {
                purpose: "scope_candidates.design",
                phases: &["technical_decisions", "proof_planning"],
                states: &["review"],
            },
        )?;
        Ok(CandidateSnapshotMaterial {
            program,
            selected_worktrees,
            selected_sources_digest,
            method,
            registry_revision: REGISTRY_REVISION.into(),
            registry_digest: registry_digest()?,
            rules,
        })
    }
}

pub(crate) fn help_registry() -> Result<Value> {
    Ok(json!({
        "revision":REGISTRY_REVISION,"source":RULE_SOURCE,
        "digest":registry_digest()?,
        "rules":RULES.iter().map(|rule| json!({
            "id":rule.id,"revision":rule.revision,"origin_refs":rule.origins
        })).collect::<Vec<_>>(),
        "bindings":BINDINGS.iter().map(|binding| json!({
            "id":binding.id,"revision":binding.revision,
            "all":binding.all.iter().map(predicate_name).collect::<Vec<_>>(),
            "rules":binding.rules.iter().map(|(id,revision)| json!({"id":id,"revision":revision})).collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
        "applicability":{
            "purpose":"scope_candidates.design",
            "phases":["technical_decisions","proof_planning"],
            "states":["review"],
            "semantics":"Every predicate in one binding must match; matching bindings add rules; duplicate rule identities are emitted once in registry order."
        }
    }))
}

fn resolve_rules(
    rules: &[RuleRecord],
    bindings: &[BindingRecord],
    context: &RuleContext<'_>,
) -> Result<Vec<CandidateRuleSnapshot>> {
    validate_registry(rules, bindings)?;
    let mut selected = BTreeMap::<(&str, &str), BTreeSet<String>>::new();
    for binding in bindings
        .iter()
        .filter(|binding| binding.all.iter().all(|term| term.matches(context)))
    {
        for reference in binding.rules {
            selected
                .entry(*reference)
                .or_default()
                .insert(format!("{}@{}", binding.id, binding.revision));
        }
    }
    Ok(rules
        .iter()
        .filter_map(|rule| {
            selected
                .remove(&(rule.id, rule.revision))
                .map(|applicability| CandidateRuleSnapshot {
                    id: rule.id.into(),
                    revision: rule.revision.into(),
                    text: rule.text.into(),
                    origin_refs: rule.origins.iter().map(|value| (*value).into()).collect(),
                    applicability: applicability.into_iter().collect(),
                })
        })
        .collect())
}

fn validate_registry(rules: &[RuleRecord], bindings: &[BindingRecord]) -> Result<()> {
    let mut catalog = BTreeMap::new();
    for rule in rules {
        let key = (rule.id, rule.revision);
        if let Some(existing) = catalog.insert(key, rule)
            && (existing.text != rule.text || existing.origins != rule.origins)
        {
            return Err(Error::InternalInvariant);
        }
    }
    let mut binding_catalog = BTreeMap::<(&str, &str), (Vec<String>, Vec<(&str, &str)>)>::new();
    for binding in bindings {
        for reference in binding.rules {
            if !catalog.contains_key(reference) {
                return Err(Error::InternalInvariant);
            }
        }
        let signature = (
            binding.all.iter().map(predicate_name).collect::<Vec<_>>(),
            binding.rules.to_vec(),
        );
        if let Some(existing) =
            binding_catalog.insert((binding.id, binding.revision), signature.clone())
            && existing != signature
        {
            return Err(Error::InternalInvariant);
        }
    }
    Ok(())
}

impl Predicate {
    fn matches(self, context: &RuleContext<'_>) -> bool {
        match self {
            Self::Purpose(value) => context.purpose == value,
            Self::Phase(value) => context.phases.contains(&value),
            Self::State(value) => context.states.contains(&value),
        }
    }
}

fn predicate_name(predicate: &Predicate) -> String {
    match predicate {
        Predicate::Purpose(value) => format!("purpose={value}"),
        Predicate::Phase(value) => format!("phase={value}"),
        Predicate::State(value) => format!("state={value}"),
    }
}

fn registry_digest() -> Result<String> {
    validate_registry(&RULES, &BINDINGS)?;
    let rules = RULES
        .iter()
        .map(|rule| (rule.id, rule.revision, rule.text, rule.origins))
        .collect::<Vec<_>>();
    let bindings = BINDINGS
        .iter()
        .map(|binding| {
            (
                binding.id,
                binding.revision,
                binding.all.iter().map(predicate_name).collect::<Vec<_>>(),
                binding.rules,
            )
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&(REGISTRY_REVISION, rules, bindings))
        .map(|bytes| digest(&bytes))
        .map_err(|_| Error::InternalInvariant)
}

pub struct CandidateEncoding {
    pub capacity: usize,
}

impl CandidateOutputGuard for CandidateEncoding {
    fn input_bytes(&self, input: &str) -> Result<i64> {
        let once = serde_json::to_string(input).map_err(|_| Error::TransportUnavailable)?;
        let twice = serde_json::to_string(&once).map_err(|_| Error::TransportUnavailable)?;
        let empty = serde_json::to_string(&"\"\"").expect("empty JSON string");
        i64::try_from(twice.len() - empty.len()).map_err(|_| Error::RequestTooLarge)
    }

    fn check_material(&self, material: &CandidateSnapshotMaterial) -> Result<()> {
        self.check_value(json!({
            "program":material.program,
            "selected_worktrees":material.selected_worktrees,
            "method":material.method,
            "rules":material.rules,
            "registry_revision":material.registry_revision,
            "registry_digest":material.registry_digest
        }))
    }

    fn check_draft(&self, draft: &ResolvedCandidateDraft) -> Result<()> {
        self.check_value(json!({"draft":draft}))
    }

    fn check_stored(&self, stored: &tect_domain::StoredCandidateContext) -> Result<()> {
        crate::scope_candidate_output::stored(stored.clone(), self.capacity).map(|_| ())
    }

    fn check_begin(&self, outcome: &tect_domain::BeginCandidateSetOutcome) -> Result<()> {
        crate::scope_candidate_output::begin(outcome.clone(), self.capacity).map(|_| ())
    }
}

impl CandidateEncoding {
    fn check_value<T: Serialize>(&self, value: T) -> Result<()> {
        if self.capacity > crate::frame::MAX_FRAME_BYTES {
            return Err(Error::InvalidArguments);
        }
        let value = responses::with_actions(
            serde_json::to_value(value).map_err(|_| Error::InternalInvariant)?,
            Vec::new(),
            None,
        );
        if responses::encoded_len(&value)? <= self.capacity {
            Ok(())
        } else {
            Err(Error::RequestTooLarge)
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = Sha256::digest(bytes);
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 0x0f) as usize] as char);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_are_conjunctive_additive_stable_and_fail_closed() {
        let unrelated = RuleContext {
            purpose: "program.compose",
            phases: &["proof_planning"],
            states: &["review"],
        };
        assert!(
            resolve_rules(&RULES, &BINDINGS, &unrelated)
                .unwrap()
                .is_empty()
        );
        let design = RuleContext {
            purpose: "scope_candidates.design",
            phases: &["technical_decisions", "proof_planning"],
            states: &["review"],
        };
        let resolved = resolve_rules(&RULES, &BINDINGS, &design).unwrap();
        assert_eq!(resolved.len(), 4);
        assert_eq!(resolved[0].id, "vertical-provable-slices");
        assert_eq!(resolved[0].applicability.len(), 2);

        let dangling = [BindingRecord {
            id: "bad",
            revision: "1",
            all: &[Predicate::Purpose("unmatched")],
            rules: &[("missing", "1")],
        }];
        assert_eq!(
            resolve_rules(&RULES, &dangling, &design),
            Err(Error::InternalInvariant)
        );
        let mut conflicting = RULES.to_vec();
        let mut duplicate = RULES[0].clone();
        duplicate.text = "different";
        conflicting.push(duplicate);
        assert_eq!(
            resolve_rules(&conflicting, &BINDINGS, &design),
            Err(Error::InternalInvariant)
        );
        let conflicting_bindings = [
            BINDINGS[0].clone(),
            BindingRecord {
                id: BINDINGS[0].id,
                revision: BINDINGS[0].revision,
                all: &TECHNICAL,
                rules: BINDINGS[0].rules,
            },
        ];
        assert_eq!(
            resolve_rules(&RULES, &conflicting_bindings, &design),
            Err(Error::InternalInvariant)
        );
    }
}
