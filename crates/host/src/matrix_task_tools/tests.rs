use super::*;

#[test]
fn verified_v2_cards_read_requires_exact_typed_digest_and_rejects_extra_fields() {
    let task_id = Uuid::new_v4();
    let params = json!({"task_id":task_id,"expected_task_revision":1,
        "operating_verification_digest":"a".repeat(64),"card_id":"EM02-SCOPE@0.1"});
    let MatrixTaskInvocation::VerifiedCards(read) =
        parse("get_verified_matrix_cards", params.clone()).unwrap()
    else {
        panic!("V2 cards read must have its own invocation");
    };
    assert_eq!(read.task_id, task_id);
    assert_eq!(read.card_id.as_deref(), Some("EM02-SCOPE@0.1"));
    for changed in [
        json!({"task_id":task_id,"expected_task_revision":1}),
        json!({"task_id":task_id,"expected_task_revision":0,
            "operating_verification_digest":"a".repeat(64)}),
        json!({"task_id":task_id,"expected_task_revision":1,
            "operating_verification_digest":"a".repeat(64),"detail":"full"}),
        json!({"task_id":task_id,"expected_task_revision":1,
            "operating_verification_digest":"bad"}),
        json!({"task_id":task_id,"expected_task_revision":1,
            "operating_verification_digest":"a".repeat(64),"card_id":null}),
    ] {
        assert!(parse("get_verified_matrix_cards", changed).is_err());
    }
}

fn example_params() -> Value {
    crate::api::help(
        crate::api::parse_help(json!({
            "mode": "describe",
            "tool": "command",
            "route": "task.source.record"
        }))
        .unwrap(),
    )
    .unwrap()["example"]["arguments"]["params"]
        .clone()
}

#[test]
fn bound_source_wire_requires_strict_locator_and_projects_immutable_binding() {
    let mut params = example_params();
    let program_id = Uuid::new_v4();
    params["requirements_locator"] = json!({"level":"program","program_id":program_id});
    let MatrixTaskInvocation::BoundRecord(request, locator) =
        parse("record_matrix_task", params.clone()).unwrap()
    else {
        panic!("bound record")
    };
    assert_eq!(locator, MatrixRequirementsLocator::Program { program_id });
    let binding = tect_application::MatrixTaskRequirementsBinding {
        locator: locator.clone(),
        snapshot_id: Uuid::new_v4(),
        semantic_digest: "a".repeat(64),
        authority_schema: tect_domain::MATRIX_REQUIREMENTS_SCHEMA.into(),
    };
    let output = source(MatrixTaskSource {
        revision: MatrixTaskRevision {
            task_id: request.task_id,
            revision: request.revision,
            request_id: request.request_id,
            input: request.input.clone(),
            input_digest: "b".repeat(64),
            choice_set: request.choice_set.clone(),
            choice_set_digest: None,
            recorded_by_principal_id: Uuid::new_v4(),
            recorded_by_session_id: Uuid::new_v4(),
        },
        requirements_binding: Some(binding.clone()),
    });
    assert_eq!(
        output["requirements_snapshot_id"],
        json!(binding.snapshot_id)
    );
    assert_eq!(
        output["requirements_semantic_digest"],
        binding.semantic_digest
    );
    assert_eq!(output["context_authority_schema"], binding.authority_schema);
    assert_eq!(output["requirements_locator"], locator.as_json());
    params["requirements_locator"] = Value::Null;
    assert!(parse("record_matrix_task", params.clone()).is_err());
    params["requirements_locator"] =
        json!({"level":"program","program_id":program_id,"actor_is_human":true});
    assert!(parse("record_matrix_task", params.clone()).is_err());
    params["requirements_locator"] = json!({"level":"program","program_id":Uuid::nil()});
    assert!(parse("record_matrix_task", params).is_err());
}

