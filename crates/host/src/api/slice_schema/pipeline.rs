pub(super) fn scope_context() -> Value {
    object_schema(json!({"scope_id":uuid()}), json!(["scope_id"]))
}

pub(super) fn slice_context() -> Value {
    object_schema(json!({"slice_id":uuid()}), json!(["slice_id"]))
}

pub(super) fn pipeline_context() -> Value {
    object_schema(
        json!({"run_id":uuid(),"view":{"type":"string","enum":["current","output","delivery_receipt"]},
            "output_id":uuid(),"digest":text(),"refresh":{"type":"boolean"}}),
        json!(["run_id"]),
    )
}

pub(super) fn pipeline_instruction() -> Value {
    object_schema(
        json!({
            "run_id":uuid(),
            "instruction_id":text(),
            "version":text(),
            "digest":text(),
            "refresh":{"type":"boolean"}
        }),
        json!(["run_id", "instruction_id", "version", "digest"]),
    )
}

pub(super) fn pipeline_begin() -> Value {
    object_schema(
        json!({
            "request_id":uuid(),"scope_id":uuid(),"slice_id":uuid(),
            "slice_revision":{"type":"integer","minimum":1},
            "delivery_mode":{"type":"string","enum":["whole","phasewise"]},
            "definition_version":text(),
            "inquiry":inquiry(),"source_checkpoint":checkpoint_ref(),
            "qualification_reason":text()
        }),
        json!([
            "request_id",
            "scope_id",
            "slice_id",
            "slice_revision",
            "qualification_reason"
        ]),
    )
}

pub(super) fn pipeline_run_migrate() -> Value {
    let evidence = object_schema(
        json!({"reference":text(),"digest":text()}),
        json!(["reference", "digest"]),
    );
    let mapping = object_schema(
        json!({"legacy_obligation_id":text(),"successor_obligation_id":text(),
            "evidence_refs":{"type":"array","items":evidence,"minItems":1,"uniqueItems":true}}),
        json!([
            "legacy_obligation_id",
            "successor_obligation_id",
            "evidence_refs"
        ]),
    );
    object_schema(
        json!({"request_id":uuid(),"predecessor_run_id":uuid(),
            "expected_revision":{"type":"integer","minimum":1},
            "idempotency_key":{"type":"string","minLength":1,"maxLength":128},
            "successor_definition_version":text(),
            "mappings":{"type":"array","items":mapping,"minItems":1,"uniqueItems":true}}),
        json!([
            "request_id",
            "predecessor_run_id",
            "expected_revision",
            "idempotency_key",
            "successor_definition_version",
            "mappings"
        ]),
    )
}

pub(super) fn pipeline_input() -> Value {
    let artifact = object_schema(
        json!({"name":text(),"media_type":text(),"body":{"type":"string","minLength":1,"maxLength":2097152},
            "digest":text(),"reference":text()}),
        json!(["name", "media_type", "body", "digest"]),
    );
    let predecessor = object_schema(
        json!({"output_id":uuid(),"output_revision":{"type":"integer","minimum":1},
            "output_digest":text(),"artifact_name":text(),"artifact_digest":text(),
            "source_path":text(),"source_digest":text()}),
        json!([
            "output_id",
            "output_revision",
            "output_digest",
            "artifact_name",
            "artifact_digest",
            "source_path",
            "source_digest"
        ]),
    );
    let successor = object_schema(
        json!({"path":text(),"artifact":artifact}),
        json!(["path", "artifact"]),
    );
    let amendment = object_schema(
        json!({"target_phase_id":text(),"predecessor":predecessor,"successor":successor,
            "authorization_scope":text(),"authorization_provenance":text()}),
        json!([
            "target_phase_id",
            "predecessor",
            "successor",
            "authorization_scope",
            "authorization_provenance"
        ]),
    );
    object_schema(
        json!({"request_id":uuid(),"run_id":uuid(),"run_revision":{"type":"integer","minimum":1},"phase_id":text(),"input":{"type":"string","minLength":1,"maxLength":65536},"source_amendment":amendment}),
        json!(["request_id", "run_id", "run_revision", "phase_id", "input"]),
    )
}

pub(super) fn pipeline_delivery_escalate() -> Value {
    object_schema(
        json!({"request_id":uuid(),"run_id":uuid(),"run_revision":{"type":"integer","minimum":1},"phase_id":text(),"reason":text()}),
        json!(["request_id", "run_id", "run_revision", "phase_id", "reason"]),
    )
}

