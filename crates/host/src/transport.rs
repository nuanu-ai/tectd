use crate::Result;
use crate::frame::{Frame, FrameReader, MAX_FRAME_BYTES};
use crate::program_output::ProgramEncoding;
use crate::program_tools::ProgramInvocation;
use crate::tools::{Invocation, parse_invocation};
use crate::{program_output, responses};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::future::Future;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tect_application::{AntiBloatVerificationEvidence, WorkspaceService};
use tect_domain::{Error, RequestContext, WorkspaceState};
use tokio::io::{AsyncWriteExt, WriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;

const MAX_CONNECTIONS: usize = 32;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    #[serde(default)]
    api_version: Option<u32>,
    context: RequestContext,
    tool_name: String,
    arguments: Value,
    #[serde(default = "default_capacity")]
    output_capacity: usize,
}

fn default_capacity() -> usize {
    MAX_FRAME_BYTES
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum WireResponse {
    Ok { result: Value },
    Error { error: Error },
}

pub async fn serve(listener: UnixListener, service: Arc<WorkspaceService>) -> Result<()> {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let permit = permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::TransportUnavailable)?;
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|_| Error::TransportUnavailable)?;
        let service = service.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = handle_connection(stream, service).await;
        });
    }
}

pub async fn call(
    socket: &Path,
    context: &RequestContext,
    tool_name: &str,
    arguments: Value,
) -> Result<WorkspaceState> {
    let result = call_tool(socket, context, tool_name, arguments).await?;
    serde_json::from_value(result).map_err(|_| Error::TransportUnavailable)
}

pub async fn call_tool(
    socket: &Path,
    context: &RequestContext,
    tool_name: &str,
    arguments: Value,
) -> Result<Value> {
    call_tool_bounded(socket, context, tool_name, arguments, MAX_FRAME_BYTES).await
}

pub(crate) async fn call_tool_bounded(
    socket: &Path,
    context: &RequestContext,
    tool_name: &str,
    arguments: Value,
    output_capacity: usize,
) -> Result<Value> {
    let expected_socket = validate_socket(socket)?;

    let request = WireRequest {
        api_version: Some(crate::api::WIRE_API_VERSION),
        context: context.clone(),
        tool_name: tool_name.to_owned(),
        arguments,
        output_capacity,
    };
    let bytes = encode_line(&request)?;
    let stream = timeout(IO_TIMEOUT, UnixStream::connect(socket))
        .await
        .map_err(|_| Error::TransportUnavailable)?
        .map_err(|_| Error::TransportUnavailable)?;
    let connected_socket = validate_socket(socket)?;
    if connected_socket != expected_socket {
        return Err(Error::InvalidConfiguration);
    }
    let (read, mut write) = tokio::io::split(stream);
    timeout(IO_TIMEOUT, write.write_all(&bytes))
        .await
        .map_err(|_| Error::TransportUnavailable)?
        .map_err(|_| Error::TransportUnavailable)?;
    timeout(IO_TIMEOUT, write.shutdown())
        .await
        .map_err(|_| Error::TransportUnavailable)?
        .map_err(|_| Error::TransportUnavailable)?;

    let mut reader = FrameReader::new(read);
    let frame = timeout(OPERATION_TIMEOUT, reader.next())
        .await
        .map_err(|_| Error::TransportUnavailable)?
        .map_err(|_| Error::TransportUnavailable)?;
    let bytes = match frame {
        Some(Frame::Data(bytes)) => bytes,
        Some(Frame::TooLarge) => return Err(Error::RequestTooLarge),
        None => return Err(Error::TransportUnavailable),
    };
    let response: WireResponse =
        serde_json::from_slice(&bytes).map_err(|_| Error::TransportUnavailable)?;
    match response {
        WireResponse::Ok { result } => Ok(result),
        WireResponse::Error { error } => Err(error),
    }
}

async fn handle_connection(stream: UnixStream, service: Arc<WorkspaceService>) -> Result<()> {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = FrameReader::new(read);
    let frame = timeout(IO_TIMEOUT, reader.next())
        .await
        .map_err(|_| Error::TransportUnavailable)?
        .map_err(|_| Error::TransportUnavailable)?;
    let response = match frame {
        Some(Frame::Data(bytes)) => match serde_json::from_slice::<WireRequest>(&bytes) {
            Ok(request) => execute(request, &service).await,
            Err(_) => WireResponse::Error {
                error: Error::InvalidArguments,
            },
        },
        Some(Frame::TooLarge) => WireResponse::Error {
            error: Error::RequestTooLarge,
        },
        None => return Ok(()),
    };
    write_response(&mut write, response).await
}

include!("transport/dispatch.rs");

fn validate_wire_version(request: &WireRequest) -> Result<()> {
    if request.api_version == Some(crate::api::WIRE_API_VERSION) {
        Ok(())
    } else {
        Err(Error::InvalidConfiguration)
    }
}