#[test]
fn source_text_uses_domain_utf8_byte_and_trim_rules() {
    for value in [" \t ".to_owned(), "🦀".repeat(65)] {
        let mut params = example_params();
        params["input"]["criticality"] =
            json!({"state":"known","value":value,"provenance":"source"});
        assert!(parse("record_matrix_task", params).is_err());
    }
    let mut params = example_params();
    params["input"]["criticality"] =
        json!({"state":"known","value":"🦀".repeat(64),"provenance":"source"});
    assert!(parse("record_matrix_task", params).is_ok());
    let mut params = example_params();
    params["input"]["mode"] = json!({"state":"unknown","provenance":" \t "});
    assert!(parse("record_matrix_task", params).is_err());
}

#[test]
fn source_budget_and_response_capacity_fail_before_write() {
    let params = example_params();
    let MatrixTaskInvocation::Record(request) =
        parse("record_matrix_task", params.clone()).unwrap()
    else {
        panic!("record")
    };
    let projected = source(MatrixTaskSource {
        revision: MatrixTaskRevision {
            task_id: request.task_id,
            revision: request.revision,
            request_id: request.request_id,
            input: request.input.clone(),
            input_digest: "0".repeat(64),
            choice_set: request.choice_set.clone(),
            choice_set_digest: request.choice_set.as_ref().map(|_| "0".repeat(64)),
            recorded_by_principal_id: Uuid::nil(),
            recorded_by_session_id: Uuid::nil(),
        },
        requirements_binding: None,
    });
    for field in [
        "requirements_snapshot_id",
        "requirements_semantic_digest",
        "context_authority_schema",
        "requirements_locator",
    ] {
        assert_eq!(projected.get(field), Some(&Value::Null));
    }
    let size =
        crate::responses::encoded_len(&crate::responses::with_actions(projected, Vec::new(), None))
            .unwrap();
    assert!(guard_record_output(&request, size).is_ok());
    assert!(matches!(
        guard_record_output(&request, size - 1),
        Err(Error::RequestTooLarge)
    ));

    let mut too_many = params.clone();
    too_many["input"]["envelope"]["operational_facts"] = json!({"state":"reported","entries":(0..=MAX_OPERATIONAL_FACTS).map(|i| json!({"name":format!("fact-{i}"),"fact":{"state":"absent"}})).collect::<Vec<_>>()});
    assert!(parse("record_matrix_task", too_many).is_err());

    let mut over_bytes = params;
    let control = "\u{0000}".repeat(256);
    over_bytes["input"]["envelope"]["operational_facts"] = json!({"state":"reported","entries":(0..MAX_OPERATIONAL_FACTS).map(|i| json!({"name":format!("fact-{i}"),"fact":{"state":"known","value":control,"provenance":"source"}})).collect::<Vec<_>>()});
    assert!(serde_json::to_vec(&over_bytes["input"]).unwrap().len() > MAX_MATRIX_INPUT_BYTES);
    assert!(parse("record_matrix_task", over_bytes).is_err());

    let mut within_budget = example_params();
    let control = "\u{0000}".repeat(256);
    within_budget["input"]["envelope"]["operational_facts"] = json!({"state":"reported","entries":(0..600).map(|i| json!({"name":format!("fact-{i}"),"fact":{"state":"known","value":control,"provenance":"source"}})).collect::<Vec<_>>()});
    assert!(serde_json::to_vec(&within_budget["input"]).unwrap().len() <= MAX_MATRIX_INPUT_BYTES);
    let MatrixTaskInvocation::Record(large_request) =
        parse("record_matrix_task", within_budget).unwrap()
    else {
        panic!("record")
    };
    assert!(guard_record_output(&large_request, crate::frame::MAX_FRAME_BYTES - 16 * 1024).is_ok());

    let mut combined = example_params();
    for candidate in combined["choice_set"]["candidates"].as_array_mut().unwrap() {
        candidate["approach"] = json!("a".repeat(4096));
    }
    let choice_bytes = serde_json::to_vec(&combined["choice_set"]).unwrap().len();
    let mut low = 1;
    let mut high = MAX_OPERATIONAL_FACTS;
    let mut found_boundary = false;
    while low <= high {
        let count = low + (high - low) / 2;
        combined["input"]["envelope"]["operational_facts"] = json!({"state":"reported","entries":(0..count).map(|i| json!({"name":format!("fact-{i}"),"fact":{"state":"known","value":"\u{0000}".repeat(256),"provenance":"source"}})).collect::<Vec<_>>()});
        let input_bytes = serde_json::to_vec(&combined["input"]).unwrap().len();
        if input_bytes > MAX_MATRIX_INPUT_BYTES {
            high = count - 1;
        } else if input_bytes + choice_bytes <= MAX_MATRIX_INPUT_BYTES {
            low = count + 1;
        } else {
            assert!(parse("record_matrix_task", combined).is_err());
            found_boundary = true;
            break;
        }
    }
    assert!(
        found_boundary,
        "combined input and choice-set boundary was not reached"
    );
}

