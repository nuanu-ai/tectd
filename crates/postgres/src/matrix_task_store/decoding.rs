use super::*;

pub(super) fn same_request(
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

pub(super) fn decode_revision(row: PgRow) -> Result<MatrixTaskRevision> {
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

pub(super) fn decode_choice_set(
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

pub(super) fn decode_input(
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

fn decode_locator(value: serde_json::Value) -> Result<MatrixRequirementsLocator> {
    let uuid = |key: &str| -> Result<Uuid> {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .ok_or(Error::InternalInvariant)?
            .parse()
            .map_err(|_| Error::InternalInvariant)
    };
    let locator = match value.get("level").and_then(serde_json::Value::as_str) {
        Some("program") => MatrixRequirementsLocator::Program {
            program_id: uuid("program_id")?,
        },
        Some("scope") => MatrixRequirementsLocator::Scope {
            program_id: uuid("program_id")?,
            scope_id: uuid("scope_id")?,
        },
        Some("slice") => MatrixRequirementsLocator::Slice {
            program_id: uuid("program_id")?,
            scope_id: uuid("scope_id")?,
            candidate_set_id: uuid("candidate_set_id")?,
            work_candidate_id: uuid("work_candidate_id")?,
            expected_work_revision: value
                .get("expected_work_revision")
                .and_then(serde_json::Value::as_i64)
                .ok_or(Error::InternalInvariant)?,
        },
        Some("opened_slice") => MatrixRequirementsLocator::OpenedSlice {
            slice_id: uuid("slice_id")?,
        },
        _ => return Err(Error::InternalInvariant),
    };
    if locator.as_json() != value {
        return Err(Error::InternalInvariant);
    }
    Ok(locator)
}

pub(super) fn decode_binding(row: PgRow) -> Result<(MatrixTaskRequirementsBinding, String)> {
    let binding = MatrixTaskRequirementsBinding {
        locator: decode_locator(row.try_get("requirements_locator").map_err(storage_error)?)?,
        snapshot_id: row.try_get("snapshot_id").map_err(storage_error)?,
        semantic_digest: row.try_get("semantic_digest").map_err(storage_error)?,
        authority_schema: row.try_get("authority_schema").map_err(storage_error)?,
    };
    let digest = row
        .try_get("original_request_digest")
        .map_err(storage_error)?;
    Ok((binding, digest))
}

pub(super) fn matrix_write_error(error: sqlx::Error) -> Error {
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
