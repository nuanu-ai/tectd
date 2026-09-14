use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{
    Error, NewProgramInput, Program, ProgramCursor, ProgramInput, ProgramStatus, ProgramStep,
    ProgramSummary, Result, validate_program_input,
};
use uuid::Uuid;

const PROGRAM_COLUMNS: &str = "id, workspace_id, status, revision, name, intent, basis, \
    boundaries, constraints, success, working_notes, pending_question, current_step, \
    input_cursor, latest_input, max_input_bytes, payload_erased";

#[derive(sqlx::FromRow)]
struct ProgramRow {
    id: Uuid,
    workspace_id: Uuid,
    status: String,
    revision: i64,
    name: Option<String>,
    intent: Option<String>,
    basis: Option<String>,
    boundaries: Option<String>,
    constraints: Option<String>,
    success: Option<String>,
    working_notes: Option<String>,
    pending_question: Option<String>,
    current_step: String,
    input_cursor: i64,
    latest_input: i64,
    max_input_bytes: i64,
    payload_erased: bool,
}

pub(crate) async fn ensure_program(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    session_id: Uuid,
    input: &NewProgramInput,
) -> Result<Program> {
    validate_input(input)?;
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
             pg_catalog.hashtextextended(\
                 $1::text || ':' || $2::text || ':' || $3::text, 0))",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(input.request_id)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;

    let existing: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT program_id, input FROM program_inputs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3 AND sequence=1",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(input.request_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if let Some((program_id, stored_input)) = existing {
        if stored_input != input.input {
            return Err(Error::InputConflict);
        }
        return program(transaction, tenant_id, workspace_id, program_id, false)
            .await?
            .ok_or(Error::StorageUnavailable);
    }

    let id: Uuid = sqlx::query_scalar("SELECT pg_catalog.gen_random_uuid()")
        .fetch_one(&mut **transaction)
        .await
        .map_err(storage_error)?;
    let program = Program::draft(id, workspace_id, input.encoded_bytes);
    insert_program(transaction, tenant_id, &program).await?;
    insert_program_input(
        transaction,
        tenant_id,
        workspace_id,
        id,
        session_id,
        1,
        input,
    )
    .await?;
    Ok(program)
}

