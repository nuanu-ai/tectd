use async_trait::async_trait;
use sqlx::{Row, postgres::PgRow};
use tect_application::{
    MATRIX_INPUT_SCHEMA, MatrixTaskRevision, MatrixTaskStore, RecordMatrixTask,
    canonical_matrix_input_digest,
};
use tect_domain::{
    EngineeringChoiceSet, EngineeringMatrixInput, Error, MATRIX_CHOICE_SET_SCHEMA, Result,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

impl PgUnitOfWork {
    async fn matrix_receipt_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<MatrixTaskRevision>> {
        let tenant_id = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT task_id,revision,request_id,input_schema,canonical_input,input_digest,choice_set_schema,choice_set,choice_set_digest, \
                    recorded_by_principal_id,recorded_by_session_id \
             FROM matrix_task_revisions \
             WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(request_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(decode_revision).transpose()
    }

    async fn matrix_retry_or_error(
        &mut self,
        workspace_id: Uuid,
        request: &RecordMatrixTask,
        canonical_input: &serde_json::Value,
        input_digest: &str,
        fallback: Error,
    ) -> Result<MatrixTaskRevision> {
        match self
            .matrix_receipt_by_request(workspace_id, request.request_id)
            .await?
        {
            Some(prior) if same_request(&prior, request, canonical_input, input_digest)? => {
                Ok(prior)
            }
            Some(_) => Err(Error::InputConflict),
            None => Err(fallback),
        }
    }
}

fn same_request(
    prior: &MatrixTaskRevision,
    request: &RecordMatrixTask,
    canonical_input: &serde_json::Value,
    input_digest: &str,
) -> Result<bool> {
    Ok(prior.task_id == request.task_id
        && prior.revision == request.revision
        && prior.input_digest == input_digest
        && serde_json::to_value(&prior.input).map_err(storage_error)? == *canonical_input
        && prior.choice_set == request.choice_set
        && prior.choice_set_digest
            == request
                .choice_set
                .as_ref()
                .map(|choice| choice.canonical_digest(&request.input))
                .transpose()?)
}

fn decode_revision(row: PgRow) -> Result<MatrixTaskRevision> {
    let schema: String = row.try_get("input_schema").map_err(storage_error)?;
    if schema != MATRIX_INPUT_SCHEMA {
        return Err(Error::InternalInvariant);
    }
    let canonical_input: serde_json::Value =
        row.try_get("canonical_input").map_err(storage_error)?;
    let input_digest: String = row.try_get("input_digest").map_err(storage_error)?;
    let input = decode_input(canonical_input, &input_digest)?;
    let task_id: Uuid = row.try_get("task_id").map_err(storage_error)?;
    let revision: i64 = row.try_get("revision").map_err(storage_error)?;
    let choice_set_schema: Option<String> =
        row.try_get("choice_set_schema").map_err(storage_error)?;
    let choice_json: Option<serde_json::Value> =
        row.try_get("choice_set").map_err(storage_error)?;
    let choice_set_digest: Option<String> =
        row.try_get("choice_set_digest").map_err(storage_error)?;
    let choice_set = decode_choice_set(
        choice_set_schema,
        choice_json,
        choice_set_digest.as_deref(),
        task_id,
        revision,
        &input,
    )?;
    Ok(MatrixTaskRevision {
        task_id,
        revision,
        request_id: row.try_get("request_id").map_err(storage_error)?,
        input,
        input_digest,
        choice_set,
        choice_set_digest,
        recorded_by_principal_id: row
            .try_get("recorded_by_principal_id")
            .map_err(storage_error)?,
        recorded_by_session_id: row
            .try_get("recorded_by_session_id")
            .map_err(storage_error)?,
    })
}

fn decode_choice_set(
    schema: Option<String>,
    json: Option<serde_json::Value>,
    digest: Option<&str>,
    task_id: Uuid,
    revision: i64,
    input: &EngineeringMatrixInput,
) -> Result<Option<EngineeringChoiceSet>> {
    match (schema, json, digest) {
        (None, None, None) => Ok(None),
        (Some(schema), Some(json), Some(digest)) if schema == MATRIX_CHOICE_SET_SCHEMA => {
            let choice: EngineeringChoiceSet =
                serde_json::from_value(json.clone()).map_err(|_| Error::InternalInvariant)?;
            if choice.task_id != task_id.to_string()
                || choice.task_revision != revision.to_string()
                || choice.schema != schema
                || serde_json::to_value(&choice).map_err(|_| Error::InternalInvariant)? != json
                || choice
                    .canonical_digest(input)
                    .map_err(|_| Error::InternalInvariant)?
                    != digest
            {
                return Err(Error::InternalInvariant);
            }
            Ok(Some(choice))
        }
        _ => Err(Error::InternalInvariant),
    }
}

fn decode_input(
    canonical_input: serde_json::Value,
    input_digest: &str,
) -> Result<EngineeringMatrixInput> {
    if canonical_matrix_input_digest(&canonical_input)? != input_digest {
        return Err(Error::InternalInvariant);
    }
    let input: EngineeringMatrixInput =
        serde_json::from_value(canonical_input).map_err(storage_error)?;
    input.validate().map_err(|_| Error::InternalInvariant)?;
    Ok(input)
}

fn matrix_write_error(error: sqlx::Error) -> Error {
    if error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code.as_ref() == "42501")
    {
        Error::Forbidden
    } else {
        storage_error(error)
    }
}

#[async_trait]
impl MatrixTaskStore for PgUnitOfWork {
    async fn record_matrix_task(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &RecordMatrixTask,
        canonical_input: &serde_json::Value,
        input_digest: &str,
    ) -> Result<MatrixTaskRevision> {
        let tenant_id = self.tenant_id()?;
        let choice_set_json = request
            .choice_set
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| Error::InvalidArguments)?;
        let choice_set_digest = request
            .choice_set
            .as_ref()
            .map(|choice| choice.canonical_digest(&request.input))
            .transpose()?;
        if self.principal_id()? != principal_id {
            return Err(Error::Forbidden);
        }
        if let Some(prior) = self
            .matrix_receipt_by_request(workspace_id, request.request_id)
            .await?
        {
            return if same_request(&prior, request, canonical_input, input_digest)? {
                Ok(prior)
            } else {
                Err(Error::InputConflict)
            };
        }
        if request.revision == 1 {
            let inserted = sqlx::query(
                "INSERT INTO matrix_tasks (tenant_id,workspace_id,id,current_revision) \
                 VALUES ($1,$2,$3,1) ON CONFLICT DO NOTHING",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(request.task_id)
            .execute(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            if inserted.rows_affected() != 1 {
                return self
                    .matrix_retry_or_error(
                        workspace_id,
                        request,
                        canonical_input,
                        input_digest,
                        Error::StaleRevision,
                    )
                    .await;
            }
        } else {
            let current: Option<i64> = sqlx::query_scalar(
                "SELECT current_revision FROM matrix_tasks \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(request.task_id)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            if current != Some(request.expected_current_revision) {
                return self
                    .matrix_retry_or_error(
                        workspace_id,
                        request,
                        canonical_input,
                        input_digest,
                        Error::StaleRevision,
                    )
                    .await;
            }
        }

        let inserted = sqlx::query(
            "INSERT INTO matrix_task_revisions (tenant_id,workspace_id,task_id,revision,previous_revision,request_id,input_schema,canonical_input,input_digest,recorded_by_principal_id,recorded_by_session_id,choice_set_schema,choice_set,choice_set_digest) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) ON CONFLICT DO NOTHING",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(request.task_id)
        .bind(request.revision)
        .bind((request.revision > 1).then_some(request.expected_current_revision))
        .bind(request.request_id)
        .bind(MATRIX_INPUT_SCHEMA)
        .bind(canonical_input)
        .bind(input_digest)
        .bind(principal_id)
        .bind(session_id)
        .bind(request.choice_set.as_ref().map(|_| MATRIX_CHOICE_SET_SCHEMA))
        .bind(&choice_set_json)
        .bind(&choice_set_digest)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(matrix_write_error)?;
        if inserted.rows_affected() != 1 {
            return self
                .matrix_retry_or_error(
                    workspace_id,
                    request,
                    canonical_input,
                    input_digest,
                    Error::InputConflict,
                )
                .await;
        }

        if request.revision > 1 {
            let advanced = sqlx::query(
                "UPDATE matrix_tasks SET current_revision=$4 \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND current_revision=$5",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(request.task_id)
            .bind(request.revision)
            .bind(request.expected_current_revision)
            .execute(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            if advanced.rows_affected() != 1 {
                return Err(Error::StaleRevision);
            }
        }

        Ok(MatrixTaskRevision {
            task_id: request.task_id,
            revision: request.revision,
            request_id: request.request_id,
            input: request.input.clone(),
            input_digest: input_digest.to_owned(),
            choice_set: request.choice_set.clone(),
            choice_set_digest,
            recorded_by_principal_id: principal_id,
            recorded_by_session_id: session_id,
        })
    }

    async fn matrix_task(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<MatrixTaskRevision>> {
        let tenant_id = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT r.task_id,r.revision,r.request_id,r.input_schema,r.canonical_input,r.input_digest,r.choice_set_schema,r.choice_set,r.choice_set_digest, \
                    r.recorded_by_principal_id,r.recorded_by_session_id \
             FROM matrix_tasks AS t JOIN matrix_task_revisions AS r \
               ON r.tenant_id=t.tenant_id AND r.workspace_id=t.workspace_id \
               AND r.task_id=t.id AND r.revision=t.current_revision \
             WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.id=$3",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(task_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(decode_revision).transpose()
    }

    async fn lock_matrix_task(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<MatrixTaskRevision>> {
        let tenant_id = self.tenant_id()?;
        let head: Option<i64> = sqlx::query_scalar(
            "SELECT current_revision FROM matrix_tasks WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(task_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        match head {
            Some(revision) => {
                let current = self.matrix_task(workspace_id, task_id).await?;
                if current
                    .as_ref()
                    .is_none_or(|value| value.revision != revision)
                {
                    return Err(Error::InternalInvariant);
                }
                Ok(current)
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{EngineeringCandidate, MatrixFact, OperatingEnvelope, OperationalFacts};

    fn input() -> EngineeringMatrixInput {
        EngineeringMatrixInput {
            mode: MatrixFact::Absent,
            envelope: OperatingEnvelope {
                scale: MatrixFact::Absent,
                operational_facts: OperationalFacts::Absent,
            },
            criticality: MatrixFact::Absent,
            intent: MatrixFact::Absent,
            urgency: MatrixFact::Absent,
            promised_behavior: MatrixFact::Absent,
            promised_proof: MatrixFact::Absent,
            affected_guarantees: MatrixFact::Absent,
            actual_exposure: MatrixFact::Absent,
            demand_commitment: MatrixFact::Absent,
            latency_commitment: MatrixFact::Absent,
            urgent_repair: MatrixFact::Absent,
        }
    }

    fn choice(task_id: Uuid, revision: i64) -> EngineeringChoiceSet {
        EngineeringChoiceSet {
            schema: MATRIX_CHOICE_SET_SCHEMA.into(),
            choice_set_id: "choice-1".into(),
            version: 1,
            task_id: task_id.to_string(),
            task_revision: revision.to_string(),
            decision_question: "Which approach?".into(),
            candidates: vec![EngineeringCandidate {
                candidate_id: "a".into(),
                title: "A".into(),
                approach: "Use A".into(),
                assumption_fact_ids: vec!["criticality".into()],
            }],
        }
    }

    #[test]
    fn retry_matches_exact_record_only() {
        let input = input();
        let canonical = serde_json::to_value(&input).unwrap();
        let digest = canonical_matrix_input_digest(&canonical).unwrap();
        let task_id = Uuid::new_v4();
        let request_id = Uuid::new_v4();
        let request = RecordMatrixTask {
            task_id,
            revision: 2,
            expected_current_revision: 1,
            request_id,
            input: input.clone(),
            choice_set: None,
        };
        let prior = MatrixTaskRevision {
            task_id,
            revision: 2,
            request_id,
            input,
            input_digest: digest.clone(),
            choice_set: None,
            choice_set_digest: None,
            recorded_by_principal_id: Uuid::new_v4(),
            recorded_by_session_id: Uuid::new_v4(),
        };
        assert!(same_request(&prior, &request, &canonical, &digest).unwrap());
        assert!(!same_request(&prior, &request, &canonical, &"0".repeat(64)).unwrap());
        let mut changed_canonical = canonical.clone();
        changed_canonical["mode"] = serde_json::json!({"state": "unknown", "provenance": "other"});
        assert!(!same_request(&prior, &request, &changed_canonical, &digest).unwrap());
        let mut changed_task = request.clone();
        changed_task.task_id = Uuid::new_v4();
        assert!(!same_request(&prior, &changed_task, &canonical, &digest).unwrap());
        let mut changed_revision = request;
        changed_revision.revision += 1;
        assert!(!same_request(&prior, &changed_revision, &canonical, &digest).unwrap());
    }

    #[test]
    fn read_rejects_digest_mismatch() {
        let canonical = serde_json::to_value(input()).unwrap();
        let digest = canonical_matrix_input_digest(&canonical).unwrap();
        assert!(decode_input(canonical.clone(), &digest).is_ok());
        assert_eq!(
            decode_input(canonical, &"0".repeat(64)),
            Err(Error::InternalInvariant)
        );
    }

    #[test]
    fn choice_read_checks_digest_schema_and_binding() {
        let input = input();
        let task_id = Uuid::new_v4();
        let choice = choice(task_id, 2);
        let json = serde_json::to_value(&choice).unwrap();
        let digest = choice.canonical_digest(&input).unwrap();
        assert_eq!(
            decode_choice_set(
                Some(MATRIX_CHOICE_SET_SCHEMA.into()),
                Some(json.clone()),
                Some(&digest),
                task_id,
                2,
                &input
            ),
            Ok(Some(choice.clone()))
        );
        assert_eq!(
            decode_choice_set(None, None, None, task_id, 2, &input),
            Ok(None)
        );
        assert_eq!(
            decode_choice_set(
                Some(MATRIX_CHOICE_SET_SCHEMA.into()),
                Some(json.clone()),
                Some(&"0".repeat(64)),
                task_id,
                2,
                &input
            ),
            Err(Error::InternalInvariant)
        );
        assert_eq!(
            decode_choice_set(
                Some("wrong".into()),
                Some(json.clone()),
                Some(&digest),
                task_id,
                2,
                &input
            ),
            Err(Error::InternalInvariant)
        );
        assert_eq!(
            decode_choice_set(
                Some(MATRIX_CHOICE_SET_SCHEMA.into()),
                Some(json),
                Some(&digest),
                task_id,
                3,
                &input
            ),
            Err(Error::InternalInvariant)
        );
    }

    #[test]
    fn retry_compares_choice_set_even_when_facts_match() {
        let input = input();
        let canonical = serde_json::to_value(&input).unwrap();
        let digest = canonical_matrix_input_digest(&canonical).unwrap();
        let task_id = Uuid::new_v4();
        let choice_set = choice(task_id, 1);
        let prior = MatrixTaskRevision {
            task_id,
            revision: 1,
            request_id: Uuid::new_v4(),
            input: input.clone(),
            input_digest: digest.clone(),
            choice_set: Some(choice_set.clone()),
            choice_set_digest: Some(choice_set.canonical_digest(&input).unwrap()),
            recorded_by_principal_id: Uuid::new_v4(),
            recorded_by_session_id: Uuid::new_v4(),
        };
        let mut request = RecordMatrixTask {
            task_id,
            revision: 1,
            expected_current_revision: 0,
            request_id: prior.request_id,
            input,
            choice_set: Some(choice_set),
        };
        assert!(same_request(&prior, &request, &canonical, &digest).unwrap());
        request.choice_set.as_mut().unwrap().decision_question = "A different question?".into();
        assert!(!same_request(&prior, &request, &canonical, &digest).unwrap());
        request.choice_set = None;
        assert!(!same_request(&prior, &request, &canonical, &digest).unwrap());
    }
}
