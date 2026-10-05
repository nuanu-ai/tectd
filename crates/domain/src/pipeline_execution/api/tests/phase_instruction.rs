use super::*;
use serde_json::json;

fn phase_query(context: &PipelineRunContext, phase: &str) -> PipelineInstructionQuery {
    serde_json::from_value(json!({"run_id":context.run.id,"phase_id":phase})).unwrap()
}
fn check(error: Error, rule: &str, path: &str) {
    let refusal = error.refusal().unwrap();
    assert_eq!(refusal.rule.as_deref(), Some(rule));
    assert_eq!(refusal.path.as_deref(), Some(path));
}
#[test]
fn phase_selector_reads_only_unique_primary_instruction_from_immutable_run_snapshot() {
    let mut context = context();
    context.run.current_phase_id = Some("different-current-phase".into());
    let query = phase_query(&context, "phase-1");
    let response = query.resolve(&context).unwrap();
    assert_eq!(
        response.instruction,
        context.definition.phases[0].instructions[0]
    );
    assert_eq!(response.section, PipelineInstructionSection::Instruction);
    assert_eq!(response.phase_id.as_deref(), Some("phase-1"));
    assert_eq!(response.instruction.version, "0.6.0");
    context.definition.phases[0].instructions.clear();
    assert!(!context.definition.phases[0].skills.is_empty());
    check(
        query.resolve(&context).unwrap_err(),
        "WP6-INSTRUCTION-PHASE-EMPTY",
        "arguments.params.phase_id",
    );
    context.definition.phases[0].instructions =
        vec![snapshot("a", "1", "d"), snapshot("b", "1", "d")];
    check(
        query.resolve(&context).unwrap_err(),
        "WP6-INSTRUCTION-PHASE-AMBIGUOUS",
        "arguments.params.phase_id",
    );
    check(
        phase_query(&context, "unknown")
            .resolve(&context)
            .unwrap_err(),
        "WP6-INSTRUCTION-PHASE-UNKNOWN",
        "arguments.params.phase_id",
    );
}
#[test]
fn selector_presence_and_modes_have_precise_refusals() {
    let context = context();
    for (selectors, rule, path) in [
        (
            json!({"phase_id":""}),
            "WP6-INSTRUCTION-SELECTOR-BLANK",
            "arguments.params.phase_id",
        ),
        (
            json!({"phase_id":"phase-1","instruction_id":""}),
            "WP6-INSTRUCTION-SELECTOR-BLANK",
            "arguments.params.instruction_id",
        ),
        (
            json!({"phase_id":"phase-1","instruction_id":"i"}),
            "WP6-INSTRUCTION-SELECTOR-MIXED",
            "arguments.params.phase_id",
        ),
        (
            json!({"instruction_id":"i","version":"v"}),
            "WP6-INSTRUCTION-SELECTOR-MISSING",
            "arguments.params.digest",
        ),
        (
            json!({}),
            "WP6-INSTRUCTION-SELECTOR-MISSING",
            "arguments.params.instruction_id",
        ),
    ] {
        let mut value = selectors;
        value["run_id"] = json!(context.run.id);
        let query: PipelineInstructionQuery = serde_json::from_value(value).unwrap();
        check(query.validate().unwrap_err(), rule, path);
    }
}
#[test]
fn phase_omitted_refresh_is_read_but_explicit_false_and_old_omission_refuse() {
    let context = context();
    let mut phase = phase_query(&context, "phase-1");
    assert!(phase.resolve(&context).is_ok());
    phase.refresh = Some(false);
    check(
        phase.resolve(&context).unwrap_err(),
        "WP6-INSTRUCTION-REFRESH-01",
        "arguments.params.refresh",
    );
    let mut pinned = query(&context, "instruction", "0.6.0", "instruction-digest");
    pinned.refresh = None;
    check(
        pinned.resolve(&context).unwrap_err(),
        "WP6-INSTRUCTION-REFRESH-01",
        "arguments.params.refresh",
    );
    pinned.refresh = Some(true);
    assert!(pinned.resolve(&context).is_ok());
}