#[test]
fn revision_projection_preserves_source_and_server_identity() {
    let example = example_params()["input"].clone();
    let input = serde_json::from_value(example.clone()).unwrap();
    let task_id = Uuid::new_v4();
    let request_id = Uuid::new_v4();
    let principal = Uuid::new_v4();
    let session = Uuid::new_v4();
    let output = revision(MatrixTaskRevision {
        task_id,
        revision: 2,
        request_id,
        input,
        input_digest: "digest".into(),
        choice_set: None,
        choice_set_digest: None,
        recorded_by_principal_id: principal,
        recorded_by_session_id: session,
    });
    assert_eq!(output["task_id"], json!(task_id));
    assert_eq!(output["revision"], 2);
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["recorded_by_principal_id"], json!(principal));
    assert_eq!(output["recorded_by_session_id"], json!(session));
    assert_eq!(output["input_digest"], "digest");
    assert_eq!(output["input"], example);
    assert!(output.get("choice_set").is_none());
    assert!(output.get("choice_set_digest").is_none());
}

#[test]
fn choice_set_accepts_zero_one_and_two_candidates_and_absence() {
    let example = example_params();
    for count in 0..=2 {
        let mut params = example.clone();
        params["choice_set"]["candidates"]
            .as_array_mut()
            .unwrap()
            .truncate(count);
        let MatrixTaskInvocation::Record(request) =
            parse("record_matrix_task", params.clone()).unwrap()
        else {
            panic!("record")
        };
        assert_eq!(request.choice_set.as_ref().unwrap().candidates.len(), count);
        let projected = source(MatrixTaskSource {
            revision: MatrixTaskRevision {
                task_id: request.task_id,
                revision: request.revision,
                request_id: request.request_id,
                input: request.input.clone(),
                input_digest: "0".repeat(64),
                choice_set: request.choice_set.clone(),
                choice_set_digest: Some("0".repeat(64)),
                recorded_by_principal_id: Uuid::nil(),
                recorded_by_session_id: Uuid::nil(),
            },
            requirements_binding: None,
        });
        assert_eq!(projected["choice_set"], params["choice_set"]);
        assert_eq!(projected["choice_set_digest"].as_str().unwrap().len(), 64);
        let size = crate::responses::encoded_len(&crate::responses::with_actions(
            projected,
            Vec::new(),
            None,
        ))
        .unwrap();
        assert!(guard_record_output(&request, size).is_ok());
        assert!(matches!(
            guard_record_output(&request, size - 1),
            Err(Error::RequestTooLarge)
        ));
    }
    let mut legacy = example;
    legacy.as_object_mut().unwrap().remove("choice_set");
    let MatrixTaskInvocation::Record(request) = parse("record_matrix_task", legacy).unwrap() else {
        panic!("record")
    };
    assert!(request.choice_set.is_none());
}

