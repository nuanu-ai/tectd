use crate::{Error, Result, SetupDirectory, SetupSummary, TextPatch};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStatus {
    Draft,
    Applied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStep {
    Compose,
    WaitingInput,
    ReadyToApply,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setup {
    pub id: Uuid,
    pub workspace_id: Uuid,
    #[serde(skip)]
    pub host_id: Uuid,
    #[serde(skip)]
    pub directory: SetupDirectory,
    pub status: SetupStatus,
    pub revision: i64,
    pub content: Option<String>,
    pub working_notes: Option<String>,
    pub pending_question: Option<String>,
    pub current_step: SetupStep,
    pub input_cursor: i64,
    pub latest_input: i64,
    #[serde(skip)]
    pub max_input_bytes: i64,
    pub applied_from_revision: Option<i64>,
    pub applied_sha256: Option<String>,
}

impl Setup {
    pub fn draft(
        id: Uuid,
        workspace_id: Uuid,
        host_id: Uuid,
        directory: SetupDirectory,
        max_input_bytes: i64,
    ) -> Self {
        Self {
            id,
            workspace_id,
            host_id,
            directory,
            status: SetupStatus::Draft,
            revision: 1,
            content: None,
            working_notes: None,
            pending_question: None,
            current_step: SetupStep::Compose,
            input_cursor: 0,
            latest_input: 1,
            max_input_bytes,
            applied_from_revision: None,
            applied_sha256: None,
        }
    }

    pub fn saved(&self, change: &SaveSetup) -> Result<Self> {
        change.validate()?;
        if change.setup_id != self.id {
            return Err(Error::NotFound);
        }
        self.editable_revision(change.revision)?;
        if change.input_cursor < self.input_cursor
            || change.input_cursor > self.latest_input
            || change.ready && change.input_cursor != self.latest_input
        {
            return Err(Error::InputPending);
        }
        let mut next = self.clone();
        change.content.apply(&mut next.content);
        change.working_notes.apply(&mut next.working_notes);
        change.pending_question.apply(&mut next.pending_question);
        if next
            .pending_question
            .as_ref()
            .is_some_and(|text| text.trim().is_empty())
        {
            next.pending_question = None;
        }
        if [&next.content, &next.working_notes, &next.pending_question]
            .iter()
            .any(|field| field.as_ref().is_some_and(|text| text.contains('\0')))
        {
            return Err(Error::InvalidArguments);
        }
        if change.ready
            && (next
                .content
                .as_ref()
                .is_none_or(|text| text.trim().is_empty())
                || next.pending_question.is_some())
        {
            return Err(Error::SetupIncomplete);
        }
        next.input_cursor = change.input_cursor;
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::StorageUnavailable)?;
        next.current_step = if change.ready {
            SetupStep::ReadyToApply
        } else if next.pending_question.is_some() {
            SetupStep::WaitingInput
        } else {
            SetupStep::Compose
        };
        Ok(next)
    }

    pub fn with_new_input(&self, revision: i64, encoded_bytes: i64) -> Result<Self> {
        if revision < 1 || encoded_bytes < 0 {
            return Err(Error::InvalidArguments);
        }
        self.editable_revision(revision)?;
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
        next.current_step = SetupStep::Compose;
        Ok(next)
    }

    fn editable_revision(&self, revision: i64) -> Result<()> {
        if self.status == SetupStatus::Applied {
            return Err(Error::SetupAlreadyApplied);
        }
        if revision != self.revision {
            return Err(Error::StaleRevision);
        }
        Ok(())
    }

    /// Applied replay accepts only the durable pre-publication revision.
    pub fn validate_apply(&self, revision: i64) -> Result<()> {
        if revision < 1 {
            return Err(Error::InvalidArguments);
        }
        if self.status == SetupStatus::Applied {
            return if self.applied_from_revision == Some(revision) {
                Ok(())
            } else {
                Err(Error::StaleRevision)
            };
        }
        self.editable_revision(revision)?;
        if self.input_cursor != self.latest_input {
            return Err(Error::InputPending);
        }
        if self.current_step != SetupStep::ReadyToApply
            || self.pending_question.is_some()
            || self
                .content
                .as_ref()
                .is_none_or(|text| text.trim().is_empty())
        {
            return Err(Error::SetupIncomplete);
        }
        Ok(())
    }

    pub fn applied(&self, revision: i64, sha256: &str) -> Result<Self> {
        self.validate_apply(revision)?;
        if self.status == SetupStatus::Applied {
            return Err(Error::SetupAlreadyApplied);
        }
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::InvalidArguments);
        }
        let mut next = self.clone();
        next.status = SetupStatus::Applied;
        next.current_step = SetupStep::Complete;
        next.applied_from_revision = Some(revision);
        next.applied_sha256 = Some(sha256.to_owned());
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::StorageUnavailable)?;
        Ok(next)
    }

    pub fn summary(&self) -> SetupSummary {
        SetupSummary {
            id: self.id,
            status: self.status,
            revision: self.revision,
            current_step: self.current_step,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveSetup {
    pub setup_id: Uuid,
    pub revision: i64,
    pub input_cursor: i64,
    pub ready: bool,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub content: TextPatch,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub working_notes: TextPatch,
    #[serde(default, skip_serializing_if = "TextPatch::is_unchanged")]
    pub pending_question: TextPatch,
}

impl SaveSetup {
    pub fn validate(&self) -> Result<()> {
        if self.setup_id.is_nil() || self.revision < 1 || self.input_cursor < 0 {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct NewSetupInput {
    pub request_id: Uuid,
    pub input: String,
    pub encoded_bytes: i64,
}

pub fn validate_setup_input(request_id: Uuid, input: &str) -> Result<()> {
    crate::validate_program_input(request_id, input)
}

#[cfg(test)]
mod tests;
