#[path = "pipeline_execution/contract_support.rs"]
mod contract_support;

use contract_support::{definition, instruction, valid_completion};
use tect_domain::{
    PipelineArtifactDigestRef, PipelineArtifactRequirement, PipelinePhaseArtifactDraft,
    PipelineReviewerAttestation, PipelineSkillReadReceipt, PipelineValidatorContract,
    PipelineValidatorReceipt,
};

#[test]
fn typed_artifacts_require_exact_resource_receipts_media_json_digest_and_count() {
    let mut definition = definition();
    let phase = &mut definition.phases[0];
    phase.resources = vec![instruction("full-contract-schema", "resource-digest")];
    phase.required_artifacts = vec![PipelineArtifactRequirement {
        name_pattern: "contracts/*.json".into(),
        media_type: "application/json".into(),
        schema_ref: Some("v1:full-contract-schema#resource-digest".into()),
        schema_resource_id: Some("full-contract-schema".into()),
        schema_resource_digest: Some("resource-digest".into()),
        required: true,
        minimum_matches: 1,
        when_verdict: Some("implemented_locally".into()),
    }];
    phase.validator_contracts = vec![PipelineValidatorContract {
        resource_id: "full-contract-schema".into(),
        version: "v1".into(),
        digest: "resource-digest".into(),
        stage: "contract".into(),
        artifact_patterns: vec!["contracts/*.json".into()],
        required_verdicts: vec!["implemented_locally".into()],
        success_verdicts: vec!["implemented_locally".into()],
    }];
    let mut completion = valid_completion();
    completion.output.resource_reads = vec![PipelineSkillReadReceipt {
        instruction_id: "full-contract-schema".into(),
        version: "v1".into(),
        digest: "resource-digest".into(),
    }];
    completion.output.artifacts = vec![PipelinePhaseArtifactDraft {
        name: "contracts/implementation.json".into(),
        media_type: "application/json".into(),
        body: "{\"contract\":\"pinned\"}".into(),
        digest: "d1c16cd67ed96014c5cc1f9c60059bf44e58313b9afef141e745ff68d3ac4860".into(),
        reference: Some("artifact://contracts/implementation.json".into()),
    }];
    completion.output.validator_receipts = vec![PipelineValidatorReceipt {
        resource_id: "full-contract-schema".into(),
        version: "v1".into(),
        digest: "resource-digest".into(),
        stage: "contract".into(),
        command: "node validate-spec-pipeline.js fixture".into(),
        exit_code: 0,
        valid: true,
        artifacts: vec![PipelineArtifactDigestRef {
            name: "contracts/implementation.json".into(),
            digest: "d1c16cd67ed96014c5cc1f9c60059bf44e58313b9afef141e745ff68d3ac4860".into(),
        }],
    }];
    assert!(definition.validate().is_ok());
    assert!(completion.validate(&definition).is_ok());

    let mut missing_resource = completion.clone();
    missing_resource.output.resource_reads.clear();
    assert!(missing_resource.validate(&definition).is_err());

    let mut missing_artifact = completion.clone();
    missing_artifact.output.artifacts.clear();
    assert!(missing_artifact.validate(&definition).is_err());

    let mut missing_validator = completion.clone();
    missing_validator.output.validator_receipts.clear();
    assert!(missing_validator.validate(&definition).is_err());

    let mut failed_validator = completion.clone();
    failed_validator.output.validator_receipts[0].exit_code = 1;
    failed_validator.output.validator_receipts[0].valid = false;
    assert!(failed_validator.validate(&definition).is_err());

    let mut mismatched_validator_artifact = completion.clone();
    mismatched_validator_artifact.output.validator_receipts[0].artifacts[0].digest = "wrong".into();
    assert!(mismatched_validator_artifact.validate(&definition).is_err());

    let mut wrong_media = completion.clone();
    wrong_media.output.artifacts[0].media_type = "text/markdown".into();
    assert!(wrong_media.validate(&definition).is_err());
}

#[test]
fn fresh_review_requires_structured_distinct_reported_contexts() {
    let mut definition = definition();
    definition.phases[0].fresh_reviewer_input = true;
    let mut completion = valid_completion();
    assert!(completion.validate(&definition).is_err());

    let producer_context_id = "producer-session-a".to_owned();
    let reviewer_context_id = "review-session-b".to_owned();
    completion.output.producer_context_id = reviewer_context_id.clone();
    completion.output.reviewer_context = Some(PipelineReviewerAttestation {
        reviewer_identity: "reported-reviewer".into(),
        reviewer_context_id: reviewer_context_id.clone(),
        producer_context_ids: vec![producer_context_id.clone()],
        fresh_input: true,
    });
    assert!(completion.validate(&definition).is_ok());

    completion
        .output
        .reviewer_context
        .as_mut()
        .unwrap()
        .producer_context_ids = vec![reviewer_context_id];
    assert!(completion.validate(&definition).is_err());
    completion
        .output
        .reviewer_context
        .as_mut()
        .unwrap()
        .producer_context_ids = vec![producer_context_id];
    completion
        .output
        .reviewer_context
        .as_mut()
        .unwrap()
        .fresh_input = false;
    assert!(completion.validate(&definition).is_err());
}