#[test]
fn choice_set_rejects_invalid_references_binding_and_nested_fields() {
    let example = example_params();
    for (path, value) in [
        ("task_id", json!(Uuid::new_v4().to_string())),
        ("task_revision", json!("2")),
        ("schema", json!("other")),
    ] {
        let mut params = example.clone();
        params["choice_set"][path] = value;
        assert!(
            parse("record_matrix_task", params).is_err(),
            "accepted {path}"
        );
    }
    let mut invalid_reference = example.clone();
    invalid_reference["choice_set"]["candidates"][0]["assumption_fact_ids"] =
        json!(["not.a.matrix.fact"]);
    assert!(parse("record_matrix_task", invalid_reference).is_err());
    let mut duplicate_id = example.clone();
    duplicate_id["choice_set"]["candidates"][1]["candidate_id"] = json!("approach-a");
    assert!(parse("record_matrix_task", duplicate_id).is_err());
    let mut unknown_set = example.clone();
    unknown_set["choice_set"]["unexpected"] = json!(true);
    assert!(parse("record_matrix_task", unknown_set).is_err());
    let mut unknown_candidate = example.clone();
    unknown_candidate["choice_set"]["candidates"][0]["unexpected"] = json!(true);
    assert!(parse("record_matrix_task", unknown_candidate).is_err());
    let mut explicit_null = example;
    explicit_null["choice_set"] = Value::Null;
    assert!(parse("record_matrix_task", explicit_null).is_err());
}

#[test]
fn changed_choice_set_replay_is_passed_through_for_storage_conflict_check() {
    let original = example_params();
    let mut changed = original.clone();
    changed["choice_set"]["decision_question"] = json!("A changed question?");
    let MatrixTaskInvocation::Record(before) = parse("record_matrix_task", original).unwrap()
    else {
        panic!("record")
    };
    let MatrixTaskInvocation::Record(after) = parse("record_matrix_task", changed).unwrap() else {
        panic!("record")
    };
    assert_eq!(before.request_id, after.request_id);
    assert_eq!(before.task_id, after.task_id);
    assert_ne!(before.choice_set, after.choice_set);
}

