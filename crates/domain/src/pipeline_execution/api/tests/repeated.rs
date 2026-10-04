use super::*;

const STANDARDS: &str = "tect:engineering-standards";
const VERSION: &str = "1.0.0";
const DIGEST: &str = "f4374bc9f68fc9bcc7c844765d0ea4ccc581042f169ed7756a5d6e106fef9f50";
const CONTRACT: &str = "internal-instruction.tect-slice-contract-writer";
const CONTRACT_DIGEST: &str = "bddc82a2ce30dd808f8cc8238e10c09fe1cf293686510e53528e01a04931df96";

fn full_context() -> PipelineRunContext {
    let mut context = context();
    context.definition = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../host/pipeline-definitions/full-design-to-execution.json"
    )))
    .unwrap();
    context.run.definition_kind = context.definition.kind;
    context.run.definition_version = context.definition.version.clone();
    context.run.definition_digest = context.definition.digest.clone();
    context.run.current_phase_id = Some("slice-design-spec-shaper".into());
    context.run.current_phase_ordinal = Some(3);
    context
}

fn conflicting_resource(context: &mut PipelineRunContext) -> &mut PipelineInstructionSnapshot {
    context
        .definition
        .phases
        .iter_mut()
        .find(|phase| phase.id == "slice-contract-writer")
        .unwrap()
        .resources
        .iter_mut()
        .find(|resource| resource.id == STANDARDS)
        .unwrap()
}

fn assert_unavailable(context: &PipelineRunContext, id: &str, version: &str, digest: &str) {
    let error = query(context, id, version, digest)
        .resolve(context)
        .unwrap_err();
    assert_eq!(
        error.refusal().unwrap().code,
        RefusalCode::MethodVersionUnavailable
    );
}

#[test]
fn published_repeated_standards_resolve_with_current_phase_attribution() {
    let mut context = full_context();
    let occurrences = context
        .definition
        .phases
        .iter()
        .flat_map(|phase| &phase.resources)
        .filter(|resource| resource.id == STANDARDS)
        .count();
    assert_eq!(occurrences, 11);
    // Put the current phase after another identical binding to exercise preference.
    context.definition.phases.swap(2, 3);
    let response = query(&context, STANDARDS, VERSION, DIGEST)
        .resolve(&context)
        .unwrap();
    assert_eq!(
        response.phase_id.as_deref(),
        Some("slice-design-spec-shaper")
    );
    assert_eq!(response.section, PipelineInstructionSection::Resource);
    assert_eq!(response.instruction.digest, DIGEST);
    assert!(
        response
            .instruction
            .body
            .contains("# Engineering standards")
    );
}

#[test]
fn repeated_standards_preserve_canonical_order_without_a_current_binding() {
    let mut context = full_context();
    context.run.current_phase_id = Some("slice-workspace-preflight".into());
    let response = query(&context, STANDARDS, VERSION, DIGEST)
        .resolve(&context)
        .unwrap();
    assert_eq!(
        response.phase_id.as_deref(),
        Some("slice-design-spec-shaper")
    );
}

#[test]
fn repeated_standards_reject_conflicting_authoritative_fields() {
    for field in ["body", "origin", "version", "digest"] {
        let mut context = full_context();
        let resource = conflicting_resource(&mut context);
        match field {
            "body" => resource.body.push_str("\nconflicting contract"),
            "origin" => resource.origin_refs.push("conflicting-origin".into()),
            "version" => resource.version = "2.0.0".into(),
            "digest" => resource.digest = "different-digest".into(),
            _ => unreachable!(),
        }
        assert_unavailable(&context, STANDARDS, VERSION, DIGEST);
    }
}

#[test]
fn archived_contract_resolves_identical_guidance_and_resource_with_honest_attribution() {
    let mut context = legacy_full_context();
    context.run.current_phase_id = Some("slice-contract-writer".into());
    context.run.current_phase_ordinal = Some(4);
    let phase = &context.definition.phases[3];
    let instruction = phase
        .instructions
        .iter()
        .find(|item| item.id == CONTRACT)
        .unwrap();
    let resource = phase
        .resources
        .iter()
        .find(|item| item.id == CONTRACT)
        .unwrap();
    assert_eq!(instruction, resource);
    let response = query(&context, CONTRACT, "0.1.0", CONTRACT_DIGEST)
        .resolve(&context)
        .unwrap();
    assert_eq!(response.phase_id.as_deref(), Some("slice-contract-writer"));
    assert_eq!(response.section, PipelineInstructionSection::Instruction);
    assert_eq!(&response.instruction, instruction);
}

