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
        trace.snapshot("operation_timeout")["stage_aggregates"]["blocked"]["cancelled_count"],
        1
    );
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
        let mut failing = scope(Some(inner.clone()), async {
            let _guard = stage("panic_stage");
            panic!("test poll panic")
        });
        let mut cx = Context::from_waker(std::task::Waker::noop());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Pin::new(&mut failing).poll(&mut cx)
        }));
        assert!(result.is_err());
        assert_eq!(
            inner.snapshot("panic")["stage_aggregates"]["panic_stage"]["cancelled_count"],
            1
        );
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
            record["stage_aggregates"]["layer"]["cancelled_count"]
                .as_u64()
                .unwrap_or(0),
            if enabled { 5 } else { 0 }
        );
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

#[tokio::test]
async fn stage_aggregates_survive_timeline_overflow_with_exact_terminal_coverage() {
    let trace = RequestTrace::new("test");
    scope(Some(trace.clone()), async {
        for index in 0..1500 {
            let guard = stage(if index % 2 == 0 { "one" } else { "two" });
            if index % 3 != 0 {
                guard.complete();
            }
        }
    })
    .await;
    let record = trace.snapshot("ok");
    assert!(record["dropped_events"].as_u64().unwrap() > 0);
    assert!(record["dropped_completed_stages"].as_u64().unwrap() > 0);
    assert!(record["dropped_cancellations"].as_u64().unwrap() > 0);
    assert_eq!(record["stage_aggregate_labels_complete"], true);
    assert_eq!(record["dropped_stage_aggregate_labels"], 0);
    for label in ["one", "two"] {
        let aggregate = &record["stage_aggregates"][label];
        assert_eq!(aggregate["started_count"], 750);
        assert_eq!(aggregate["completed_count"], 500);
        assert_eq!(aggregate["cancelled_count"], 250);
        assert_eq!(
            aggregate["inclusive_total_us"].as_u64().unwrap(),
            aggregate["completed_total_us"].as_u64().unwrap()
                + aggregate["cancelled_total_us"].as_u64().unwrap()
        );
        assert!(
            aggregate["inclusive_min_us"].as_u64().unwrap()
                <= aggregate["inclusive_max_us"].as_u64().unwrap()
        );
    }
}

#[tokio::test]
async fn aggregate_label_overflow_is_explicit_and_snapshot_fits_host_budget() {
    const LABELS: [&str; 65] = [
        "s00", "s01", "s02", "s03", "s04", "s05", "s06", "s07", "s08", "s09", "s10", "s11", "s12",
        "s13", "s14", "s15", "s16", "s17", "s18", "s19", "s20", "s21", "s22", "s23", "s24", "s25",
        "s26", "s27", "s28", "s29", "s30", "s31", "s32", "s33", "s34", "s35", "s36", "s37", "s38",
        "s39", "s40", "s41", "s42", "s43", "s44", "s45", "s46", "s47", "s48", "s49", "s50", "s51",
        "s52", "s53", "s54", "s55", "s56", "s57", "s58", "s59", "s60", "s61", "s62", "s63", "s64",
    ];
    let trace = RequestTrace::new("test");
    scope(Some(trace.clone()), async {
        for label in LABELS {
            stage(label).complete();
        }
        drop(stage("s64"));
    })
    .await;
    let record = trace.snapshot("ok");
    assert_eq!(record["stage_aggregates"].as_object().unwrap().len(), 64);
    assert_eq!(record["dropped_stage_aggregate_labels"], 2);
    assert_eq!(record["stage_aggregate_labels_complete"], false);
    assert!(record["stage_aggregates"]["s64"].is_null());
    // Maximize numeric width in all retained arrays/maps, including aggregate fields.
    fn widen(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Number(_) => *value = u64::MAX.into(),
            serde_json::Value::Array(items) => items.iter_mut().for_each(widen),
            serde_json::Value::Object(items) => items.values_mut().for_each(widen),
            _ => {}
        }
    }
    let mut worst = record;
    // Populate every independent retention channel, using the full48-byte admission bound,
    // beyond the current production inventory (maximum41bytes).
    let cancelled = worst["cancelled_stages"][0].clone();
    worst["cancelled_stages"] = serde_json::json!(vec![cancelled; 16]);
    let boundary = worst["completed_stages"][0].clone();
    worst["boundary_stages"] = serde_json::json!({
        "service_and_projection": boundary, "response_finalization": boundary,
    });
    worst["counters"] = serde_json::json!(
        (0..32)
            .map(|index| { (format!("c{index:02}_{}", "x".repeat(60)), u64::MAX) })
            .collect::<BTreeMap<_, _>>()
    );
    let entries = worst["stage_aggregates"].as_object().unwrap().clone();
    worst["stage_aggregates"] = serde_json::json!(
        entries
            .into_iter()
            .map(|(label, value)| { (format!("{label}_{}", "x".repeat(44)), value) })
            .collect::<BTreeMap<_, _>>()
    );
    for channel in ["events", "completed_stages", "cancelled_stages"] {
        for event in worst[channel].as_array_mut().unwrap() {
            event["stage"] = "x".repeat(48).into();
        }
    }
    worst["tool"] = "x".repeat(48).into();
    worst["outcome"] = "x".repeat(48).into();
    // Use terminal records for the entire raw channel for a conservative bound.
    let terminal = worst["cancelled_stages"][0].clone();
    worst["events"] = serde_json::json!(vec![terminal; EVENT_CAP]);
    widen(&mut worst);
    let bytes = serde_json::to_vec(&worst).unwrap().len();
    eprintln!("maximum_width_snapshot_bytes={bytes} host_budget=65536");
    assert!(bytes < 65536);
}

