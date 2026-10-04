//! Operator opt-in diagnostics; never part of the wire response or authorization.
use super::*;
use crate::pipeline_tools::PipelineInvocation;
use std::future::Future;
use std::sync::OnceLock;
use tect_application::request_diagnostics::{self as trace, RequestTrace};
mod sink;
static WORKER: OnceLock<Option<sink::Worker>> = OnceLock::new();

pub(super) struct Capture {
    trace: Arc<RequestTrace>,
    worker: &'static sink::Worker,
}

fn allowed_method(value: &str) -> Option<&'static str> {
    match value {
        "slice_pipeline_phase_complete" => Some("slice_pipeline_phase_complete"),
        "slice_pipeline_context" => Some("slice_pipeline_context"),
        _ => None,
    }
}

/// Called once by serve before accepting requests; filesystem work stays in the worker.
pub(super) fn initialize() {
    WORKER.get_or_init(|| {
        let method = std::env::var("TECT_REQUEST_DIAGNOSTICS_METHOD").ok();
        let directory = std::env::var("TECT_REQUEST_DIAGNOSTICS_DIR").ok();
        if method.is_none() && directory.is_none() {
            return None;
        }
        let (Some(method), Some(directory)) = (method, directory) else {
            eprintln!("request_diagnostics_configuration_disabled");
            return None;
        };
        let Some(method) = allowed_method(&method) else {
            eprintln!("request_diagnostics_configuration_disabled");
            return None;
        };
        let directory = PathBuf::from(directory);
        match sink::start(method, directory) {
            Some(worker) => Some(worker),
            None => {
                eprintln!("request_diagnostics_worker_unavailable");
                None
            }
        }
    });
}

fn method(invocation: &Invocation) -> Option<&'static str> {
    match invocation {
        Invocation::Pipeline(PipelineInvocation::Complete(_)) => {
            Some("slice_pipeline_phase_complete")
        }
        Invocation::Pipeline(PipelineInvocation::Context(_)) => Some("slice_pipeline_context"),
        _ => None,
    }
}

/// Request selection performs no filesystem access, environment access or worker creation.
pub(super) fn capture(invocation: &Invocation) -> Option<Capture> {
    let method = method(invocation)?;
    let worker = WORKER.get()?.as_ref()?;
    if worker.method != method {
        return None;
    }
    Some(Capture {
        trace: RequestTrace::new(method),
        worker,
    })
}

fn outcome(response: &WireResponse) -> &'static str {
    match response {
        WireResponse::Ok { .. } => "ok",
        WireResponse::Error { error } => error.code(),
    }
}

