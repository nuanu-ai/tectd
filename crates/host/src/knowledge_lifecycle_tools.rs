use serde_json::Value;
use tect_domain::*;

pub(crate) enum KnowledgeLifecycleInvocation {
    Lifecycle(KnowledgeLifecycleQuery),
    Unit(KnowledgeUnitQuery),
    Begin(Box<BeginKnowledgeChange>),
    PhaseComplete(Box<CompleteKnowledgeChangePhase>),
    RecordInput(RecordKnowledgeChangeInput),
    Commit(CommitKnowledgeChange),
    Settle(SettleKnowledgeChangeEffects),
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<KnowledgeLifecycleInvocation> {
    reject_nulls(&arguments)?;
    match name {
        "knowledge_lifecycle" => checked(arguments).map(KnowledgeLifecycleInvocation::Lifecycle),
        "knowledge_unit" => checked(arguments).map(KnowledgeLifecycleInvocation::Unit),
        "knowledge_change_begin" => {
            checked(arguments).map(|v| KnowledgeLifecycleInvocation::Begin(Box::new(v)))
        }
        "knowledge_change_phase_complete" => {
            let value: CompleteKnowledgeChangePhase = decode(arguments)?;
            if value.phase_id == KnowledgeChangePhaseId::KcPrepareChange {
                value.validate_before_binding_resolution()?;
            } else {
                value.validate()?;
            }
            Ok(KnowledgeLifecycleInvocation::PhaseComplete(Box::new(value)))
        }
        "knowledge_change_record_input" => {
            checked(arguments).map(KnowledgeLifecycleInvocation::RecordInput)
        }
        "knowledge_change_commit" => checked(arguments).map(KnowledgeLifecycleInvocation::Commit),
        "knowledge_change_settle_effects" => {
            checked(arguments).map(KnowledgeLifecycleInvocation::Settle)
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn decode<T: for<'de> serde::Deserialize<'de>>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(Error::invalid_arguments_from)
}

trait Validated {
    fn check(&self) -> Result<()>;
}
macro_rules! validated {
    ($($kind:ty),+) => {$(
        impl Validated for $kind { fn check(&self) -> Result<()> { self.validate() } }
    )+};
}
validated!(
    KnowledgeLifecycleQuery,
    KnowledgeUnitQuery,
    BeginKnowledgeChange,
    CompleteKnowledgeChangePhase,
    RecordKnowledgeChangeInput,
    CommitKnowledgeChange,
    SettleKnowledgeChangeEffects
);

fn checked<T: for<'de> serde::Deserialize<'de> + Validated>(value: Value) -> Result<T> {
    let value: T = decode(value)?;
    value.check()?;
    Ok(value)
}

fn reject_nulls(value: &Value) -> Result<()> {
    match value {
        Value::Object(values) => {
            if values.values().any(Value::is_null) {
                return Err(Error::InvalidArguments);
            }
            for value in values.values() {
                reject_nulls(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_nulls(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