#[tokio::test]
async fn invalid_labels_never_serialize_and_disabled_admission_is_inert() {
    const LONG: &str = concat!(
        "rejected_long_label_",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    );
    let huge: &'static str = Box::leak("OFFENSIVE".repeat(2048).into_boxed_str());
    let trace = RequestTrace::new(huge);
    scope(Some(trace.clone()), async {
        for label in [LONG, huge, "non_ascii_é", "control\nlabel", ""] {
            stage(label).complete();
            count(label, 1);
        }
        stage("accepted").complete();
        count("accepted", 1);
    })
    .await;
    let record = trace.snapshot("bad\toutcome");
    assert_eq!(record["invalid_tool_label"], 1);
    assert_eq!(record["invalid_outcome_label"], 1);
    assert_eq!(record["tool"], "invalid_tool");
    assert_eq!(record["outcome"], "invalid_outcome");
    assert_eq!(record["invalid_stage_labels"], 5);
    assert_eq!(record["invalid_counter_labels"], 5);
    assert_eq!(record["stage_aggregate_labels_complete"], false);
    assert_eq!(record["counter_labels_complete"], false);
    assert_eq!(record["stage_aggregates"]["accepted"]["completed_count"], 1);
    let text = record.to_string();
    for rejected in [
        "rejected_long_label",
        "OFFENSIVE",
        "non_ascii",
        "control",
        "bad",
    ] {
        assert!(!text.contains(rejected));
    }
    let disabled = RequestTrace::new("test");
    scope(
        Some(disabled.clone()),
        scope(None, async {
            stage(huge).complete();
            count(huge, 1);
        }),
    )
    .await;
    let disabled = disabled.snapshot("ok");
    assert_eq!(disabled["invalid_stage_labels"], 0);
    assert_eq!(disabled["invalid_counter_labels"], 0);
    assert!(disabled["events"].as_array().unwrap().is_empty());
    assert!(labels::valid("x".repeat(48).as_str(), labels::BYTE_CAP));
    assert!(!labels::valid("x".repeat(49).as_str(), labels::BYTE_CAP));
    assert!(labels::valid(
        "x".repeat(64).as_str(),
        labels::COUNTER_BYTE_CAP
    ));
    assert!(!labels::valid(
        "x".repeat(65).as_str(),
        labels::COUNTER_BYTE_CAP
    ));
}

#[tokio::test]
async fn public_count_admits_existing_fifty_two_byte_counter_and_rejects_oversize() {
    const SENTINELS: &str = "proof.native_batch_returned_rows_including_sentinels";
    const OVERSIZE: &str = concat!(
        "REJECTED_OVERSIZE_COUNTER_",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    );
    let trace = RequestTrace::new("test");
    scope(Some(trace.clone()), async {
        count(SENTINELS, 6135);
    })
    .await;
    let accepted = trace.snapshot("ok");
    assert_eq!(accepted["counters"][SENTINELS], 6135);
    assert_eq!(accepted["invalid_counter_labels"], 0);
    assert_eq!(accepted["counter_labels_complete"], true);
    scope(Some(trace.clone()), async {
        count(OVERSIZE, 1);
    })
    .await;
    let rejected = trace.snapshot("ok");
    assert_eq!(rejected["counters"][SENTINELS], 6135);
    assert_eq!(rejected["invalid_counter_labels"], 1);
    assert_eq!(rejected["counter_labels_complete"], false);
    assert!(!rejected.to_string().contains("REJECTED_OVERSIZE_COUNTER"));
}
