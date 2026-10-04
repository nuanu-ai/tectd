//! Optional request diagnostics. Durations are inclusive and may overlap nested stages.
//! Poll-local installation keeps adapter observations attached to their request without I/O.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;
use uuid::Uuid;

mod aggregates;
mod labels;

const EVENT_CAP: usize = 128;
thread_local! {
    static CURRENT: RefCell<Option<Arc<RequestTrace>>> = const { RefCell::new(None) };
}

/// Whether the currently polled request has opted into diagnostics.
pub fn enabled() -> bool {
    CURRENT.with(|current| current.borrow().is_some())
}

pub struct RequestTrace {
    started: Instant,
    trace_id: Uuid,
    tool: &'static str,
    invalid_tool_label: bool,
    state: Mutex<TraceState>,
}

#[derive(Default)]
struct TraceState {
    events: Vec<serde_json::Value>,
    dropped_events: usize,
    next_span: usize,
    cancelled_stages: Vec<serde_json::Value>,
    dropped_cancellations: usize,
    counters: BTreeMap<&'static str, usize>,
    dropped_counter_labels: usize,
    completed_stages: Vec<serde_json::Value>,
    dropped_completed_stages: usize,
    boundary_stages: BTreeMap<&'static str, serde_json::Value>,
    finished_elapsed_us: Option<u64>,
    aggregates: aggregates::StageAggregates,
    invalid_stage_labels: usize,
    invalid_counter_labels: usize,
}

impl RequestTrace {
    pub fn new(tool: &'static str) -> Arc<Self> {
        let (tool, invalid_tool_label) = labels::normalize(tool, "invalid_tool");
        Arc::new(Self {
            started: Instant::now(),
            trace_id: Uuid::new_v4(),
            tool,
            invalid_tool_label,
            state: Mutex::new(TraceState::default()),
        })
    }

    pub fn trace_id(&self) -> Uuid {
        self.trace_id
    }

    fn record(&self, event: serde_json::Value) {
        if let Ok(mut state) = self.state.lock() {
            if state.events.len() < EVENT_CAP {
                state.events.push(event);
            } else {
                state.dropped_events = state.dropped_events.saturating_add(1);
            }
        }
    }

    /// Freeze request elapsed time before handing the trace to a persistence queue.
    pub fn finish(&self) {
        if let Ok(mut state) = self.state.lock() {
            state
                .finished_elapsed_us
                .get_or_insert_with(|| micros(self.started));
        }
    }

    pub fn snapshot(&self, outcome: &'static str) -> serde_json::Value {
        let (outcome, invalid_outcome_label) = labels::normalize(outcome, "invalid_outcome");
        let Ok(state) = self.state.lock() else {
            return serde_json::json!({"version":1,"trace_id":self.trace_id,"diagnostic_unavailable":true});
        };
        serde_json::json!({
            "version":1, "trace_id":self.trace_id, "tool":self.tool,
            "outcome":outcome, "elapsed_us":state.finished_elapsed_us.unwrap_or_else(|| micros(self.started)),
            "duration_semantics":"inclusive_nested_stages_overlap",
            "event_cap":EVENT_CAP, "dropped_events":state.dropped_events,
            "events":state.events, "cancelled_stages":state.cancelled_stages,
            "cancelled_stage_cap":16, "dropped_cancellations":state.dropped_cancellations,
            "counters":state.counters, "counter_label_cap":32,
            "dropped_counter_labels":state.dropped_counter_labels,
            "completed_stages":state.completed_stages, "completed_stage_cap":32,
            "dropped_completed_stages":state.dropped_completed_stages,
            "boundary_stages":state.boundary_stages,
            "stage_aggregates":state.aggregates.snapshot(),
            "stage_aggregate_label_cap":aggregates::LABEL_CAP,
            "dropped_stage_aggregate_labels":state.aggregates.dropped_labels,
            "stage_aggregate_labels_complete":state.aggregates.dropped_labels == 0 && state.invalid_stage_labels == 0,
            "stage_aggregate_numeric_saturated":state.aggregates.saturated,
            "diagnostic_label_byte_cap":labels::BYTE_CAP,
            "diagnostic_counter_label_byte_cap":labels::COUNTER_BYTE_CAP,
            "invalid_stage_labels":state.invalid_stage_labels,
            "invalid_counter_labels":state.invalid_counter_labels,
            "counter_labels_complete":state.dropped_counter_labels == 0 && state.invalid_counter_labels == 0,
            "invalid_tool_label":u8::from(self.invalid_tool_label),
            "invalid_outcome_label":u8::from(invalid_outcome_label),
        })
    }
}

