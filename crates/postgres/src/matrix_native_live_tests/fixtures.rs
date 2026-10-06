use super::*;

pub(super) fn context(auth: &HostAuth, key: &str) -> RequestContext {
    RequestContext {
        auth: auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: key.into(),
    }
}

fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: FactProvenance("synthetic-operating-source".into()),
    }
}

fn operating_input() -> EngineeringMatrixInput {
    EngineeringMatrixInput {
        mode: MatrixFact::Absent,
        intent: MatrixFact::Absent,
        urgency: MatrixFact::Absent,
        promised_behavior: MatrixFact::Absent,
        promised_proof: MatrixFact::Absent,
        envelope: OperatingEnvelope {
            scale: known("observed limited workload".into()),
            operational_facts: OperationalFacts::KnownEmpty {
                provenance: FactProvenance("synthetic-operating-source".into()),
            },
        },
        criticality: known("limited".into()),
        affected_guarantees: MatrixFact::KnownEmpty {
            provenance: FactProvenance("synthetic-operating-source".into()),
        },
        actual_exposure: known(false),
        demand_commitment: MatrixFact::Absent,
        latency_commitment: MatrixFact::Absent,
        urgent_repair: known(false),
    }
}

fn declarations() -> Vec<DeclaredRequirementValue> {
    vec![
        DeclaredRequirementValue::Mode(EngineeringMode::Mvp),
        DeclaredRequirementValue::Intent(EngineeringIntent::Other("build".into())),
        DeclaredRequirementValue::Urgency("normal".into()),
        DeclaredRequirementValue::PromisedBehavior("booking".into()),
        DeclaredRequirementValue::PromisedProof("regression".into()),
        DeclaredRequirementValue::NoDemandCommitment,
        DeclaredRequirementValue::NoLatencyCommitment,
    ]
}

pub(super) fn service(store: Arc<PgStore>) -> WorkspaceService {
    WorkspaceService::new(
        store,
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    )
}

