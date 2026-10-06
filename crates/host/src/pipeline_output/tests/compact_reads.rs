use super::*;
use tect_application::PipelineExecutionOutputGuard;
use tect_domain::{
    PipelineDetailsSection, PipelineInput, PipelineOutputBinding, PipelineRunContextQuery,
    PipelineRunContextView,
};

fn query(context: &PipelineRunContext, view: PipelineRunContextView) -> PipelineRunContextQuery {
    PipelineRunContextQuery {
        run_id: context.run.id,
        view,
        output_id: None,
        digest: None,
        refresh: false,
        offset_bytes: Some(0),
        limit_bytes: None,
        representation_digest: None,
        definition_digest: matches!(
            view,
            PipelineRunContextView::Snapshot | PipelineRunContextView::PhaseContract
        )
        .then(|| context.run.definition_digest.clone()),
        phase_id: (view == PipelineRunContextView::PhaseContract)
            .then(|| context.run.current_phase_id.clone().unwrap()),
        run_revision: (view == PipelineRunContextView::Details).then_some(context.run.revision),
        section: None,
        receipt_kind: None,
        submitted_receipts: None,
        submitted_digest: None,
    }
}
fn read_all(context: &PipelineRunContext, mut query: PipelineRunContextQuery) -> Value {
    let mut bytes = Vec::new();
    let mut max = 0;
    loop {
        let response = query.resolve_pinned_read(context).unwrap();
        let page = context_pinned(response, 8192, &query).unwrap();
        assert_eq!(
            page,
            context_pinned(query.resolve_pinned_read(context).unwrap(), 8192, &query).unwrap()
        );
        max = max.max(responses::encoded_len(&page).unwrap());
        assert!(max <= 8192);
        bytes.extend_from_slice(page["text"].as_str().unwrap().as_bytes());
        let Some(next) = page["next_offset_bytes"].as_u64() else {
            break;
        };
        assert!(next > query.offset_bytes.unwrap_or(0));
        let params = &page["actions"][0]["arguments"]["params"];
        crate::pipeline_tools::parse("slice_pipeline_context", params.clone()).unwrap();
        query = serde_json::from_value(params.clone()).unwrap();
        query.validate().unwrap();
    }
    println!("read_fixture_max_envelope_bytes={max}");
    serde_json::from_slice(&bytes).unwrap()
}
#[test]
fn every_kind_mode_and_historical_version_has_bounded_compact_lifecycle_replies() {
    let mut max = 0;
    for kind in tect_domain::PipelineKind::HISTORICAL_SLICE_RUN_KINDS
        .into_iter()
        .chain([
            tect_domain::PipelineKind::Research,
            tect_domain::PipelineKind::DeepBrainstorming,
            tect_domain::PipelineKind::PromoteToDurableKnowledge,
        ])
    {
        for mode in [PipelineDeliveryMode::Whole, PipelineDeliveryMode::Phasewise] {
            for version in [
                "0.7.0-native.test",
                "0.7.1-native.k1k5",
                "0.7.0-native.k1k5",
                "fixture-version",
                "0.6.0-native.engineering.2",
            ] {
                let mut current = context(PipelineKnowledgeResourceState::Current, false);
                current.run.definition_kind = kind;
                current.definition.kind = kind;
                current.run.delivery_mode = mode;
                current.run.definition_version = version.into();
                current.definition.version = version.into();
                current.definition.overview.body = "huge method🙂".repeat(10000);
                current.definition.phases[0].instructions[0].body = "huge guidance".repeat(10000);
                current.run.qualification_reason = "large historic explanation".repeat(10000);
                let output:tect_domain::PipelinePhaseOutput=serde_json::from_value(json!({
                    "id":uuid::Uuid::new_v4(),"run_id":current.run.id,"phase_id":"fixture-phase","phase_ordinal":1,"revision":1,
                    "body":"large output🙂".repeat(2000),"producer_context_id":"producer","digest":"body-digest","reference":null,
                    "fields":{},"verdict":null,"dispositions":[],"skill_reads":[],"resource_reads":[],"artifacts":[],
                    "validator_receipts":[],"followup_proposal":null,"stale":false,"stale_reason":null})).unwrap();
                current.outputs = vec![output; 4];
                let checkpoint = open_checkpoint(&current, None);
                current.checkpoints = vec![checkpoint; 100];
                let before = serde_json::to_value(&current).unwrap();
                for value in [
                    super::super::begin(BeginPipelineRunOutcome::Created(current.clone()), 8192)
                        .unwrap(),
                    super::super::begin(BeginPipelineRunOutcome::Replay(current.clone()), 8192)
                        .unwrap(),
                    super::super::context(
                        PipelineContextResponse::Current(Box::new(current.clone())),
                        8192,
                        true,
                    )
                    .unwrap(),
                    mutation(
                        PipelineMutationOutcome {
                            context: current.clone(),
                            result: None,
                        },
                        8192,
                    )
                    .unwrap(),
                ] {
                    max = max.max(responses::encoded_len(&value).unwrap());
                    assert!(max <= 8192);
                    let compact = value
                        .get("created")
                        .or_else(|| value.get("replay"))
                        .or_else(|| value.get("context"))
                        .unwrap_or(&value);
                    assert_eq!(compact["delivery_scope"], "snapshot_reference");
                    for destination in compact["field_destinations"]
                        .as_object()
                        .unwrap()
                        .values()
                        .chain([&compact["output_availability"]])
                    {
                        let view = destination["view"].as_str().unwrap();
                        assert!(["snapshot", "phase_contract", "details"].contains(&view));
                        if view == "details" {
                            assert!(
                                ["all", "inputs", "outputs", "history"]
                                    .contains(&destination["section"].as_str().unwrap())
                            );
                        }
                    }
                    for field in [
                        "outputs",
                        "bindings",
                        "attempts",
                        "checkpoints",
                        "inputs",
                        "knowledge_resources",
                        "inquiry",
                        "result",
                        "delivered_phases",
                    ] {
                        assert!(compact.get(field).is_none(), "{field}");
                    }
                    for action in value["actions"].as_array().unwrap() {
                        assert!(action.get("route_contract").is_none());
                        assert!(action.get("next_action_contract").is_none());
                    }
                }
                assert_eq!(serde_json::to_value(&current).unwrap(), before);
            }
        }
    }
    println!("compact_fixture_max_envelope_bytes={max}");
}
#[test]
fn static_reads_are_lossless_stored_pinned_and_do_not_change_receipts() {
    let mut current = context(PipelineKnowledgeResourceState::Current, false);
    current.definition.overview.body = "stored method🙂\\\"".repeat(3000);
    current.definition.phases[0].instructions[0].body = "stored phase🙂\\\"".repeat(3000);
    current.run.status = PipelineRunStatus::Superseded;
    let before = current.clone();
    let snapshot = read_all(&current, query(&current, PipelineRunContextView::Snapshot));
    assert_eq!(
        snapshot["definition"],
        serde_json::to_value(&current.definition).unwrap()
    );
    assert_eq!(snapshot["definition_digest"], current.run.definition_digest);
    let phase = read_all(
        &current,
        query(&current, PipelineRunContextView::PhaseContract),
    );
    assert_eq!(
        phase["phase"],
        serde_json::to_value(&current.definition.phases[0]).unwrap()
    );
    let mut bad = query(&current, PipelineRunContextView::Snapshot);
    bad.definition_digest = Some("wrong".into());
    assert_eq!(
        bad.resolve_pinned_read(&current)
            .unwrap_err()
            .refusal()
            .unwrap()
            .path
            .as_deref(),
        Some("arguments.params.definition_digest")
    );
    let mut bad = query(&current, PipelineRunContextView::PhaseContract);
    bad.phase_id = Some("phase-alias-does-not-exist".into());
    assert_eq!(
        bad.resolve_pinned_read(&current).unwrap_err(),
        Error::NotFound
    );
    assert_eq!(current, before);
}
#[test]
fn details_cover_removed_fields_truthful_erasure_and_legacy_consumption_without_echo() {
    let mut current = context(PipelineKnowledgeResourceState::Current, true);
    current.run.current_phase_ordinal = Some(500);
    current.inputs = (0..70)
        .map(|i| PipelineInput {
            id: uuid::Uuid::new_v4(),
            sequence: i,
            phase_id: "fixture-phase".into(),
            input: "private input🙂".repeat(8),
            digest: format!("input:{i}"),
            actor_session_id: uuid::Uuid::new_v4(),
            source_amendment: None,
        })
        .collect();
    current.bindings = (0..70)
        .map(|i| PipelineOutputBinding {
            phase_id: format!("predecessor-{i}"),
            phase_ordinal: i + 1,
            output_revision: 1,
            output_id: uuid::Uuid::new_v4(),
            output_digest: "b".repeat(64),
            reference: None,
            stale: false,
            stale_reason: None,
        })
        .collect();
    current.outputs_complete = false;
    current.checkpoints = vec![open_checkpoint(&current, None)];
    let all = read_all(&current, query(&current, PipelineRunContextView::Details));
    let full = serde_json::to_value(&current).unwrap();
    for field in [
        "inputs",
        "knowledge",
        "knowledge_status",
        "knowledge_resources",
        "knowledge_resource_status",
        "bindings",
        "outputs",
        "outputs_complete",
        "attempts",
        "checkpoints",
        "inquiry",
        "result",
        "source_checkpoint",
        "delivered_phases",
    ] {
        assert_eq!(all["data"][field], full[field], "{field}");
    }
    assert_eq!(all["data"]["erasure"]["payloads_omitted"], true);
    assert_eq!(
        all["data"]["consumed_outputs"].as_array().unwrap().len(),
        70
    );
    assert_eq!(all["data"]["consumed_inputs"].as_array().unwrap().len(), 70);
    let compact = super::super::context(
        PipelineContextResponse::Current(Box::new(current.clone())),
        8192,
        false,
    )
    .unwrap();
    assert!(responses::encoded_len(&compact).unwrap() <= 8192);
    assert!(compact.get("outputs").is_none());
    let complete = action(
        compact["actions"].as_array().unwrap(),
        "slice.pipeline.phase.complete",
    );
    assert!(
        complete["arguments"]["params"]
            .get("consumed_outputs")
            .is_none()
    );
    assert_eq!(complete["kind"], "needs_context");
    let fields = complete["context_input"]["fields"].as_array().unwrap();
    for path in [
        "arguments.params.consumed_outputs",
        "arguments.params.consumed_inputs",
        "arguments.params.consumed_knowledge",
    ] {
        assert!(fields.iter().any(|field| field["path"] == path), "{path}");
    }
    assert!(
        !fields
            .iter()
            .any(|field| field["path"].as_str().unwrap().contains("/"))
    );
    let mut inputs_query = query(&current, PipelineRunContextView::Details);
    inputs_query.section = Some(PipelineDetailsSection::Inputs);
    let inputs = read_all(&current, inputs_query.clone());
    assert!(inputs["data"].get("outputs").is_none());
    current.run.definition_version = "0.7.0-native.test".into();
    let v7 = read_all(&current, inputs_query);
    assert!(v7["data"].get("consumed_outputs").is_none());
    let mut stale = query(&current, PipelineRunContextView::Details);
    stale.run_revision = Some(current.run.revision - 1);
    assert_eq!(
        stale
            .resolve_pinned_read(&current)
            .unwrap_err()
            .refusal()
            .unwrap()
            .code,
        tect_domain::RefusalCode::StaleRevision
    );
}
#[test]
fn guard_and_final_reply_share_projection_and_never_hide_non_size_errors() {
    let current = context(PipelineKnowledgeResourceState::Current, false);
    assert!(PipelineEncoding::new(8192).check_context(&current).is_ok());
    assert!(
        super::super::context(
            PipelineContextResponse::Current(Box::new(current.clone())),
            8192,
            false
        )
        .is_ok()
    );
    assert_eq!(
        PipelineEncoding::new(1).check_context(&current),
        Err(Error::RequestTooLarge)
    );
    let mut invalid = current;
    invalid.run.current_phase_ordinal = None;
    assert_eq!(
        PipelineEncoding::new(8192).check_context(&invalid),
        Err(Error::InternalInvariant)
    );
}
#[test]
fn missing_existing_receipt_recovers_only_exact_current() {
    let run_id = uuid::Uuid::new_v4();
    let error = Error::refused_at(
        tect_domain::RefusalCode::DeliveryRefreshRequired,
        "PIPELINE-SNAPSHOT-REFERENCE-MISSING",
        "arguments.params.view",
        "existing receipt",
        "missing",
        "refresh_pipeline_context",
        "current_pipeline_context",
    );
    let reply = responses::failure(
        error,
        Some((
            "query",
            &json!({"route":"slice.pipeline.context","params":{"run_id":run_id,"view":"delivery_receipt"}}),
        )),
    );
    let data: Value = serde_json::from_str(reply["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(data["actions"].as_array().unwrap().len(), 1);
    assert_eq!(
        data["actions"][0]["arguments"]["params"],
        json!({"run_id":run_id,"view":"current"})
    );
}