fn maximum_context() -> tect_domain::EffectiveMatrixRequirements {
    use tect_domain::*;
    let anchor = RequirementsAnchor::Program {
        program_id: Uuid::new_v4(),
    };
    let recorder = DeclarationRecorder {
        principal: "owner".into(),
        session: "session".into(),
    };
    let values = vec![
        DeclaredRequirementValue::Mode(EngineeringMode::Production),
        DeclaredRequirementValue::Intent(EngineeringIntent::Other("\0".repeat(256))),
        DeclaredRequirementValue::Urgency("\0".repeat(256)),
        DeclaredRequirementValue::PromisedBehavior("\0".repeat(256)),
        DeclaredRequirementValue::PromisedProof("\0".repeat(256)),
        DeclaredRequirementValue::NoDemandCommitment,
        DeclaredRequirementValue::NoLatencyCommitment,
    ];
    let proposal = MatrixRequirementsProposal::new(
        anchor,
        1,
        values
            .into_iter()
            .map(|value| RequirementDeclarationPatch::Set { value })
            .collect(),
        recorder.clone(),
    )
    .unwrap();
    let confirmation = MatrixRequirementsConfirmation::new(
        &proposal,
        1,
        proposal.digest().into(),
        "owner".into(),
        "owner-adopted-exact-proposal".into(),
        recorder,
    )
    .unwrap();
    resolve_matrix_requirements(
        &[anchor],
        &[MatrixRequirementsRevision {
            proposal,
            confirmation: Some(confirmation),
        }],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap()
}

fn bound_fixture_source(
    request: &RecordMatrixTask,
    locator: &MatrixRequirementsLocator,
    context: &tect_domain::EffectiveMatrixRequirements,
) -> MatrixTaskSource {
    let mut actual = record_source_projection(request);
    actual.revision.input =
        tect_domain::bind_matrix_requirements_input(context, &request.input).unwrap();
    actual.requirements_binding = Some(tect_application::MatrixTaskRequirementsBinding {
        locator: locator.clone(),
        snapshot_id: Uuid::new_v4(),
        semantic_digest: context.semantic_digest().into(),
        authority_schema: context.schema().into(),
    });
    actual
}

fn source_response_len(source_value: MatrixTaskSource) -> usize {
    crate::responses::encoded_len(&crate::responses::with_actions(
        source(source_value),
        Vec::new(),
        None,
    ))
    .unwrap()
}

#[test]
fn bound_record_guard_rejects_real_four_nul_declaration_growth_before_service() {
    let MatrixTaskInvocation::Record(request) =
        parse("record_matrix_task", example_params()).unwrap()
    else {
        panic!("record")
    };
    let original = request.clone();
    let context = maximum_context();
    let locator = MatrixRequirementsLocator::Program {
        program_id: context.program_id(),
    };
    let capacity = source_response_len(record_source_projection(&request)) + 4096;
    // The old reserve admits this exact request, but the validated binder's
    // actual response is too wide. This is not a global frame-overflow claim.
    assert!(guard_record_output(&request, capacity.saturating_sub(4096)).is_ok());
    let actual = bound_fixture_source(&request, &locator, &context);
    actual.revision.input.validate().unwrap();
    assert!(source_response_len(actual) > capacity);
    let service_called = std::cell::Cell::new(false);
    let result = guard_bound_record_output(&request, &locator, capacity).map(|()| {
        service_called.set(true);
    });
    assert!(matches!(result, Err(Error::RequestTooLarge)));
    assert!(!service_called.get());
    assert_eq!(format!("{request:?}"), format!("{original:?}"));
}

#[test]
fn bound_projection_dominates_real_binding_for_all_locators_known_facts_and_choices() {
    use tect_domain::{FactProvenance, MatrixFact};
    let context = maximum_context();
    let id = Uuid::new_v4();
    let locators = [
        MatrixRequirementsLocator::Program {
            program_id: context.program_id(),
        },
        MatrixRequirementsLocator::Scope {
            program_id: context.program_id(),
            scope_id: id,
        },
        MatrixRequirementsLocator::Slice {
            program_id: context.program_id(),
            scope_id: id,
            candidate_set_id: id,
            work_candidate_id: id,
            expected_work_revision: i64::MAX,
        },
        MatrixRequirementsLocator::OpenedSlice { slice_id: id },
    ];
    for locator in locators {
        for has_choices in [false, true] {
            for existing_known in [false, true] {
                let MatrixTaskInvocation::Record(mut request) =
                    parse("record_matrix_task", example_params()).unwrap()
                else {
                    panic!("record")
                };
                if !has_choices {
                    request.choice_set = None;
                }
                if existing_known {
                    request.input =
                        tect_domain::bind_matrix_requirements_input(&context, &request.input)
                            .unwrap();
                    request.input.urgency = MatrixFact::Known {
                        value: "\0".repeat(256),
                        provenance: FactProvenance("\0".repeat(256)),
                    };
                }
                // Unresolved operating claims are untouched by the declaration
                // binder and must retain exact provenance in the projection.
                request.input.criticality = MatrixFact::Unknown {
                    provenance: FactProvenance("reported-unknown".into()),
                };
                request.input.validate().unwrap();
                let original = request.clone();
                let actual = bound_fixture_source(&request, &locator, &context);
                let projected = bound_record_projection(&request, &locator);
                assert_eq!(
                    projected.revision.input.criticality,
                    request.input.criticality
                );
                if existing_known {
                    assert_eq!(projected.revision.input, request.input);
                }
                assert_eq!(projected.revision.choice_set, request.choice_set);
                let projected_size = source_response_len(projected);
                assert!(projected_size >= source_response_len(actual));
                assert!(guard_bound_record_output(&request, &locator, projected_size).is_ok());
                assert_eq!(format!("{request:?}"), format!("{original:?}"));
            }
        }
    }
}
