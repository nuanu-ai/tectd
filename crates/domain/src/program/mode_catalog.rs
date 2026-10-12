//! Wording of the four product levels, and nothing else: tune the texts here, the
//! logic in `mode.rs` does not change. The Program method states the same four texts.

pub(super) const DEMO: &str = "The simplest way to actually show the requested behaviour. Internals may be rough and temporary. The real scenario is not replaced by a fake. Real money, secrets and data keep their guarantees even in a demo.";
pub(super) const MVP: &str =
    "A real beta for first users. The product scope is cut down, not the quality of what is in it.";
pub(super) const PROD: &str =
    "A product in operation that fits on one machine or on one machine per service.";
pub(super) const SERIOUS_PROD: &str = "A distributed system: queues, asynchronous processing, many services, Kubernetes, heavy load and many users. Best practices throughout.";