#[test]
fn archived_cross_section_bindings_still_reject_conflicting_authoritative_fields() {
    for field in ["body", "origin", "version", "digest"] {
        let mut context = legacy_full_context();
        let resource = context.definition.phases[3]
            .resources
            .iter_mut()
            .find(|item| item.id == CONTRACT)
            .unwrap();
        match field {
            "body" => resource.body.push_str("\nconflicting contract"),
            "origin" => resource.origin_refs.push("conflicting-origin".into()),
            "version" => resource.version = "2.0.0".into(),
            "digest" => resource.digest = "different-digest".into(),
            _ => unreachable!(),
        }
        assert_unavailable(&context, CONTRACT, "0.1.0", CONTRACT_DIGEST);
    }
}

#[test]
fn every_published_full_phase_carrier_resolves_its_exact_snapshot() {
    let mut context = full_context();
    let phases = context.definition.phases.clone();
    assert_eq!(phases.len(), 21);
    for phase in phases {
        context.run.current_phase_id = Some(phase.id.clone());
        context.run.current_phase_ordinal = Some(phase.ordinal);
        for snapshot in phase
            .instructions
            .iter()
            .chain(&phase.skills)
            .chain(&phase.resources)
        {
            let response = query(&context, &snapshot.id, &snapshot.version, &snapshot.digest)
                .resolve(&context)
                .unwrap_or_else(|error| panic!("{} / {}: {error:?}", phase.id, snapshot.id));
            assert_eq!(response.phase_id.as_deref(), Some(phase.id.as_str()));
            assert_eq!(&response.instruction, snapshot);
        }
    }
}

#[test]
fn repeated_standards_still_require_exact_requested_pins() {
    let context = full_context();
    assert_unavailable(&context, STANDARDS, "2.0.0", DIGEST);
    assert_unavailable(&context, STANDARDS, VERSION, "different-digest");
    assert_unavailable(&context, "missing-resource", VERSION, DIGEST);
    let mut without_refresh = query(&context, STANDARDS, VERSION, DIGEST);
    without_refresh.refresh = false;
    assert_eq!(
        without_refresh
            .resolve(&context)
            .unwrap_err()
            .refusal()
            .unwrap()
            .code,
        RefusalCode::DeliveryRefreshRequired
    );
}

fn legacy_full_context() -> PipelineRunContext {
    let mut context = full_context();
    context.definition = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../host/pipeline-definitions/full-design-to-execution-0.6.0-native.engineering.2.json"
    )))
    .unwrap();
    context.run.definition_version = context.definition.version.clone();
    context.run.definition_digest = context.definition.digest.clone();
    context
}

#[test]
fn native_contract_schema_is_exactly_readable_at_writer_and_consumers() {
    let mut context = full_context();
    for ordinal in [4, 10, 11, 13] {
        context.run.current_phase_id = Some(context.definition.phases[ordinal - 1].id.clone());
        context.run.current_phase_ordinal = Some(ordinal as u32);
        let resource = context.definition.phases[ordinal - 1]
            .resources
            .iter()
            .find(|r| r.id == "tect:native-slice-work-contract-schema")
            .unwrap()
            .clone();
        let response = query(&context, &resource.id, &resource.version, &resource.digest)
            .resolve(&context)
            .unwrap();
        assert_eq!(response.section, PipelineInstructionSection::Resource);
        assert_eq!(response.phase_id, context.run.current_phase_id);
        assert_eq!(response.instruction, resource);
        assert_unavailable(&context, &resource.id, "wrong-version", &resource.digest);
        assert_unavailable(&context, &resource.id, &resource.version, "wrong-digest");
    }
    let legacy = legacy_full_context();
    assert_unavailable(
        &legacy,
        "tect:native-slice-work-contract-schema",
        "1.0.0",
        "wrong-digest",
    );
}