pub(super) async fn bound_source(
    service: &WorkspaceService,
    pool: &PgPool,
    owner: &admin::Enrollment,
    context: &RequestContext,
    workspace: Uuid,
) -> (Uuid, MatrixTaskSource, EffectiveMatrixRequirements) {
    let program = Uuid::new_v4();
    // Smallest existing synthetic Program fixture; Matrix context/task writes use public APIs.
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',4,'p','i','b','finite','c','s','ready',2,2,4096)")
        .bind(program).bind(owner.tenant_id).bind(workspace).execute(pool).await.unwrap();
    let locator = MatrixRequirementsLocator::Program {
        program_id: program,
    };
    let proposal = service
        .propose_matrix_requirements_context(
            context,
            &ProposeMatrixRequirementsContext {
                request_id: Uuid::new_v4(),
                locator: locator.clone(),
                expected_context_revision: 0,
                patches: declarations()
                    .into_iter()
                    .map(|value| RequirementDeclarationPatch::Set { value })
                    .collect(),
            },
        )
        .await
        .unwrap();
    assert_eq!(proposal.proposal.revision(), 1);
    let confirmation = service
        .confirm_matrix_requirements_context(
            context,
            &ConfirmMatrixRequirementsContext {
                request_id: Uuid::new_v4(),
                locator: locator.clone(),
                proposal_revision: proposal.proposal.revision(),
                proposal_digest: proposal.proposal.digest().into(),
                owner_response_ref: format!("synthetic-owner-response-{program}"),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        confirmation.confirmation.owner_principal(),
        owner.principal_id.to_string()
    );
    let effective = service
        .get_effective_matrix_requirements_context(context, &locator)
        .await
        .unwrap();
    assert_eq!(effective.schema(), MATRIX_REQUIREMENTS_SCHEMA);
    assert_eq!(effective.program_id(), program);
    assert_eq!(effective.values().len(), 7);
    for expected in declarations() {
        let resolved = effective
            .values()
            .values()
            .find(|value| value.value == expected)
            .unwrap();
        assert_eq!(
            resolved.source.owner_principal,
            owner.principal_id.to_string()
        );
        assert_eq!(resolved.source.proposal_revision, 1);
        assert_eq!(resolved.source.proposal_digest, proposal.proposal.digest());
    }
    let task = Uuid::new_v4();
    let request = RecordMatrixTask {
        task_id: task,
        revision: 1,
        expected_current_revision: 0,
        request_id: Uuid::new_v4(),
        input: operating_input(),
        choice_set: Some(EngineeringChoiceSet {
            schema: MATRIX_CHOICE_SET_SCHEMA.into(),
            choice_set_id: format!("synthetic-choice-{task}"),
            version: 1,
            task_id: task.to_string(),
            task_revision: "1".into(),
            decision_question: "Which approach?".into(),
            candidates: ["a", "b"]
                .into_iter()
                .map(|id| EngineeringCandidate {
                    candidate_id: id.into(),
                    title: id.into(),
                    approach: id.into(),
                    assumption_fact_ids: vec![],
                })
                .collect(),
        }),
    };
    let saved = service
        .record_matrix_task_with_requirements(context, &request, &locator)
        .await
        .unwrap();
    assert_eq!(
        service
            .record_matrix_task_with_requirements(context, &request, &locator)
            .await
            .unwrap(),
        saved
    );
    let read = service.get_matrix_task_source(context, task).await.unwrap();
    assert_eq!(read, saved);
    assert_eq!(saved.revision.revision, 1);
    assert_eq!(saved.revision.request_id, request.request_id);
    assert_eq!(saved.revision.recorded_by_principal_id, owner.principal_id);
    assert_eq!(
        saved.revision.input,
        bind_matrix_requirements_input(&effective, &request.input).unwrap()
    );
    assert_eq!(
        saved.revision.input_digest,
        tect_application::canonical_matrix_input_digest(
            &serde_json::to_value(&saved.revision.input).unwrap()
        )
        .unwrap()
    );
    let choice = saved.revision.choice_set.as_ref().unwrap();
    assert_eq!(choice.candidates.len(), 2);
    assert_eq!(
        saved.revision.choice_set_digest.as_deref(),
        Some(
            choice
                .canonical_digest(&saved.revision.input)
                .unwrap()
                .as_str()
        )
    );
    let binding = saved.requirements_binding.as_ref().unwrap();
    assert_eq!(binding.locator, locator);
    assert!(!binding.snapshot_id.is_nil());
    assert_eq!(binding.authority_schema, MATRIX_REQUIREMENTS_SCHEMA);
    assert_eq!(binding.semantic_digest, effective.semantic_digest());
    (program, saved, effective)
}

#[derive(Clone)]
pub(super) struct EvidenceCase {
    pub(super) workspace: Uuid,
    pub(super) task: Uuid,
    pub(super) revision: i64,
    pub(super) binding: MatrixEvidenceBinding,
}

pub(super) struct SyntheticValidator {
    pub(super) cases: BTreeMap<String, EvidenceCase>,
}

#[async_trait]
impl MatrixEvidenceValidator for SyntheticValidator {
    fn policy_version(&self) -> &str {
        "synthetic-matrix-registry/1"
    }

    async fn validate(
        &self,
        workspace: Uuid,
        task: Uuid,
        revision: i64,
        fact: &RequiredMatrixFact,
        reference: &str,
        now: i64,
    ) -> Result<MatrixEvidenceBinding> {
        let case = self.cases.get(reference).ok_or(Error::Forbidden)?;
        if case.workspace != workspace
            || case.task != task
            || case.revision != revision
            || case.binding.fact_path != fact.path
            || case.binding.value_digest != fact.value_digest
            || case.binding.evidence_ref != reference
            || now < case.binding.observed_at
            || now >= case.binding.expires_at
        {
            return Err(Error::Forbidden);
        }
        Ok(case.binding.clone())
    }

    async fn revalidate(
        &self,
        workspace: Uuid,
        task: Uuid,
        revision: i64,
        fact: &RequiredMatrixFact,
        binding: &MatrixEvidenceBinding,
        now: i64,
    ) -> Result<()> {
        let expected = self
            .validate(workspace, task, revision, fact, &binding.evidence_ref, now)
            .await?;
        if expected != *binding {
            return Err(Error::Forbidden);
        }
        Ok(())
    }
}

pub(super) struct SyntheticProvider {
    pub(super) identity: MatrixProviderIdentity,
    pub(super) prepares: Arc<AtomicUsize>,
    pub(super) sends: Arc<AtomicUsize>,
}

#[async_trait]
impl MatrixAdviceProvider for SyntheticProvider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        Some(self.identity.clone())
    }

    fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
        self.prepares.fetch_add(1, Ordering::SeqCst);
        PreparedMatrixAdviceAttempt::new(
            request,
            self.identity.clone(),
            b"synthetic-bound-no-call-body".to_vec(),
        )
    }

    async fn attempt_prepared(
        &self,
        _: PreparedMatrixAdviceAttempt,
        _: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        panic!("synthetic Matrix NoCall must never enter provider transport")
    }
}

pub(super) async fn counts(
    pool: &PgPool,
    tenant: Uuid,
    workspace: Uuid,
    task: Uuid,
) -> (i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM matrix_task_revisions WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3), (SELECT count(*) FROM matrix_verifications WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3), (SELECT count(*) FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND work_item_kind='matrix_task' AND work_item_id=$3), (SELECT count(*) FROM advisory_dispatch d JOIN advisory_opportunity o ON (o.tenant_id,o.workspace_id,o.id)=(d.tenant_id,d.workspace_id,d.opportunity_id) WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.work_item_kind='matrix_task' AND o.work_item_id=$3)")
        .bind(tenant).bind(workspace).bind(task).fetch_one(pool).await.unwrap()
}