/// Preserve the existing timeout future and the existing external finalization boundary.
pub(super) fn timed(
    capture: Option<Capture>,
    deadline: Duration,
    capacity: usize,
    future: impl Future<Output = Result<Value>>,
) -> impl Future<Output = WireResponse> {
    let future = Box::pin(future);
    async move {
        if capture.is_none() {
            return operation_response(timeout(deadline, future).await, capacity);
        }
        let trace = capture.as_ref().map(|capture| capture.trace.clone());
        let result = trace::scope(
            trace.clone(),
            timeout(deadline, trace::measure("service_and_projection", future)),
        )
        .await;
        let response = trace::scope(trace, async {
            let guard = trace::stage("response_finalization");
            let response = operation_response(result, capacity);
            guard.complete();
            response
        })
        .await;
        if let Some(capture) = capture {
            capture.trace.finish();
            sink::submit(
                capture.worker,
                sink::Job {
                    trace: capture.trace,
                    outcome: outcome(&response),
                },
            );
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::Ordering, mpsc};

    fn capture_for_test() -> (Capture, mpsc::Receiver<sink::Job>) {
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = Box::leak(Box::new(sink::Worker {
            method: "slice_pipeline_context",
            sender,
            losses: Arc::new(sink::Losses::default()),
        }));
        (
            Capture {
                trace: RequestTrace::new("slice_pipeline_context"),
                worker,
            },
            receiver,
        )
    }

    #[test]
    fn selection_is_exact_allowlist() {
        assert!(allowed_method("slice_pipeline_context").is_some());
        assert!(allowed_method("slice_pipeline_phase_complete").is_some());
        assert!(allowed_method("get_state").is_none());
    }

    #[tokio::test]
    async fn enabled_and_disabled_preserve_success_error_and_wire_capacity() {
        for capacity in [MAX_FRAME_BYTES, 1] {
            for result in [
                Ok(serde_json::json!({"data":"unchanged"})),
                Err(Error::Forbidden),
            ] {
                let expected =
                    encode_line(&operation_response(Ok(result.clone()), capacity)).unwrap();
                let disabled =
                    timed(None, OPERATION_TIMEOUT, capacity, async { result.clone() }).await;
                assert_eq!(encode_line(&disabled).unwrap(), expected);
                let (capture, receiver) = capture_for_test();
                let enabled = timed(Some(capture), OPERATION_TIMEOUT, capacity, async {
                    result.clone()
                })
                .await;
                assert_eq!(encode_line(&enabled).unwrap(), expected);
                assert!(receiver.try_recv().is_ok());
            }
        }
        assert_eq!(OPERATION_TIMEOUT, Duration::from_secs(45));
    }

    #[tokio::test]
    async fn sink_disconnection_and_full_queue_preserve_success() {
        let (capture, receiver) = capture_for_test();
        let worker = capture.worker;
        drop(receiver);
        let response = timed(Some(capture), OPERATION_TIMEOUT, MAX_FRAME_BYTES, async {
            Ok(serde_json::json!({}))
        })
        .await;
        assert_eq!(outcome(&response), "ok");
        assert_eq!(worker.losses.queue.load(Ordering::Relaxed), 1);
        let (capture, _receiver) = capture_for_test();
        let worker = capture.worker;
        sink::submit(
            worker,
            sink::Job {
                trace: RequestTrace::new("slice_pipeline_context"),
                outcome: "ok",
            },
        );
        let response = timed(Some(capture), OPERATION_TIMEOUT, MAX_FRAME_BYTES, async {
            Ok(serde_json::json!({}))
        })
        .await;
        assert_eq!(outcome(&response), "ok");
        assert_eq!(worker.losses.queue.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn queued_timeout_is_safe_and_elapsed_is_frozen() {
        let (capture, receiver) = capture_for_test();
        let response = timed(
            Some(capture),
            Duration::from_millis(1),
            MAX_FRAME_BYTES,
            std::future::pending(),
        )
        .await;
        assert_eq!(outcome(&response), "operation_timeout");
        let job = receiver.try_recv().unwrap();
        let first = job.trace.snapshot(job.outcome);
        std::thread::sleep(Duration::from_millis(2));
        let second = job.trace.snapshot(job.outcome);
        assert_eq!(first["elapsed_us"], second["elapsed_us"]);
        assert_eq!(
            first["boundary_stages"]["service_and_projection"]["status"],
            "cancelled"
        );
        let text = first.to_string();
        for forbidden in [
            "request_id",
            "run_id",
            "credential",
            "arguments",
            "body",
            "sql",
            "params",
            "directory",
        ] {
            assert!(!text.contains(forbidden));
        }
    }
    async fn large_pending_input() -> Result<Value> {
        let payload = [0u8; 262144];
        std::hint::black_box(&payload);
        std::future::pending::<()>().await;
        std::hint::black_box(&payload);
        Ok(Value::Null)
    }

    #[tokio::test]
    async fn timed_large_input_has_small_frame_and_preserves_cancellation() {
        fn size<F: Future>(_: impl FnOnce() -> F) -> usize {
            std::mem::size_of::<F>()
        }
        assert!(size(large_pending_input) >= 262144);
        assert!(
            size(|| timed(
                None,
                OPERATION_TIMEOUT,
                MAX_FRAME_BYTES,
                large_pending_input()
            )) <= 1024
        );
        for enabled in [false, true] {
            let (capture, receiver) = capture_for_test();
            let response = timed(
                enabled.then_some(capture),
                Duration::from_millis(1),
                MAX_FRAME_BYTES,
                large_pending_input(),
            )
            .await;
            assert_eq!(outcome(&response), "operation_timeout");
            if enabled {
                let job = receiver.try_recv().unwrap();
                assert_eq!(
                    job.trace.snapshot(job.outcome)["boundary_stages"]["service_and_projection"]["status"],
                    "cancelled"
                );
            } else {
                assert!(receiver.try_recv().is_err());
            }
        }
    }
}