async fn authenticate_invalid_request(
    service: &WorkspaceService,
    context: &RequestContext,
    tool_name: &str,
) -> WireResponse {
    authenticate_invalid_request_with(tool_name, |kind| async move {
        match kind {
            InvalidRequestAuth::MatrixVerifier => {
                service.authenticate_matrix_verifier_session(context).await
            }
            InvalidRequestAuth::CandidateAdvisory => {
                service
                    .authenticate_candidate_advisory_session(context)
                    .await
            }
            InvalidRequestAuth::State => service.get_state(context).await.map(|_| ()),
        }
    })
    .await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvalidRequestAuth {
    MatrixVerifier,
    CandidateAdvisory,
    State,
}

fn invalid_request_auth(tool_name: &str) -> InvalidRequestAuth {
    if matches!(
        tool_name,
        "verify_matrix_task"
            | "get_matrix_planning_effect"
            | "verify_matrix_planning_effect"
            | "get_pipeline_open_effect"
            | "verify_pipeline_open_effect"
            | "get_pipeline_phase_effect"
            | "verify_pipeline_phase_effect"
    ) {
        InvalidRequestAuth::MatrixVerifier
    } else if allows_verifier_invalid_request(tool_name) {
        InvalidRequestAuth::CandidateAdvisory
    } else {
        InvalidRequestAuth::State
    }
}

async fn authenticate_invalid_request_with<F, Fut>(tool_name: &str, authenticate: F) -> WireResponse
where
    F: FnOnce(InvalidRequestAuth) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let authorization = timeout(
        OPERATION_TIMEOUT,
        authenticate(invalid_request_auth(tool_name)),
    )
    .await;
    match authorization {
        Ok(Ok(_)) => WireResponse::Error {
            error: Error::InvalidArguments,
        },
        Ok(Err(error)) => WireResponse::Error { error },
        Err(_) => WireResponse::Error {
            error: Error::TransportUnavailable,
        },
    }
}

fn allows_verifier_invalid_request(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "candidate_advisory_verify" | "candidate_advisory_get" | "candidate_advisory_audit"
    )
}

fn serialize<T: Serialize>(result: Result<T>) -> Result<Value> {
    result
        .and_then(|value| serde_json::to_value(value).map_err(|_| Error::TransportUnavailable))
        .and_then(|data| {
            Ok(responses::with_actions(
                data,
                vec![responses::action("get_state", serde_json::json!({}))?],
                Some(0),
            ))
        })
}

async fn execute_program(
    context: &RequestContext,
    invocation: ProgramInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    let guard = ProgramEncoding { capacity };
    let guidance = program_output::StaticProgramGuidance;
    match invocation {
        ProgramInvocation::Begin {
            request_id,
            input,
            task_context,
        } => service
            .begin_program(
                context,
                request_id,
                &input,
                &task_context,
                &guidance,
                &guard,
            )
            .await
            .and_then(program_output::program),
        ProgramInvocation::Get {
            program_id,
            after_input,
            limit,
        } => service
            .get_program(context, program_id, after_input, limit, &guidance)
            .await
            .and_then(|page| program_output::page(page, capacity)),
        ProgramInvocation::Save(changes) => service
            .save_program(context, &changes, &guidance, &guard)
            .await
            .and_then(program_output::saved),
        ProgramInvocation::Record {
            program_id,
            request_id,
            input,
            task_context,
        } => service
            .record_program_input(
                context,
                program_id,
                request_id,
                &input,
                task_context.as_ref(),
                &guidance,
                &guard,
            )
            .await
            .and_then(program_output::program),
        ProgramInvocation::Refresh(request) => service
            .refresh_program_knowledge(context, &request, &guidance, &guard)
            .await
            .and_then(program_output::program),
        ProgramInvocation::List { after, limit } => service
            .list_programs(context, after, limit)
            .await
            .and_then(|list| program_output::list(list, capacity)),
        ProgramInvocation::ReadSkill => {
            service.read_program_skill(context).await?;
            Ok(program_output::skill())
        }
    }
}

async fn write_response(writer: &mut WriteHalf<UnixStream>, response: WireResponse) -> Result<()> {
    let bytes = match encode_line(&response) {
        Ok(bytes) => bytes,
        Err(Error::RequestTooLarge) => encode_line(&WireResponse::Error {
            error: Error::RequestTooLarge,
        })?,
        Err(error) => return Err(error),
    };
    timeout(IO_TIMEOUT, writer.write_all(&bytes))
        .await
        .map_err(|_| Error::TransportUnavailable)?
        .map_err(|_| Error::TransportUnavailable)
}

fn encode_line<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| Error::TransportUnavailable)?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(Error::RequestTooLarge);
    }
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(PartialEq, Eq)]
struct SocketIdentity {
    device: u64,
    inode: u64,
}

fn validate_socket(path: &Path) -> Result<SocketIdentity> {
    if !path.is_absolute() {
        return Err(Error::InvalidConfiguration);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            _ => return Err(Error::InvalidConfiguration),
        }
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::TransportUnavailable
            } else {
                Error::InvalidConfiguration
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(Error::InvalidConfiguration);
        }
    }

    let parent = path.parent().ok_or(Error::InvalidConfiguration)?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|_| Error::InvalidConfiguration)?;
    if !parent_metadata.is_dir() || parent_metadata.mode() & 0o7777 != 0o700 {
        return Err(Error::InvalidConfiguration);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| Error::TransportUnavailable)?;
    if !metadata.file_type().is_socket() || metadata.mode() & 0o7777 != 0o600 {
        return Err(Error::InvalidConfiguration);
    }
    Ok(SocketIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(test)]
mod tests;
