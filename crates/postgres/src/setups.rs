use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{
    Error, NewSetupInput, Result, Setup, SetupContext, SetupDirectory, SetupInput, SetupStatus,
    SetupStep, SetupSummary, validate_setup_input, validate_setup_path,
};
use uuid::Uuid;

const SETUP_COLUMNS: &str = "id, workspace_id, host_id, task_directory, device, inode, \
    status, revision, content, working_notes, pending_question, current_step, input_cursor, \
    latest_input, max_input_bytes, applied_from_revision, applied_sha256";

type SetupContextRow = (
    String,
    Option<Uuid>,
    Option<String>,
    Option<i64>,
    Option<String>,
);

#[derive(sqlx::FromRow)]
struct SetupRow {
    id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    task_directory: String,
    device: i64,
    inode: i64,
    status: String,
    revision: i64,
    content: Option<String>,
    working_notes: Option<String>,
    pending_question: Option<String>,
    current_step: String,
    input_cursor: i64,
    latest_input: i64,
    max_input_bytes: i64,
    applied_from_revision: Option<i64>,
    applied_sha256: Option<String>,
}

pub(crate) async fn setup_directory(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    session_id: Uuid,
) -> Result<Option<SetupDirectory>> {
    let row: Option<(String, i64, i64)> = sqlx::query_as(
        "SELECT task_directory, device, inode FROM setup_session_directories \
         WHERE tenant_id=$1 AND workspace_id=$2 AND host_id=$3 AND session_id=$4",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(row.map(|(path, device, inode)| SetupDirectory {
        path,
        device,
        inode,
    }))
}

pub(crate) async fn bind_setup_directory(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    session_id: Uuid,
    directory: &SetupDirectory,
) -> Result<()> {
    validate_directory(directory)?;
    sqlx::query(
        "INSERT INTO setup_session_directories \
             (tenant_id, workspace_id, host_id, session_id, task_directory, device, inode) \
         VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(session_id)
    .bind(&directory.path)
    .bind(directory.device)
    .bind(directory.inode)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let saved = setup_directory(transaction, tenant_id, workspace_id, host_id, session_id)
        .await?
        .ok_or(Error::StorageUnavailable)?;
    if saved != *directory {
        return Err(Error::TaskDirectoryMismatch);
    }
    Ok(())
}

pub(crate) async fn lock_setup_directory(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    path: &str,
) -> Result<()> {
    validate_setup_path(path)?;
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
             pg_catalog.hashtextextended(\
                 $1::text || ':' || $2::text || ':' || $3::text || ':' || $4, 0))",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(path)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(crate) async fn setup_for_directory(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    path: &str,
    for_update: bool,
) -> Result<Option<Setup>> {
    validate_setup_path(path)?;
    let query = format!(
        "SELECT {SETUP_COLUMNS} FROM workspace_setups \
         WHERE tenant_id=$1 AND workspace_id=$2 AND host_id=$3 AND task_directory=$4{}",
        if for_update { " FOR UPDATE" } else { "" }
    );
    let row = sqlx::query_as::<_, SetupRow>(&query)
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(host_id)
        .bind(path)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(storage_error)?;
    row.map(SetupRow::into_domain).transpose()
}

pub(crate) async fn setup(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    setup_id: Uuid,
    for_update: bool,
) -> Result<Option<Setup>> {
    let query = format!(
        "SELECT {SETUP_COLUMNS} FROM workspace_setups \
         WHERE tenant_id=$1 AND workspace_id=$2 AND host_id=$3 AND id=$4{}",
        if for_update { " FOR UPDATE" } else { "" }
    );
    let row = sqlx::query_as::<_, SetupRow>(&query)
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(host_id)
        .bind(setup_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(storage_error)?;
    row.map(SetupRow::into_domain).transpose()
}

pub(crate) async fn insert_setup(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    setup: &Setup,
    session_id: Uuid,
    input: &NewSetupInput,
) -> Result<()> {
    validate_directory(&setup.directory)?;
    validate_input(input)?;
    sqlx::query(
        "INSERT INTO workspace_setups \
             (id, tenant_id, workspace_id, host_id, task_directory, device, inode, status, \
              revision, content, working_notes, pending_question, current_step, input_cursor, \
              latest_input, max_input_bytes, applied_from_revision, applied_sha256) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
    )
    .bind(setup.id)
    .bind(tenant_id)
    .bind(setup.workspace_id)
    .bind(setup.host_id)
    .bind(&setup.directory.path)
    .bind(setup.directory.device)
    .bind(setup.directory.inode)
    .bind(status_name(setup.status))
    .bind(setup.revision)
    .bind(&setup.content)
    .bind(&setup.working_notes)
    .bind(&setup.pending_question)
    .bind(step_name(setup.current_step))
    .bind(setup.input_cursor)
    .bind(setup.latest_input)
    .bind(setup.max_input_bytes)
    .bind(setup.applied_from_revision)
    .bind(&setup.applied_sha256)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    insert_setup_input(
        transaction,
        tenant_id,
        setup.workspace_id,
        setup.id,
        session_id,
        1,
        input,
    )
    .await
}

pub(crate) async fn update_setup(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    setup: &Setup,
) -> Result<()> {
    validate_directory(&setup.directory)?;
    let result = sqlx::query(
        "UPDATE workspace_setups SET status=$7, revision=$8, content=$9, working_notes=$10, \
             pending_question=$11, current_step=$12, input_cursor=$13, latest_input=$14, \
             max_input_bytes=$15, applied_from_revision=$16, applied_sha256=$17, \
             updated_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND host_id=$3 AND id=$4 \
           AND task_directory=$5 AND device=$6 AND inode=$18",
    )
    .bind(tenant_id)
    .bind(setup.workspace_id)
    .bind(setup.host_id)
    .bind(setup.id)
    .bind(&setup.directory.path)
    .bind(setup.directory.device)
    .bind(status_name(setup.status))
    .bind(setup.revision)
    .bind(&setup.content)
    .bind(&setup.working_notes)
    .bind(&setup.pending_question)
    .bind(step_name(setup.current_step))
    .bind(setup.input_cursor)
    .bind(setup.latest_input)
    .bind(setup.max_input_bytes)
    .bind(setup.applied_from_revision)
    .bind(&setup.applied_sha256)
    .bind(setup.directory.inode)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if result.rows_affected() != 1 {
        return Err(Error::NotFound);
    }
    Ok(())
}

pub(crate) async fn setup_input(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    setup_id: Uuid,
    request_id: Uuid,
) -> Result<Option<SetupInput>> {
    let row: Option<(Uuid, i64, Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, sequence, request_id, session_id, input FROM workspace_setup_inputs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND setup_id=$3 AND request_id=$4",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(setup_id)
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(row.map(input_from_row))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_setup_input(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    setup_id: Uuid,
    session_id: Uuid,
    sequence: i64,
    input: &NewSetupInput,
) -> Result<()> {
    validate_input(input)?;
    if sequence < 1 {
        return Err(Error::InvalidArguments);
    }
    let result = sqlx::query(
        "INSERT INTO workspace_setup_inputs \
             (tenant_id, workspace_id, host_id, setup_id, sequence, request_id, session_id, input) \
         SELECT s.tenant_id, s.workspace_id, s.host_id, s.id, $4, $5, $6, $7 \
         FROM workspace_setups AS s \
         WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(setup_id)
    .bind(sequence)
    .bind(input.request_id)
    .bind(session_id)
    .bind(&input.input)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if result.rows_affected() != 1 {
        return Err(Error::NotFound);
    }
    Ok(())
}

pub(crate) async fn setup_inputs(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    setup_id: Uuid,
    after: i64,
    limit: u32,
) -> Result<Vec<SetupInput>> {
    let rows: Vec<(Uuid, i64, Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, sequence, request_id, session_id, input FROM workspace_setup_inputs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND setup_id=$3 AND sequence>$4 \
         ORDER BY sequence LIMIT $5",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(setup_id)
    .bind(after)
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(rows.into_iter().map(input_from_row).collect())
}

pub(crate) async fn setup_context(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    session_id: Uuid,
) -> Result<Option<SetupContext>> {
    let row: Option<SetupContextRow> = sqlx::query_as(
        "SELECT d.task_directory, s.id, s.status, s.revision, s.current_step \
             FROM setup_session_directories AS d \
             LEFT JOIN workspace_setups AS s \
               ON s.tenant_id=d.tenant_id AND s.workspace_id=d.workspace_id \
              AND s.host_id=d.host_id AND s.task_directory=d.task_directory \
              AND s.device=d.device AND s.inode=d.inode \
             WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.host_id=$3 AND d.session_id=$4",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    row.map(|(task_directory, id, status, revision, current_step)| {
        let setup = match (id, status, revision, current_step) {
            (Some(id), Some(status), Some(revision), Some(current_step)) => Some(SetupSummary {
                id,
                status: parse_status(&status)?,
                revision,
                current_step: parse_step(&current_step)?,
            }),
            (None, None, None, None) => None,
            _ => return Err(Error::StorageUnavailable),
        };
        Ok(SetupContext {
            task_directory,
            setup,
        })
    })
    .transpose()
}

impl SetupRow {
    fn into_domain(self) -> Result<Setup> {
        Ok(Setup {
            id: self.id,
            workspace_id: self.workspace_id,
            host_id: self.host_id,
            directory: SetupDirectory {
                path: self.task_directory,
                device: self.device,
                inode: self.inode,
            },
            status: parse_status(&self.status)?,
            revision: self.revision,
            content: self.content,
            working_notes: self.working_notes,
            pending_question: self.pending_question,
            current_step: parse_step(&self.current_step)?,
            input_cursor: self.input_cursor,
            latest_input: self.latest_input,
            max_input_bytes: self.max_input_bytes,
            applied_from_revision: self.applied_from_revision,
            applied_sha256: self.applied_sha256,
        })
    }
}

fn validate_directory(directory: &SetupDirectory) -> Result<()> {
    validate_setup_path(&directory.path)?;
    if directory.device < 0 || directory.inode < 0 {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn validate_input(input: &NewSetupInput) -> Result<()> {
    validate_setup_input(input.request_id, &input.input)?;
    if input.encoded_bytes < 0 {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn input_from_row(
    (id, sequence, request_id, session_id, input): (Uuid, i64, Uuid, Uuid, String),
) -> SetupInput {
    SetupInput {
        id,
        sequence,
        request_id,
        session_id,
        input,
    }
}

fn status_name(status: SetupStatus) -> &'static str {
    match status {
        SetupStatus::Draft => "draft",
        SetupStatus::Applied => "applied",
    }
}

fn step_name(step: SetupStep) -> &'static str {
    match step {
        SetupStep::Compose => "compose",
        SetupStep::WaitingInput => "waiting_input",
        SetupStep::ReadyToApply => "ready_to_apply",
        SetupStep::Complete => "complete",
    }
}

fn parse_status(value: &str) -> Result<SetupStatus> {
    match value {
        "draft" => Ok(SetupStatus::Draft),
        "applied" => Ok(SetupStatus::Applied),
        _ => Err(Error::StorageUnavailable),
    }
}

fn parse_step(value: &str) -> Result<SetupStep> {
    match value {
        "compose" => Ok(SetupStep::Compose),
        "waiting_input" => Ok(SetupStep::WaitingInput),
        "ready_to_apply" => Ok(SetupStep::ReadyToApply),
        "complete" => Ok(SetupStep::Complete),
        _ => Err(Error::StorageUnavailable),
    }
}