pub(crate) async fn program(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    program_id: Uuid,
    for_update: bool,
) -> Result<Option<Program>> {
    crate::planning_knowledge::require_owned_payload_identity(
        transaction,
        tenant_id,
        workspace_id,
        &["programs"],
        Some(program_id),
    )
    .await?;
    let query = format!(
        "SELECT {PROGRAM_COLUMNS} FROM programs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3{}",
        if for_update { " FOR UPDATE" } else { "" }
    );
    let row = sqlx::query_as::<_, ProgramRow>(&query)
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(program_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(storage_error)?;
    row.map(ProgramRow::into_domain).transpose()
}

pub(crate) async fn program_input(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    program_id: Uuid,
    request_id: Uuid,
) -> Result<Option<ProgramInput>> {
    let row: Option<(Uuid, i64, Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, sequence, request_id, session_id, input FROM program_inputs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND program_id=$3 AND request_id=$4",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(program_id)
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(row.map(input_from_row))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_program_input(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    program_id: Uuid,
    session_id: Uuid,
    sequence: i64,
    input: &NewProgramInput,
) -> Result<ProgramInput> {
    validate_input(input)?;
    if sequence < 1 {
        return Err(Error::InvalidArguments);
    }
    let row: (Uuid, i64, Uuid, Uuid, String) = sqlx::query_as(
        "INSERT INTO program_inputs \
             (tenant_id, workspace_id, program_id, sequence, request_id, session_id, input) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id, sequence, request_id, session_id, input",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(program_id)
    .bind(sequence)
    .bind(input.request_id)
    .bind(session_id)
    .bind(&input.input)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(input_from_row(row))
}

pub(crate) async fn update_program(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    program: &Program,
) -> Result<()> {
    let result = sqlx::query(
        "UPDATE programs SET status=$4, revision=$5, name=$6, intent=$7, basis=$8, \
             boundaries=$9, constraints=$10, success=$11, working_notes=$12, \
             pending_question=$13, current_step=$14, input_cursor=$15, \
             latest_input=$16, max_input_bytes=$17 \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant_id)
    .bind(program.workspace_id)
    .bind(program.id)
    .bind(status_name(program.status))
    .bind(program.revision)
    .bind(&program.name)
    .bind(&program.intent)
    .bind(&program.basis)
    .bind(&program.boundaries)
    .bind(&program.constraints)
    .bind(&program.success)
    .bind(&program.working_notes)
    .bind(&program.pending_question)
    .bind(step_name(program.current_step))
    .bind(program.input_cursor)
    .bind(program.latest_input)
    .bind(program.max_input_bytes)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if result.rows_affected() != 1 {
        return Err(Error::NotFound);
    }
    sqlx::query(
        "INSERT INTO knowledge_owned_copies \
         (id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision,row_revision) \
         SELECT pg_catalog.gen_random_uuid(),tenant_id,workspace_id,unit_id,'planning_derived', \
                'programs',row_id,source_revision,$4 \
         FROM (SELECT DISTINCT tenant_id,workspace_id,unit_id,row_id,source_revision \
           FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 \
             AND relation_name='programs' AND row_id=$3 AND NOT redacted) copies \
         ON CONFLICT DO NOTHING",
    )
    .bind(tenant_id)
    .bind(program.workspace_id)
    .bind(program.id)
    .bind(program.revision)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(crate) async fn program_inputs(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    program_id: Uuid,
    after: i64,
    limit: u32,
) -> Result<Vec<ProgramInput>> {
    let rows: Vec<(Uuid, i64, Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, sequence, request_id, session_id, input FROM program_inputs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND program_id=$3 AND sequence>$4 \
         ORDER BY sequence LIMIT $5",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(program_id)
    .bind(after)
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(rows.into_iter().map(input_from_row).collect())
}

pub(crate) async fn list_programs(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    after: Option<ProgramCursor>,
    limit: u32,
) -> Result<Vec<ProgramSummary>> {
    crate::planning_knowledge::require_owned_payload_identity(
        transaction,
        tenant_id,
        workspace_id,
        &["programs"],
        None,
    )
    .await?;
    let after_ready = after.map(|cursor| cursor.ready);
    let after_id = after.map(|cursor| cursor.id);
    let rows: Vec<(Uuid, String, i64, Option<String>, String)> = sqlx::query_as(
        "SELECT id, status, revision, name, current_step FROM programs \
         WHERE tenant_id=$1 AND workspace_id=$2 \
           AND ($3::boolean IS NULL OR (current_step='ready')>$3 \
                OR ((current_step='ready')=$3 AND id>$4)) \
         ORDER BY (current_step='ready'), id LIMIT $5",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(after_ready)
    .bind(after_id)
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    rows.into_iter()
        .map(|(id, status, revision, name, current_step)| {
            Ok(ProgramSummary {
                id,
                status: parse_status(&status)?,
                revision,
                name,
                current_step: parse_step(&current_step)?,
            })
        })
        .collect()
}

async fn insert_program(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    program: &Program,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO programs \
             (id, tenant_id, workspace_id, status, revision, name, intent, basis, \
              boundaries, constraints, success, working_notes, pending_question, \
              current_step, input_cursor, latest_input, max_input_bytes) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
    )
    .bind(program.id)
    .bind(tenant_id)
    .bind(program.workspace_id)
    .bind(status_name(program.status))
    .bind(program.revision)
    .bind(&program.name)
    .bind(&program.intent)
    .bind(&program.basis)
    .bind(&program.boundaries)
    .bind(&program.constraints)
    .bind(&program.success)
    .bind(&program.working_notes)
    .bind(&program.pending_question)
    .bind(step_name(program.current_step))
    .bind(program.input_cursor)
    .bind(program.latest_input)
    .bind(program.max_input_bytes)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(())
}

impl ProgramRow {
    fn into_domain(self) -> Result<Program> {
        if self.payload_erased {
            return Err(Error::KnowledgePayloadErased);
        }
        Ok(Program {
            id: self.id,
            workspace_id: self.workspace_id,
            status: parse_status(&self.status)?,
            revision: self.revision,
            name: self.name,
            intent: self.intent,
            basis: self.basis,
            boundaries: self.boundaries,
            constraints: self.constraints,
            success: self.success,
            working_notes: self.working_notes,
            pending_question: self.pending_question,
            current_step: parse_step(&self.current_step)?,
            input_cursor: self.input_cursor,
            latest_input: self.latest_input,
            planning_knowledge: None,
            max_input_bytes: self.max_input_bytes,
        })
    }
}

fn validate_input(input: &NewProgramInput) -> Result<()> {
    validate_program_input(input.request_id, &input.input)?;
    if input.encoded_bytes < 0 {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn input_from_row(
    (id, sequence, request_id, session_id, input): (Uuid, i64, Uuid, Uuid, String),
) -> ProgramInput {
    ProgramInput {
        id,
        sequence,
        request_id,
        session_id,
        input,
    }
}

fn status_name(status: ProgramStatus) -> &'static str {
    match status {
        ProgramStatus::Draft => "draft",
        ProgramStatus::Open => "open",
    }
}

fn step_name(step: ProgramStep) -> &'static str {
    match step {
        ProgramStep::Compose => "compose",
        ProgramStep::WaitingInput => "waiting_input",
        ProgramStep::Ready => "ready",
    }
}

fn parse_status(value: &str) -> Result<ProgramStatus> {
    match value {
        "draft" => Ok(ProgramStatus::Draft),
        "open" => Ok(ProgramStatus::Open),
        _ => Err(Error::StorageUnavailable),
    }
}

fn parse_step(value: &str) -> Result<ProgramStep> {
    match value {
        "compose" => Ok(ProgramStep::Compose),
        "waiting_input" => Ok(ProgramStep::WaitingInput),
        "ready" => Ok(ProgramStep::Ready),
        _ => Err(Error::StorageUnavailable),
    }
}
