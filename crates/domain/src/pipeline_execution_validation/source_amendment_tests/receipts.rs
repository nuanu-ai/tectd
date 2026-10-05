use super::*;

#[test]
fn receipt_diagnostics_report_complete_differences_and_duplicate_counts() {
    let expected = [("a", "1", "da"), ("b", "1", "db")];
    let actual = [("a", "2", "wrong"), ("a", "2", "wrong"), ("z", "1", "dz")];
    let diff = FullReceiptDiff::between(&expected, &actual);
    assert_eq!(diff.missing.len(), 2);
    assert_eq!(diff.unexpected.len(), 2);
    assert_eq!(diff.duplicates[0].repeat_count, 2);
    let refusal = phase_read_receipt_refusal(
        "skill",
        "WP6-SKILL-READ-01",
        "arguments.params.output.skill_reads",
        &diff,
    )
    .unwrap()
    .refusal()
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(refusal.actual.as_ref().unwrap()).unwrap();
    assert_eq!(actual["submitted_count"], 3);
    assert_eq!(actual["submitted_unique_count"], 2);
    let labels = (0..20).map(|i| format!("Ж🙂{i}")).collect::<Vec<_>>();
    let tuples = labels
        .iter()
        .map(|id| (id.as_str(), "1", "digest"))
        .collect::<Vec<_>>();
    let diff = FullReceiptDiff::between(&tuples, &[]);
    let refusal = phase_read_receipt_refusal(
        "skill",
        "WP6-SKILL-READ-01",
        "arguments.params.output.skill_reads",
        &diff,
    )
    .unwrap()
    .refusal()
    .unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(refusal.expected.as_ref().unwrap()).unwrap();
    assert_eq!(expected["missing"].as_array().unwrap().len(), 20);
    assert!(refusal.expected.unwrap().len() > 240);
}

#[test]
fn completion_disposition_and_reviewer_aggregate_branches_have_named_refusals() {
    for (case, rule) in [
        (0, "WP6-COMPLETE-OUTPUT-05"),
        (1, "WP6-COMPLETE-OUTPUT-06"),
        (2, "WP6-COMPLETE-OUTPUT-07"),
        (3, "WP6-COMPLETE-OUTPUT-08"),
        (4, "WP6-COMPLETE-OUTPUT-09"),
        (5, "WP6-COMPLETE-OUTPUT-10"),
        (6, "WP6-REVIEW-CONTEXT-07"),
    ] {
        let mut definition = proof_test_definition("0.6.0");
        let phase = &mut definition.phases[0];
        let mut request = proof_test_completion();
        match case {
            0 => phase.allowed_verdicts = vec!["allowed".into()],
            1 => phase.required_dispositions = vec!["required".into()],
            2 => phase.disposition_required = true,
            3 => {
                phase.allowed_dispositions = vec!["allowed".into()];
                request.output.dispositions = vec!["other".into()];
            }
            4 => {
                phase.allowed_dispositions = vec!["a".into(), "b".into()];
                phase.disposition_required = true;
                request.output.dispositions = vec!["a".into(), "b".into()];
            }
            5 => request.output.dispositions = vec!["a".into(), "a".into()],
            6 => phase.fresh_reviewer_input = true,
            _ => unreachable!(),
        }
        let refusal = request
            .validate(&definition)
            .unwrap_err()
            .refusal()
            .unwrap();
        assert_eq!(refusal.rule.as_deref(), Some(rule));
        assert_eq!(refusal.code, RefusalCode::InvalidOutput);
        assert!(refusal.actual.is_some());
    }
}

#[test]
fn completion_receipt_all_differences_and_duplicates_survive_named_error_serialization() {
    let mut definition = proof_test_definition("0.6.0");
    for (id, digest) in [("a", "da"), ("b", "db")] {
        let mut skill = definition.overview.clone();
        skill.id = id.into();
        skill.version = "1".into();
        skill.digest = digest.into();
        definition.phases[0].skills.push(skill);
    }
    let mut request = proof_test_completion();
    request.output.skill_reads = vec![
        PipelineSkillReadReceipt {
            instruction_id: "a".into(),
            version: "2".into(),
            digest: "wrong".into()
        };
        2
    ];
    let error = request.validate(&definition).unwrap_err();
    let refusal = error.refusal().unwrap();
    assert_eq!(refusal.rule.as_deref(), Some("WP6-SKILL-READ-01"));
    let expected: serde_json::Value =
        serde_json::from_str(refusal.expected.as_ref().unwrap()).unwrap();
    assert_eq!(expected["missing"].as_array().unwrap().len(), 2);
    assert_eq!(expected["missing"][0]["instruction_id"], "a");
    assert_eq!(expected["missing"][1]["instruction_id"], "b");
    let actual: serde_json::Value = serde_json::from_str(refusal.actual.as_ref().unwrap()).unwrap();
    assert_eq!(actual["submitted_count"], 2);
    assert_eq!(actual["submitted_unique_count"], 1);
    assert_eq!(actual["unexpected"].as_array().unwrap().len(), 1);
    assert_eq!(actual["duplicates"][0]["repeat_count"], 2);
    assert_eq!(
        serde_json::from_slice::<Refusal>(&serde_json::to_vec(&refusal).unwrap()).unwrap(),
        refusal
    );
}

#[test]
fn receipt_skill_priority_resource_second_and_original_positive_acceptance_are_preserved() {
    let mut definition = proof_test_definition("0.6.0");
    let mut skill = definition.overview.clone();
    skill.id = "skill@界".into();
    let mut resource = skill.clone();
    resource.id = "resource@🙂".into();
    definition.phases[0].skills = vec![skill.clone()];
    definition.phases[0].resources = vec![resource.clone()];
    let mut request = proof_test_completion();
    assert_eq!(
        request
            .validate(&definition)
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("WP6-SKILL-READ-01")
    );
    request.output.skill_reads = vec![PipelineSkillReadReceipt {
        instruction_id: skill.id,
        version: skill.version,
        digest: skill.digest,
    }];
    assert_eq!(
        request
            .validate(&definition)
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("WP6-RESOURCE-READ-01")
    );
    request.output.resource_reads = vec![PipelineSkillReadReceipt {
        instruction_id: resource.id,
        version: resource.version,
        digest: resource.digest,
    }];
    request.validate(&definition).unwrap();
    request
        .output
        .skill_reads
        .push(request.output.skill_reads[0].clone());
    let refusal = request
        .validate(&definition)
        .unwrap_err()
        .refusal()
        .unwrap();
    assert_eq!(refusal.rule.as_deref(), Some("WP6-SKILL-READ-01"));
    let actual: serde_json::Value = serde_json::from_str(refusal.actual.as_ref().unwrap()).unwrap();
    assert_eq!(actual["duplicates"][0]["repeat_count"], 2);
    assert!(actual["unexpected"].as_array().unwrap().is_empty());
}