fn micros(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

pub struct TraceStage {
    trace: Option<Arc<RequestTrace>>,
    label: &'static str,
    id: usize,
    started: Instant,
    completed: bool,
}

/// Capture only a constant stage label. Drop without completion marks cancellation.
pub fn stage(label: &'static str) -> TraceStage {
    let trace = CURRENT
        .with(|current| current.borrow().clone())
        .filter(|trace| {
            // Validate only after the enabled Option check, and never retain rejected text.
            if labels::valid(label, labels::BYTE_CAP) {
                true
            } else {
                if let Ok(mut state) = trace.state.lock() {
                    state.invalid_stage_labels = state.invalid_stage_labels.saturating_add(1);
                }
                false
            }
        });
    let mut id = 0;
    if let Some(trace) = &trace {
        if let Ok(mut state) = trace.state.lock() {
            state.aggregates.start(label);
            id = state.next_span;
            state.next_span = state.next_span.saturating_add(1);
        }
        trace.record(serde_json::json!({"kind":"stage_start","stage":label,"span":id,"at_us":micros(trace.started)}));
    }
    TraceStage {
        trace,
        label,
        id,
        started: Instant::now(),
        completed: false,
    }
}

impl TraceStage {
    pub fn complete(mut self) {
        self.completed = true;
    }
}

impl Drop for TraceStage {
    fn drop(&mut self) {
        if let Some(trace) = &self.trace {
            let duration_us = micros(self.started);
            let event = serde_json::json!({
                "kind":"stage_end", "stage":self.label, "span":self.id,
                "duration_us":duration_us, "at_us":micros(trace.started),
                "status":if self.completed {"completed"} else {"cancelled"},
            });
            if let Ok(mut state) = trace.state.lock() {
                state
                    .aggregates
                    .end(self.label, self.completed, duration_us);
                if matches!(
                    self.label,
                    "service_and_projection" | "response_finalization"
                ) {
                    state.boundary_stages.insert(self.label, event.clone());
                }
                if self.completed {
                    if state.completed_stages.len() == 32 {
                        state.completed_stages.remove(0);
                        state.dropped_completed_stages =
                            state.dropped_completed_stages.saturating_add(1);
                    }
                    state.completed_stages.push(event.clone());
                } else {
                    if state.cancelled_stages.len() == 16 {
                        state.cancelled_stages.remove(0);
                        state.dropped_cancellations = state.dropped_cancellations.saturating_add(1);
                    }
                    state.cancelled_stages.push(event.clone());
                }
            }
            trace.record(event);
        }
    }
}

pub fn measure<T>(label: &'static str, future: impl Future<Output = T>) -> impl Future<Output = T> {
    // Box before constructing the async state: an async-fn argument remains inline
    // in its unpolled state and compounds through nested measurement wrappers.
    let future = Box::pin(future);
    async move {
        let guard = stage(label);
        let result = future.await;
        guard.complete();
        result
    }
}

/// Add a nonnegative count; aggregate totals survive event truncation.
pub fn count(label: &'static str, value: usize) {
    CURRENT.with(|current| {
        if let Some(trace) = current.borrow().as_ref() {
            if !labels::valid(label, labels::COUNTER_BYTE_CAP) {
                if let Ok(mut state) = trace.state.lock() {
                    state.invalid_counter_labels = state.invalid_counter_labels.saturating_add(1);
                }
                return;
            }
            if let Ok(mut state) = trace.state.lock() {
                if state.counters.contains_key(label) || state.counters.len() < 32 {
                    let total = state.counters.entry(label).or_default();
                    *total = total.saturating_add(value);
                } else {
                    state.dropped_counter_labels = state.dropped_counter_labels.saturating_add(1);
                }
            }
            trace.record(serde_json::json!({"kind":"count","counter":label,"value":value}));
        }
    });
}

pub struct ScopedFuture<F> {
    future: Pin<Box<F>>,
    trace: Option<Arc<RequestTrace>>,
}

pub fn scope<F: Future>(trace: Option<Arc<RequestTrace>>, future: F) -> ScopedFuture<F> {
    ScopedFuture {
        future: Box::pin(future),
        trace,
    }
}

struct Restore(Option<Arc<RequestTrace>>);
impl Drop for Restore {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = self.0.take());
    }
}

impl<F: Future> Future for ScopedFuture<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let previous = CURRENT.with(|current| current.replace(this.trace.clone()));
        let _restore = Restore(previous);
        this.future.as_mut().poll(cx)
    }
}

#[cfg(test)]
mod tests;