pub(super) fn pipeline_phase_complete() -> Value {
    let consumed = object_schema(
        json!({"phase_id":text(),"output_revision":{"type":"integer","minimum":1},"digest":text()}),
        json!(["phase_id", "output_revision", "digest"]),
    );
    let consumed_knowledge = object_schema(
        json!({"manifest_id":uuid(),"digest":text()}),
        json!(["manifest_id", "digest"]),
    );
    let skill_read = object_schema(
        json!({"instruction_id":text(),"version":text(),"digest":text()}),
        json!(["instruction_id", "version", "digest"]),
    );
    let artifact = object_schema(
        json!({"name":text(),"media_type":text(),"body":{"type":"string","minLength":1,"maxLength":2097152},
            "digest":text(),"reference":text()}),
        json!(["name", "media_type", "body", "digest"]),
    );
    let reviewer = object_schema(
        json!({"reviewer_identity":context_id(),"reviewer_context_id":context_id(),
            "producer_context_ids":{"type":"array","items":context_id(),"minItems":1,"maxItems":100,"uniqueItems":true},"fresh_input":{"const":true}}),
        json!([
            "reviewer_identity",
            "reviewer_context_id",
            "producer_context_ids",
            "fresh_input"
        ]),
    );
    let artifact_digest = object_schema(
        json!({"name":text(),"digest":text()}),
        json!(["name", "digest"]),
    );
    let validator_receipt = object_schema(
        json!({"resource_id":text(),"version":text(),"digest":text(),"stage":text(),
            "command":text(),"exit_code":{"type":"integer"},"valid":{"type":"boolean"},
            "artifacts":{"type":"array","items":artifact_digest,"uniqueItems":true}}),
        json!([
            "resource_id",
            "version",
            "digest",
            "stage",
            "command",
            "exit_code",
            "valid",
            "artifacts"
        ]),
    );
    let followup_node = object_schema(
        json!({
            "local_id":text(),"pipeline":{"type":"string","enum":pipelines()},
            "status":{"type":"string","enum":["future_candidate","satisfied_by_current_run"]},
            "target":text(),"trigger":text(),
            "source_outputs":{"type":"array","items":consumed.clone(),"uniqueItems":true}
        }),
        json!(["local_id", "pipeline", "status", "target", "trigger"]),
    );
    let ordered_dependency = object_schema(
        json!({"kind":{"const":"ordered"},"predecessor":text(),"successor":text(),"condition":text()}),
        json!(["kind", "predecessor", "successor", "condition"]),
    );
    let unresolved_dependency = object_schema(
        json!({"kind":{"const":"unresolved"},"node_ids":{"type":"array","items":text(),"minItems":2,"uniqueItems":true},"condition":text(),"owner":text()}),
        json!(["kind", "node_ids", "condition", "owner"]),
    );
    let followup_proposal = object_schema(
        json!({
            "nodes":{"type":"array","items":followup_node,"minItems":2,"maxItems":16,"uniqueItems":true},
            "dependencies":{"type":"array","items":{"oneOf":[ordered_dependency,unresolved_dependency]},"minItems":1,"maxItems":32,"uniqueItems":true}
        }),
        json!(["nodes", "dependencies"]),
    );
    let knowledge_publication = object_schema(
        json!({"change_id":uuid(),"publisher_receipt_id":uuid(),"publisher_receipt_digest":text(),
            "operation_ids":{"type":"array","items":uuid(),"minItems":1,"maxItems":16,"uniqueItems":true}}),
        json!([
            "change_id",
            "publisher_receipt_id",
            "publisher_receipt_digest",
            "operation_ids"
        ]),
    );
    let output = object_schema(
        json!({
            "body":{"type":"string","minLength":1,"maxLength":2097152},
            "producer_context_id":context_id(),
            "fields":{"type":"object","additionalProperties":{"type":"string"}},
            "verdict":text(),"dispositions":{"type":"array","items":text(),"uniqueItems":true},
            "skill_reads":{"type":"array","items":skill_read,"uniqueItems":true},
            "resource_reads":{"type":"array","items":skill_read,"uniqueItems":true},
            "artifacts":{"type":"array","items":artifact,"uniqueItems":true},
            "validator_receipts":{"type":"array","items":validator_receipt,"uniqueItems":true},
            "followup_proposal":followup_proposal,
            "reviewer_context":reviewer,"reference":text(),"knowledge_publication":knowledge_publication
        }),
        json!(["producer_context_id"]),
    );
    let evidence = object_schema(
        json!({"kind":text(),"reference":text(),"observation":text()}),
        json!(["kind", "reference", "observation"]),
    );
    let terminal = object_schema(
        json!({"summary":text(),"evidence":{"type":"array","items":evidence,"minItems":1,"maxItems":100},"scope_impact":text(),"remaining_work":text()}),
        json!(["summary", "evidence", "scope_impact", "remaining_work"]),
    );
    let research_checkpoint = object_schema(
        json!({"question":text(),"answer_criteria":text(),"inquiry":inquiry(),"reason":text()}),
        json!(["question", "answer_criteria", "inquiry", "reason"]),
    );
    object_schema(
        json!({
            "request_id":uuid(),"run_id":uuid(),"run_revision":{"type":"integer","minimum":1},
            "phase_id":text(),"outcome":{"type":"string","enum":["completed","waiting_input","blocked"]},
            "transition":{"type":"string","enum":["continue","complete","block","escalate"]},
            "output":output,
            "consumed_knowledge":consumed_knowledge,"revisit_phase_id":text(),"escalation_target":{"type":"string","enum":pipelines()},
            "terminal_result":terminal,"publish_blocked_result":{"type":"boolean","default":false}
            ,"research_checkpoint":research_checkpoint
        }),
        json!([
            "request_id",
            "run_id",
            "run_revision",
            "phase_id",
            "outcome",
            "transition",
            "output"
        ]),
    )
}

pub(super) fn pipeline_checkpoint_resolve() -> Value {
    let terminal = object_schema(
        json!({"result_id":uuid(),"output_id":uuid(),"output_digest":text()}),
        json!(["result_id", "output_id", "output_digest"]),
    );
    object_schema(
        json!({"request_id":uuid(),"producer_run_id":uuid(),
            "producer_run_revision":{"type":"integer","minimum":1},
            "checkpoint":checkpoint_ref(),"action":{"type":"string","enum":["accept","reject","cancel"]},
            "reason":text(),"terminal":terminal}),
        json!([
            "request_id",
            "producer_run_id",
            "producer_run_revision",
            "checkpoint",
            "action",
            "reason"
        ]),
    )
}
