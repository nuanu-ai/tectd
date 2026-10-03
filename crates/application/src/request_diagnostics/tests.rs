use super::*;

#[tokio::test]
async fn nested_scopes_restore_and_disabled_scope_collects_nothing() {
    assert!(!enabled());
    let outer = RequestTrace::new("test");
    let inner = RequestTrace::new("test");
    scope(Some(outer.clone()), async {
        assert!(enabled());
        count("before", 1);
        scope(Some(inner.clone()), async {
            assert!(enabled());
            count("inner", 2);
        })
        .await;
        assert!(enabled());
        let value = scope(None, async {
            assert!(!enabled());
            count("disabled", 3);
            42
        })
        .await;
        assert_eq!(value, 42);
        assert!(enabled());
        count("after", 4);
    })
    .await;
    assert_eq!(outer.snapshot("ok")["events"].as_array().unwrap().len(), 2);
    assert_eq!(inner.snapshot("ok")["events"].as_array().unwrap().len(), 1);
    assert!(CURRENT.with(|current| current.borrow().is_none()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_yielding_tasks_keep_their_trace() {
    let mut tasks = Vec::new();
    for number in 0..16 {
        let trace = RequestTrace::new("test");
        tasks.push(tokio::spawn(async move {
            scope(Some(trace.clone()), async {
                for _ in 0..32 {
                    count("task", number);
                    tokio::task::yield_now().await;
                }
            })
            .await;
            let record = trace.snapshot("ok");
            assert!(
                record["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|event| event["value"] == number)
            );
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_outside_poll_records_stage_and_restores_tls() {
    let trace = RequestTrace::new("test");
    let future = scope(
        Some(trace.clone()),
        measure("blocked", std::future::pending::<()>()),
    );
    let result = tokio::time::timeout(std::time::Duration::from_millis(1), future).await;
    assert!(result.is_err());
    assert!(CURRENT.with(|current| current.borrow().is_none()));
    assert_eq!(
        trace.snapshot("operation_timeout")["events"][1]["status"],
        "cancelled"
    );
}

#[tokio::test]
async fn event_limit_and_safe_record_schema_are_explicit() {
    let trace = RequestTrace::new("test");
    scope(Some(trace.clone()), async {
        for _ in 0..200 {
            count("fixed", 1);
        }
        let _cancelled = stage("overflow_cancelled");
    })
    .await;
    let record = trace.snapshot("ok");
    assert_eq!(record["events"].as_array().unwrap().len(), EVENT_CAP);
    assert_eq!(record["dropped_events"], 74);
    assert_eq!(record["counters"]["fixed"], 200);
    assert_eq!(record["cancelled_stages"][0]["stage"], "overflow_cancelled");
    let text = record.to_string();
    assert!(text.len() < 16384);
    for forbidden in [
        "credential",
        "arguments",
        "context",
        "body",
        "event_payload",
    ] {
        assert!(!text.contains(forbidden));
    }
}

#[test]
fn poll_panic_restores_outer_and_thread_context() {
    let outer = RequestTrace::new("test");
    let inner = RequestTrace::new("test");
    let mut future = scope(Some(outer.clone()), async {
        count("before", 1);
        let mut failing = scope(Some(inner), async { panic!("test poll panic") });
        let mut cx = Context::from_waker(std::task::Waker::noop());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Pin::new(&mut failing).poll(&mut cx)
        }));
        assert!(result.is_err());
        count("after", 1);
    });
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(Pin::new(&mut future).poll(&mut cx).is_ready());
    assert!(CURRENT.with(|current| current.borrow().is_none()));
    assert_eq!(outer.snapshot("ok")["counters"]["before"], 1);
    assert_eq!(outer.snapshot("ok")["counters"]["after"], 1);
}

#[tokio::test]
async fn boundary_completion_survives_event_and_stage_overflow() {
    let trace = RequestTrace::new("test");
    scope(Some(trace.clone()), async {
        let outer = stage("service_and_projection");
        for _ in 0..200 {
            count("fixed", 1);
        }
        for _ in 0..48 {
            stage("nested").complete();
        }
        outer.complete();
        stage("response_finalization").complete();
    })
    .await;
    let record = trace.snapshot("ok");
    assert_eq!(record["completed_stages"].as_array().unwrap().len(), 32);
    assert_eq!(record["dropped_completed_stages"], 18);
    assert_eq!(
        record["boundary_stages"]["service_and_projection"]["status"],
        "completed"
    );
    assert_eq!(
        record["boundary_stages"]["response_finalization"]["status"],
        "completed"
    );
}

#[tokio::test]
async fn outer_timeout_survives_more_than_sixteen_nested_cancellations() {
    let trace = RequestTrace::new("test");
    let future = scope(
        Some(trace.clone()),
        measure("service_and_projection", async {
            let _nested = (0..48).map(|_| stage("nested")).collect::<Vec<_>>();
            std::future::pending::<()>().await;
        }),
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1), future)
            .await
            .is_err()
    );
    let record = trace.snapshot("operation_timeout");
    assert_eq!(record["cancelled_stages"].as_array().unwrap().len(), 16);
    assert_eq!(record["dropped_cancellations"], 33);
    assert_eq!(
        record["boundary_stages"]["service_and_projection"]["status"],
        "cancelled"
    );
}

#[test]
fn scope_restores_after_moving_between_threads() {
    let trace = RequestTrace::new("test");
    let mut future = scope(Some(trace.clone()), async {
        count("first", 1);
        std::future::poll_fn({
            let mut first = true;
            move |_| {
                if first {
                    first = false;
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            }
        })
        .await;
        count("second", 2);
    });
    for expected in [false, true] {
        future = std::thread::spawn(move || {
            let mut cx = Context::from_waker(std::task::Waker::noop());
            assert_eq!(Pin::new(&mut future).poll(&mut cx).is_ready(), expected);
            assert!(CURRENT.with(|current| current.borrow().is_none()));
            future
        })
        .join()
        .unwrap();
    }
    assert_eq!(trace.snapshot("ok")["events"].as_array().unwrap().len(), 2);
}

async fn large_pending() {
    let payload = [0u8; 262144];
    std::hint::black_box(&payload);
    std::future::pending::<()>().await;
    std::hint::black_box(&payload);
}
fn future_size<F: Future>(_: impl FnOnce() -> F) -> usize {
    std::mem::size_of::<F>()
}
fn five_layers() -> impl Future<Output = ()> {
    measure(
        "layer",
        measure(
            "layer",
            measure("layer", measure("layer", measure("layer", large_pending()))),
        ),
    )
}

#[tokio::test]
async fn large_nested_measurements_have_bounded_frames_and_cancel_cleanly() {
    assert!(future_size(large_pending) >= 262144);
    assert!(future_size(five_layers) <= 256);
    for enabled in [false, true] {
        let trace = RequestTrace::new("test");
        let scoped = scope(enabled.then(|| trace.clone()), five_layers());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), scoped)
                .await
                .is_err()
        );
        assert!(!super::enabled());
        let record = trace.snapshot("operation_timeout");
        assert_eq!(
            record["cancelled_stages"].as_array().unwrap().len(),
            if enabled { 5 } else { 0 }
        );
    }
}

#[tokio::test]
async fn boxed_measurement_preserves_output_and_drops_pending_input_once() {
    let value = measure("result", async { Err::<usize, _>("expected") }).await;
    assert_eq!(value, Err("expected"));
    struct DropFlag(Arc<std::sync::atomic::AtomicUsize>);
    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let flag = DropFlag(count.clone());
    let future = measure("drop", async move {
        let _flag = flag;
        std::future::pending::<()>().await;
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(1), future)
            .await
            .is_err()
    );
    assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 1);
}
