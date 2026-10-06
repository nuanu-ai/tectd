mod socket_path;
use crate::Result;
use crate::frame::{Frame, FrameReader, MAX_FRAME_BYTES};
use crate::knowledge_lifecycle_tools::KnowledgeLifecycleInvocation;
use crate::knowledge_tools::KnowledgeInvocation;
use crate::program_output::ProgramEncoding;
use crate::program_tools::ProgramInvocation;
use crate::tools::{Invocation, parse_invocation};
use crate::{program_output, responses};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use socket_path::validate_socket;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tect_application::WorkspaceService;
use tect_domain::{
    Error, KnowledgeChangePhaseId, KnowledgeSearchMode, RequestContext, WorkspaceState,
};
use tokio::io::{AsyncWriteExt, WriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;

mod diagnostics;
mod program;
use program::execute_program;

const MAX_CONNECTIONS: usize = 32;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(45);
const SUPER_WIDE_TIMEOUT: Duration = Duration::from_secs(60);
const KNOWLEDGE_WRITE_TIMEOUT: Duration = Duration::from_secs(120);
// The bridge allows the daemon's response write cap plus additional scheduling margin.
const RESPONSE_MARGIN: Duration = Duration::from_secs(5);

fn operation_timeout_for(invocation: &Invocation) -> Duration {
    match invocation {
        Invocation::KnowledgeSearch(query) if query.mode == KnowledgeSearchMode::SuperWide => {
            SUPER_WIDE_TIMEOUT
        }
        Invocation::KnowledgeLifecycle(KnowledgeLifecycleInvocation::PhaseComplete(request))
            if request.phase_id == KnowledgeChangePhaseId::KcImpactPlan =>
        {
            KNOWLEDGE_WRITE_TIMEOUT
        }
        Invocation::KnowledgeLifecycle(KnowledgeLifecycleInvocation::Commit(_))
        | Invocation::Knowledge(KnowledgeInvocation::Publish(_)) => KNOWLEDGE_WRITE_TIMEOUT,
        _ => OPERATION_TIMEOUT,
    }
}

fn response_read_timeout_for(invocation: Option<&Invocation>) -> Duration {
    invocation.map_or(OPERATION_TIMEOUT, operation_timeout_for) + IO_TIMEOUT + RESPONSE_MARGIN
}

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
    diagnostics::initialize();
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
    // Parse the same validated invocation shape used by the daemon. Invalid calls
    // still get the bounded default while the daemon authenticates their error.
    let invocation = parse_invocation(tool_name, arguments.clone()).ok();
    let response_read_timeout = response_read_timeout_for(invocation.as_ref());

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
    let frame = timeout(response_read_timeout, reader.next())
        .await
        .map_err(|_| Error::TransportUnavailable)?
        .map_err(|_| Error::TransportUnavailable)?;
    let bytes = match frame {
        Some(Frame::Data(bytes)) => bytes,
        Some(Frame::TooLarge) => return Err(Error::RequestTooLarge),
        None => return Err(Error::TransportUnavailable),
    };
    decode_wire_response(&bytes)
}

fn decode_wire_response(bytes: &[u8]) -> Result<Value> {
    let response: WireResponse =
        serde_json::from_slice(bytes).map_err(|_| Error::TransportUnavailable)?;
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

include!("transport/execute.rs");

fn operation_response(
    result: std::result::Result<Result<Value>, tokio::time::error::Elapsed>,
    output_capacity: usize,
) -> WireResponse {
    match result {
        Ok(Ok(result)) => match responses::encoded_len(&result) {
            Ok(bytes) if bytes <= output_capacity => WireResponse::Ok { result },
            Ok(_) => WireResponse::Error {
                error: Error::RequestTooLarge,
            },
            Err(error) => WireResponse::Error { error },
        },
        Ok(Err(error)) => WireResponse::Error { error },
        Err(_) => WireResponse::Error {
            error: Error::OperationTimeout,
        },
    }
}

fn validate_wire_version(request: &WireRequest) -> Result<()> {
    if request.api_version == Some(crate::api::WIRE_API_VERSION) {
        Ok(())
    } else {
        Err(Error::InvalidConfiguration)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum InvalidRequestAuthority {
    Verifier,
    MatrixVerifier,
    CandidateRead,
    WorkspaceState,
}

fn invalid_request_authority(tool_name: &str) -> InvalidRequestAuthority {
    match tool_name {
        "candidate_advisory_verify" => InvalidRequestAuthority::Verifier,
        "verify_matrix_task"
        | "get_pipeline_open_effect"
        | "verify_pipeline_open_effect"
        | "get_pipeline_phase_effect"
        | "verify_pipeline_phase_effect" => InvalidRequestAuthority::MatrixVerifier,
        "candidate_advisory_get" | "candidate_advisory_audit" | "get_matrix_disposition" => {
            InvalidRequestAuthority::CandidateRead
        }
        _ => InvalidRequestAuthority::WorkspaceState,
    }
}

async fn authenticate_invalid_request(
    service: &WorkspaceService,
    context: &RequestContext,
    tool_name: &str,
) -> WireResponse {
    let authorization = timeout(OPERATION_TIMEOUT, async {
        match invalid_request_authority(tool_name) {
            InvalidRequestAuthority::Verifier => {
                service
                    .authenticate_candidate_advisory_verifier_session(context)
                    .await
            }
            InvalidRequestAuthority::MatrixVerifier => {
                service.authenticate_matrix_verifier_session(context).await
            }
            InvalidRequestAuthority::CandidateRead => {
                service
                    .authenticate_candidate_advisory_session(context)
                    .await
            }
            InvalidRequestAuthority::WorkspaceState => service.get_state(context).await.map(|_| ()),
        }
    })
    .await;
    match authorization {
        Ok(Ok(_)) => WireResponse::Error {
            error: Error::InvalidArguments,
        },
        Ok(Err(error)) => WireResponse::Error { error },
        Err(_) => WireResponse::Error {
            error: Error::OperationTimeout,
        },
    }
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

#[cfg(test)]
mod tests;
