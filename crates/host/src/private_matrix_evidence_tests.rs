use super::*;
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::os::unix::fs::{PermissionsExt, symlink};
use tect_application::UnitOfWork;
use tect_domain::{
    CommitmentEvidence, EngineeringIntent, EngineeringMode, FactProvenance, MatrixFact,
    OperatingEnvelope, OperationalFacts,
};

struct NeverStore;
#[async_trait]
impl Store for NeverStore {
    async fn begin(&self, _: TransactionMode) -> Result<Box<dyn UnitOfWork>> {
        Err(Error::Forbidden)
    }
}
fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: FactProvenance("inspected-fixture-only".into()),
    }
}
fn input() -> EngineeringMatrixInput {
    EngineeringMatrixInput {
        mode: known(EngineeringMode::Demo),
        envelope: OperatingEnvelope {
            scale: known("one private fixture".into()),
            operational_facts: OperationalFacts::KnownEmpty {
                provenance: FactProvenance("fixture-only".into()),
            },
        },
        criticality: known("reversible fixture".into()),
        intent: known(EngineeringIntent::Other("fixture".into())),
        urgency: known("fixture".into()),
        promised_behavior: known("fixture".into()),
        promised_proof: known("fixture".into()),
        affected_guarantees: MatrixFact::KnownEmpty {
            provenance: FactProvenance("fixture-only".into()),
        },
        actual_exposure: known(false),
        demand_commitment: known(CommitmentEvidence::NoCommitment),
        latency_commitment: known(CommitmentEvidence::NoCommitment),
        urgent_repair: known(false),
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
struct Fixture {
    _dir: tempfile::TempDir,
    validator: PrivateMatrixEvidenceValidator,
    fact: RequiredMatrixFact,
    envelope: Envelope,
    key: Ed25519KeyPair,
}
impl Fixture {
    fn new() -> Self {
        // macOS /var is a symlink. Canonicalize test root before path validation.
        let dir = tempfile::tempdir_in("/private/tmp").unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let key = Ed25519KeyPair::from_seed_unchecked(&[17; 32]).unwrap();
        let payload = Payload {
            schema: EVIDENCE_SCHEMA.into(),
            policy_version: "fixture-only-v1".into(),
            workspace_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            revision: 1,
            verifier_principal: Uuid::new_v4(),
            source: "fixture-only-source".into(),
            subject: "fixture-only-subject".into(),
            observed_at: 100,
            expires_at: 200,
            input: input(),
        };
        let fact = required_matrix_facts(&payload.input)
            .unwrap()
            .into_iter()
            .find(|f| f.path == "/criticality")
            .unwrap();
        let envelope = Envelope {
            payload,
            signature_hex: String::new(),
        };
        let path = root.join("policy.json");
        let mut value = Self {
            _dir: dir,
            validator: PrivateMatrixEvidenceValidator {
                policy: Policy {
                    schema: POLICY_SCHEMA.into(),
                    policy_version: "fixture-only-v1".into(),
                    tenant_id: Uuid::new_v4(),
                    workspace_id: envelope.payload.workspace_id,
                    verifier_principal: envelope.payload.verifier_principal,
                    verifier_auth: HostAuth {
                        host_id: Uuid::new_v4(),
                        credential: "a".repeat(64),
                    },
                    verifier_public_key_hex: hex(key.public_key().as_ref()),
                    artifact_root: root.clone(),
                    entries: vec![Entry {
                        file_name: "evidence.json".into(),
                        task_id: envelope.payload.task_id,
                        revision: 1,
                        binding: MatrixEvidenceBinding {
                            fact_path: fact.path.clone(),
                            value_digest: fact.value_digest.clone(),
                            evidence_ref: "approved:fixture-only".into(),
                            content_digest: String::new(),
                            source: envelope.payload.source.clone(),
                            subject: envelope.payload.subject.clone(),
                            observed_at: 100,
                            expires_at: 200,
                            validation_outcome: EvidenceValidationOutcome::Accepted,
                        },
                    }],
                },
                policy_path: path,
                policy_digest: String::new(),
                store: Arc::new(NeverStore),
            },
            fact,
            envelope,
            key,
        };
        value.write_artifact();
        fs::write(&value.validator.policy_path, b"pinned-fixture-policy").unwrap();
        fs::set_permissions(
            &value.validator.policy_path,
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        value.validator.policy_digest = hash(b"pinned-fixture-policy");
        value
    }
    fn write_artifact(&mut self) {
        self.envelope.signature_hex = hex(self
            .key
            .sign(
                &serde_json::to_vec(&serde_json::to_value(&self.envelope.payload).unwrap())
                    .unwrap(),
            )
            .as_ref());
        let bytes = serde_json::to_vec(&self.envelope).unwrap();
        let path = self.validator.policy.artifact_root.join("evidence.json");
        if path.exists() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(&path, &bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        self.validator.policy.entries[0].binding.content_digest = hash(&bytes);
    }
    fn resolve(&self, now: i64) -> Result<MatrixEvidenceBinding> {
        self.validator.resolve(
            self.envelope.payload.workspace_id,
            self.envelope.payload.task_id,
            1,
            &self.fact,
            "approved:fixture-only",
            now,
        )
    }
}
#[test]
fn signed_fixture_exact_binding_and_time_only() {
    let f = Fixture::new();
    assert!(f.resolve(150).is_ok());
    assert!(f.resolve(99).is_err());
    assert!(f.resolve(200).is_err());
    let mut overlong = Fixture::new();
    overlong.envelope.payload.expires_at = 4000;
    overlong.validator.policy.entries[0].binding.expires_at = 4000;
    overlong.write_artifact();
    assert!(overlong.resolve(150).is_err());
}
#[test]
fn rejects_foreign_binding_unknown_fact_and_unsigned_claim() {
    let f = Fixture::new();
    assert!(
        f.validator
            .resolve(
                Uuid::new_v4(),
                f.envelope.payload.task_id,
                1,
                &f.fact,
                "approved:fixture-only",
                150
            )
            .is_err()
    );
    assert!(
        f.validator
            .resolve(
                f.envelope.payload.workspace_id,
                Uuid::new_v4(),
                1,
                &f.fact,
                "approved:fixture-only",
                150
            )
            .is_err()
    );
    assert!(
        f.validator
            .resolve(
                f.envelope.payload.workspace_id,
                f.envelope.payload.task_id,
                2,
                &f.fact,
                "approved:fixture-only",
                150
            )
            .is_err()
    );
    assert!(
        f.validator
            .resolve(
                f.envelope.payload.workspace_id,
                f.envelope.payload.task_id,
                1,
                &f.fact,
                "foreign-ref",
                150
            )
            .is_err()
    );
    let mut unknown = Fixture::new();
    unknown.fact.path = "/unknown".into();
    unknown.validator.policy.entries[0].binding.fact_path = "/unknown".into();
    assert!(unknown.resolve(150).is_err());
    let mut forged = Fixture::new();
    forged.validator.policy.verifier_public_key_hex =
        hex(Ed25519KeyPair::from_seed_unchecked(&[18; 32])
            .unwrap()
            .public_key()
            .as_ref());
    assert!(forged.resolve(150).is_err());
}
#[test]
fn rejects_source_subject_policy_and_typed_fact_drift() {
    for field in ["source", "subject", "policy", "fact", "workspace", "signer"] {
        let mut f = Fixture::new();
        match field {
            "source" => f.envelope.payload.source = "drift".into(),
            "subject" => f.envelope.payload.subject = "drift".into(),
            "policy" => f.envelope.payload.policy_version = "drift".into(),
            "fact" => f.envelope.payload.input.criticality = known("changed".into()),
            "workspace" => f.envelope.payload.workspace_id = Uuid::new_v4(),
            _ => f.envelope.payload.verifier_principal = Uuid::new_v4(),
        }
        f.write_artifact();
        assert!(f.resolve(150).is_err(), "{field}");
    }
}
#[test]
fn rejects_tamper_symlink_hardlink_writable_and_policy_mutation() {
    for attack in ["tamper", "symlink", "hardlink", "writable", "policy"] {
        let f = Fixture::new();
        let path = f.validator.policy.artifact_root.join("evidence.json");
        match attack {
            "tamper" => {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                fs::write(&path, b"{}").unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
            }
            "symlink" => {
                fs::rename(&path, path.with_extension("original")).unwrap();
                symlink(path.with_extension("original"), &path).unwrap();
            }
            "hardlink" => fs::hard_link(&path, path.with_extension("link")).unwrap(),
            "writable" => fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap(),
            _ => fs::write(&f.validator.policy_path, b"changed-policy").unwrap(),
        }
        assert!(f.resolve(150).is_err(), "{attack}");
    }
}
#[tokio::test]
async fn local_signature_never_bypasses_actual_store_authentication() {
    let f = Fixture::new();
    assert!(f.resolve(150).is_ok());
    assert!(
        f.validator
            .validate(
                f.envelope.payload.workspace_id,
                f.envelope.payload.task_id,
                1,
                &f.fact,
                "approved:fixture-only",
                150
            )
            .await
            .is_err()
    );
    let binding = f.resolve(150).unwrap();
    assert!(
        f.validator
            .revalidate(
                f.envelope.payload.workspace_id,
                f.envelope.payload.task_id,
                1,
                &f.fact,
                &binding,
                150
            )
            .await
            .is_err()
    );
}

#[test]
fn existing_binding_checks_every_persisted_field_again() {
    let f = Fixture::new();
    let binding = f.resolve(150).unwrap();
    assert!(
        f.validator
            .resolve_existing(
                f.envelope.payload.workspace_id,
                f.envelope.payload.task_id,
                1,
                &f.fact,
                &binding,
                150
            )
            .is_ok()
    );
    for field in [
        "fact_path",
        "value_digest",
        "evidence_ref",
        "content_digest",
        "source",
        "subject",
        "observed_at",
        "expires_at",
        "validation_outcome",
    ] {
        let mut changed = serde_json::to_value(&binding).unwrap();
        changed[field] = match field {
            "observed_at" | "expires_at" => serde_json::json!(151),
            "validation_outcome" => serde_json::json!("rejected"),
            _ => serde_json::json!("drift"),
        };
        let changed: MatrixEvidenceBinding = serde_json::from_value(changed).unwrap();
        assert!(
            f.validator
                .resolve_existing(
                    f.envelope.payload.workspace_id,
                    f.envelope.payload.task_id,
                    1,
                    &f.fact,
                    &changed,
                    150
                )
                .is_err(),
            "{field}"
        );
    }
    assert!(
        f.validator
            .resolve_existing(
                f.envelope.payload.workspace_id,
                f.envelope.payload.task_id,
                1,
                &f.fact,
                &binding,
                200
            )
            .is_err()
    );
}
#[test]
fn constructor_loads_exact_private_policy_and_rejects_mutation() {
    let f = Fixture::new();
    fs::write(
        &f.validator.policy_path,
        serde_json::to_vec(&f.validator.policy).unwrap(),
    )
    .unwrap();
    let loaded = PrivateMatrixEvidenceValidator::from_file(
        f.validator.policy_path.clone(),
        Arc::new(NeverStore),
    )
    .unwrap();
    assert!(
        loaded
            .resolve(
                f.envelope.payload.workspace_id,
                f.envelope.payload.task_id,
                1,
                &f.fact,
                "approved:fixture-only",
                150
            )
            .is_ok()
    );
    fs::set_permissions(&f.validator.policy_path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        PrivateMatrixEvidenceValidator::from_file(
            f.validator.policy_path.clone(),
            Arc::new(NeverStore)
        )
        .is_err()
    );
}
#[test]
fn rejects_nested_ignored_unknown_fields_even_with_pinned_file_hash() {
    let mut f = Fixture::new();
    let path = f.validator.policy.artifact_root.join("evidence.json");
    let mut value = serde_json::to_value(&f.envelope).unwrap();
    value["payload"]["input"]["criticality"]["ignored_claim"] = serde_json::json!("accepted");
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&path, &bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    f.validator.policy.entries[0].binding.content_digest = hash(&bytes);
    assert!(f.resolve(150).is_err());
}
