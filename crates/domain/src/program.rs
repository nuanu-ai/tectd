use crate::{Error, ProgramSummary, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramStatus {
    Draft,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramStep {
    Compose,
    WaitingInput,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Program {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub status: ProgramStatus,
    pub revision: i64,
    pub name: Option<String>,
    pub intent: Option<String>,
    pub basis: Option<String>,
    pub boundaries: Option<String>,
    pub constraints: Option<String>,
    pub success: Option<String>,
    pub working_notes: Option<String>,
    pub pending_question: Option<String>,
    pub current_step: ProgramStep,
    pub input_cursor: i64,
    pub latest_input: i64,
    /// Adapter-measured maximum original-input encoding cost, never a business field.
    #[serde(skip)]
    pub max_input_bytes: i64,
}

impl Program {
    pub fn draft(id: Uuid, workspace_id: Uuid, max_input_bytes: i64) -> Self {
        Self {
            id,
            workspace_id,
            status: ProgramStatus::Draft,
            revision: 1,
            name: None,
            intent: None,
            basis: None,
            boundaries: None,
            constraints: None,
            success: None,
            working_notes: None,
            pending_question: None,
            current_step: ProgramStep::Compose,
            input_cursor: 0,
            latest_input: 1,
            max_input_bytes,
        }
    }

    pub fn saved(&self, change: &SaveProgram) -> Result<Self> {
        change.validate()?;
        if change.program_id != self.id {
            return Err(Error::NotFound);
        }
        if change.revision != self.revision {
            return Err(Error::StaleRevision);
        }
        if change.input_cursor < self.input_cursor
            || change.input_cursor > self.latest_input
            || change.complete && change.input_cursor != self.latest_input
        {
            return Err(Error::InputPending);
        }
        let mut next = self.clone();
        change.name.apply(&mut next.name);
        change.intent.apply(&mut next.intent);
        change.basis.apply(&mut next.basis);
        change.boundaries.apply(&mut next.boundaries);
        change.constraints.apply(&mut next.constraints);
        change.success.apply(&mut next.success);
        change.working_notes.apply(&mut next.working_notes);
        change.pending_question.apply(&mut next.pending_question);
        if next
            .pending_question
            .as_ref()
            .is_some_and(|text| text.trim().is_empty())
        {
            next.pending_question = None;
        }
        let fields = [
            &next.name,
            &next.intent,
            &next.basis,
            &next.boundaries,
            &next.constraints,
            &next.success,
        ];
        if fields
            .iter()
            .chain([&next.working_notes, &next.pending_question].iter())
            .any(|field| field.as_ref().is_some_and(|text| text.contains('\0')))
        {
            return Err(Error::InvalidArguments);
        }
        if (change.complete || self.status == ProgramStatus::Open)
            && fields
                .iter()
                .any(|field| field.as_ref().is_none_or(|text| text.trim().is_empty()))
            || change.complete && next.pending_question.is_some()
        {
            return Err(Error::ProgramIncomplete);
        }
        next.input_cursor = change.input_cursor;
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::StorageUnavailable)?;
        next.current_step = if change.complete {
            next.status = ProgramStatus::Open;
            ProgramStep::Ready
        } else if next.pending_question.is_some() {
            ProgramStep::WaitingInput
        } else {
            ProgramStep::Compose
        };
        Ok(next)
    }

    pub fn with_new_input(&self, encoded_bytes: i64) -> Result<Self> {
        if encoded_bytes < 0 {
            return Err(Error::InvalidArguments);
        }
        let mut next = self.clone();
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::StorageUnavailable)?;
        next.latest_input = self
            .latest_input
            .checked_add(1)
            .ok_or(Error::StorageUnavailable)?;
        next.max_input_bytes = self.max_input_bytes.max(encoded_bytes);
        next.current_step = ProgramStep::Compose;
        Ok(next)
    }

    pub fn summary(&self) -> ProgramSummary {
        ProgramSummary {
            id: self.id,
            status: self.status,
            revision: self.revision,
            name: self.name.clone(),
            current_step: self.current_step,
        }
    }
}

/// Omission preserves; explicit JSON null clears. This distinction is part of PATCH.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TextPatch {
    #[default]
    Unchanged,
    Set(Option<String>),
}

impl TextPatch {
    pub fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged)
    }

    fn apply(&self, field: &mut Option<String>) {
        if let Self::Set(value) = self {
            *field = value.clone();
        }
    }
}

impl<'de> Deserialize<'de> for TextPatch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Option::<String>::deserialize(deserializer).map(Self::Set)
    }
}

impl Serialize for TextPatch {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Unchanged => serializer.serialize_none(),
            Self::Set(value) => value.serialize(serializer),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveProgram {
    pub program_id: Uuid,
    pub revision: i64,
    pub input_cursor: i64,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub name: TextPatch,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub intent: TextPatch,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub basis: TextPatch,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub boundaries: TextPatch,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub constraints: TextPatch,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub success: TextPatch,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub working_notes: TextPatch,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub pending_question: TextPatch,
    #[serde(default)]
    pub complete: bool,
}

impl SaveProgram {
    pub fn validate(&self) -> Result<()> {
        if self.program_id.is_nil() || self.revision < 1 || self.input_cursor < 0 {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct NewProgramInput {
    pub request_id: Uuid,
    pub input: String,
    pub encoded_bytes: i64,
}

pub fn validate_program_input(request_id: Uuid, input: &str) -> Result<()> {
    if request_id.is_nil() || input.trim().is_empty() || input.contains('\0') {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
