const ADVICE: &str = include_str!("../migrations/0057_matrix_advice_receipts.sql");
const AUTHORITY: &str = include_str!("../migrations/0058_matrix_advisory_verification_reasons.sql");
const CUTOVER: &str = include_str!("../migrations/0064_matrix_v1_dispatch_cutover_allowlist.sql");
const TRIAL: &str = include_str!("../migrations/0065_matrix_trial_uncertainty.sql");
const PREFERENCE: &str = include_str!("../migrations/0066_agent_session_advisory_preference.sql");
const GRANTS: &str = include_str!("admin/migration/matrix_advice.rs");
const STORE: &str = include_str!("matrix_advice_store/implementation.rs");
const READER: &str = include_str!("store/unit_of_work.rs");

#[test]
fn advice_runtime_insert_only_contract_preserves_exact_receipt_and_versioned_authority() {
    for required in [
        "advisory_matrix_advice_opportunity_unique",
        "advisory_matrix_advice_dispatch_unique",
        "tenant_id, workspace_id, opportunity_id, task_id",
        "matrix_task_revision, matrix_choice_set_digest",
        "advisory_matrix_advice_opportunity_fk",
        "advisory_matrix_advice_dispatch_fk",
        "advisory_matrix_advice_revision_fk",
        "provider_profile_ref text NOT NULL",
        "model_configuration jsonb NOT NULL",
        "response_payload_sha256 text NOT NULL",
        "advisory_matrix_advice_provider_profile_ref_check",
        "advisory_matrix_advice_model_configuration_check",
        "advisory_matrix_advice_response_payload_sha256_check",
        "ENABLE ROW LEVEL SECURITY",
        "FORCE ROW LEVEL SECURITY",
        "FROM PUBLIC",
    ] {
        assert!(ADVICE.contains(required), "missing {required}");
    }
    assert!(AUTHORITY.contains("advisory_opportunity_matrix_verification_fk"));
    assert!(GRANTS.contains("GRANT SELECT, INSERT ON TABLE advisory_matrix_advice"));
    assert!(GRANTS.contains("UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER"));
    assert!(GRANTS.contains("a.attname='id'"));
    for required in [
        "matrix_v1_cutover_advice_insert_guard",
        "BEFORE INSERT ON public.advisory_matrix_advice",
        "verification_schema='tect.context-matrix-verification/1'",
        "Matrix V1 advice dispatch was not sealed at cutover",
        "captured_fingerprint IS DISTINCT FROM",
        "LOCK TABLE public.advisory_dispatch IN ACCESS EXCLUSIVE MODE",
    ] {
        assert!(CUTOVER.contains(required), "missing {required}");
    }
    // Runtime INSERT-only privileges and store integrity are not universal admin immutability.
    assert!(STORE.contains("response_payload_sha256"));
    assert!(STORE.contains("configuration_digest"));
    // Exact known regression fragments, not a SQL parser or deployed migration proof.
    assert!(ADVICE.contains("response_payload_sha256 ~ '^[0-9a-f]{64}$'\n    )\n);"));
    assert_eq!(
        ADVICE
            .matches("CREATE POLICY advisory_matrix_advice_tenant_scope")
            .count(),
        1
    );
    assert_eq!(
        ADVICE
            .matches("REVOKE ALL PRIVILEGES ON TABLE advisory_matrix_advice FROM PUBLIC;")
            .count(),
        1
    );
}

#[test]
fn catalog_checks_cover_ten_trigger_structures_and_all_six_policy_inventory() {
    for required in [
        "count(*)=10",
        "NOT t.tgisinternal AND t.tgtype=e.type",
        "t.tgenabled::text=e.enabled",
        "t.tgnargs=0 AND pg_catalog.octet_length(t.tgargs)=0",
        "pn.nspname='public' AND p.proname=e.function AND p.pronargs=0",
        "p.prorettype='pg_catalog.trigger'::regtype",
        "count(*)=6",
        "p.polcmd::text=e.command AND p.polpermissive AND p.polroles=ARRAY[0::oid]",
        "p.polqual IS NOT NULL AND (p.polwithcheck IS NOT NULL)=e.checked",
        "=pg_catalog.pg_get_expr(p.polwithcheck,p.polrelid)",
        "FROM pg_catalog.pg_policy allp",
        "allc.relname IN ({TABLES}))=6",
    ] {
        assert!(GRANTS.contains(required), "missing {required}");
    }
    // These structural source assertions do not prove deployed expressions/RLS behavior.
}

#[test]
fn trial_columns_preserve_strict_null_pair_and_typed_trial_pair() {
    for required in [
        "ranking_policy_version IS NULL AND trial_uncertainty IS NULL",
        "tect.matrix-native-ranking-policy/robust-trial-v1",
        "kind = 'ranked'",
        "trial_uncertainty->>'policy_version' = ranking_policy_version",
        "jsonb_array_length(trial_uncertainty->'scores') = 2",
    ] {
        assert!(TRIAL.contains(required), "missing {required}");
    }
    assert!(!TRIAL.contains("UPDATE advisory_matrix_advice"));
    assert!(!TRIAL.contains("DROP COLUMN"));
    // Whole paired predicate must reject SQL UNKNOWN; this is source shape only.
    assert!(TRIAL.contains("advisory_matrix_advice_trial_uncertainty_pair_check CHECK (("));
    assert!(TRIAL.ends_with("    ) IS TRUE);\n"));
}

#[test]
fn session_preference_uses_real_durable_defaults_and_fail_closed_bound_reader() {
    for required in [
        "advisory_preference text NOT NULL DEFAULT 'use_workspace'",
        "advisory_preference_revision bigint NOT NULL DEFAULT 0",
        "CHECK (advisory_preference IN ('use_workspace', 'skip'))",
        "CHECK (advisory_preference_revision >= 0)",
    ] {
        assert!(PREFERENCE.contains(required), "missing {required}");
    }
    assert!(!PREFERENCE.contains("session_advisory_preference_history"));
    let method = &READER[READER
        .find("async fn session_advisory_preference(")
        .unwrap()..];
    let method = &method[..method
        .find("fn matrix_requirements_context_store(")
        .unwrap()];
    for required in [
        "self.tenant_id()?",
        "tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT revoked",
        ".bind(tenant_id)",
        ".bind(workspace_id)",
        ".bind(session_id)",
        "row.ok_or(Error::Forbidden)?",
        ".map_err(storage_error)?",
        "AdvisoryRequestPreference::UseWorkspace",
        "AdvisoryRequestPreference::Skip",
        "Err(Error::InternalInvariant)",
    ] {
        assert!(method.contains(required), "missing {required}");
    }
    assert!(!method.contains("unwrap_or_default"));
}
