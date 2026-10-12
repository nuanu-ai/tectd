//! The product level of one Scope or one Slice where it differs from the Program's.
//! An agent cannot decide it: a draft states the level together with the id of an
//! input the user gave to that candidate set and a verbatim quote of the permission
//! from that input, and the save keeps the override only when the quote stands there.
use crate::{Error, ProgramMode, Refusal, RefusalCode, Result};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// The longest stated level a refusal repeats back.
const SHOWN_LEVEL_CHARS: usize = 64;

/// The level a piece of work is built to: its own override, else the override of the
/// Scope it lies in, else the Program's mode.
pub fn effective_mode(
    own: Option<ProgramMode>,
    scope: Option<ProgramMode>,
    program: ProgramMode,
) -> ProgramMode {
    own.or(scope).unwrap_or(program)
}

/// The override as a Scope candidate of a draft states it. The level is text until
/// the save has checked it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeModeOverrideDraft {
    pub mode: String,
    /// A `planning_input` source reference of the snapshot the draft is saved against.
    pub source_ref_id: Uuid,
    pub exact_quote: String,
}

/// The override as a work node of a Slice candidate draft states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceModeOverrideDraft {
    pub mode: String,
    /// An input the user gave to the Slice candidate set of the draft.
    pub input_id: Uuid,
    pub exact_quote: String,
}

/// A permitted override as a Scope candidate holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeCandidateModeOverride {
    pub mode: ProgramMode,
    pub source_ref_id: Uuid,
    pub exact_quote: String,
}

/// A permitted override as a Slice work candidate holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceCandidateModeOverride {
    pub mode: ProgramMode,
    pub input_id: Uuid,
    pub exact_quote: String,
}

/// What an opened Scope carries: the level and the reference that permitted it. The
/// user's words stay in the input and in the candidate the Scope was opened from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeModeOverride {
    pub mode: ProgramMode,
    pub source_ref_id: Uuid,
}

/// What an opened Slice carries: the level and the input that permitted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceModeOverride {
    pub mode: ProgramMode,
    pub input_id: Uuid,
}

impl ScopeCandidateModeOverride {
    pub const fn carried(&self) -> ScopeModeOverride {
        ScopeModeOverride {
            mode: self.mode,
            source_ref_id: self.source_ref_id,
        }
    }
}

impl SliceCandidateModeOverride {
    pub const fn carried(&self) -> SliceModeOverride {
        SliceModeOverride {
            mode: self.mode,
            input_id: self.input_id,
        }
    }
}

/// An override is stated whole or left out: an explicit null is not "no override".
pub(crate) fn stated<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// What the store found behind the id an override cites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRecord<'a> {
    /// The draft's candidate set has no record with that id.
    Unknown,
    /// The candidate set has it, but the snapshot the draft is saved against does not.
    OutsideSnapshot,
    /// A record the user did not give as input to this candidate set: a field of
    /// the Program, or an input that a Slice result produced.
    NotUserInput,
    /// An input the user gave to this candidate set, with its whole text.
    UserInput(&'a str),
}

/// The planning level a draft belongs to, and with it the names its refusal uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeOverrideLevel {
    Scope,
    Slice,
}

impl ModeOverrideLevel {
    /// The command that records the user's answer on this level.
    pub const fn record_route(self) -> &'static str {
        match self {
            Self::Scope => "scope.candidates.record_input",
            Self::Slice => "slice.candidates.input",
        }
    }

    /// The level whose save produced this refusal, when it refuses an override.
    pub fn of(refusal: &Refusal) -> Option<Self> {
        if refusal.code != RefusalCode::ModeOverrideNotPermitted {
            return None;
        }
        [Self::Scope, Self::Slice]
            .into_iter()
            .find(|level| refusal.next_action.as_deref() == Some(level.record_route()))
    }

    /// The part of the override that names the user's input.
    const fn input_part(self) -> &'static str {
        match self {
            Self::Scope => "source_ref_id",
            Self::Slice => "input_id",
        }
    }

    /// What that part must name.
    const fn input_expected(self) -> &'static str {
        match self {
            Self::Scope => {
                "a planning_input source reference of the snapshot this draft is saved against"
            }
            Self::Slice => {
                "the id of an input the user gave to this Slice candidate set, held by the snapshot this draft is saved against"
            }
        }
    }

    fn refused(self, index: usize, fault: Fault, expected: &str, actual: String) -> Error {
        let (list, prefix) = match self {
            Self::Scope => ("candidates", "SCOPE"),
            Self::Slice => ("nodes", "SLICE"),
        };
        let (part, rule) = match fault {
            Fault::Level => ("mode", "LEVEL"),
            Fault::Quote => ("exact_quote", "QUOTE"),
            Fault::Input => (self.input_part(), "INPUT"),
        };
        Error::Refused(Box::new(
            Refusal::new(RefusalCode::ModeOverrideNotPermitted)
                .with_message(RefusalCode::ModeOverrideNotPermitted.message())
                .with_rule(format!("{prefix}-DRAFT-MODE-OVERRIDE-{rule}"))
                .with_path(format!("/params/draft/{list}/{index}/mode_override/{part}"))
                .with_expected(expected)
                .with_actual(actual)
                .with_next_action(self.record_route())
                .with_required("user_permission_quote"),
        ))
    }

    /// The level an override may be kept with, or the refusal that sends the agent
    /// to the user. `index` is the position of the candidate or node in the draft.
    fn permitted(
        self,
        index: usize,
        mode: &str,
        exact_quote: &str,
        record: PermissionRecord<'_>,
    ) -> Result<ProgramMode> {
        let Some(level) = ProgramMode::named(mode) else {
            return Err(self.refused(
                index,
                Fault::Level,
                "demo, mvp, prod or serious_prod",
                mode.chars().take(SHOWN_LEVEL_CHARS).collect(),
            ));
        };
        if exact_quote.trim().is_empty() {
            return Err(self.refused(
                index,
                Fault::Quote,
                "the user's permission, quoted verbatim from their input",
                "blank".into(),
            ));
        }
        let unusable = |actual: &str| {
            Err(self.refused(index, Fault::Input, self.input_expected(), actual.into()))
        };
        let text = match record {
            PermissionRecord::UserInput(text) => text,
            PermissionRecord::Unknown => return unusable("an id this candidate set does not have"),
            PermissionRecord::OutsideSnapshot => {
                return unusable("an id that snapshot does not hold");
            }
            PermissionRecord::NotUserInput => {
                return unusable("a record that is not the user's input to this candidate set");
            }
        };
        if !text.contains(exact_quote) {
            return Err(self.refused(
                index,
                Fault::Quote,
                "text that stands verbatim in the cited input",
                "text the cited input does not contain".into(),
            ));
        }
        Ok(level)
    }
}

/// Which part of an override a refusal points at.
#[derive(Clone, Copy)]
enum Fault {
    Level,
    Quote,
    Input,
}

impl ScopeModeOverrideDraft {
    /// The override the candidate at `index` may hold, given what its
    /// `source_ref_id` names.
    pub fn permitted(
        &self,
        index: usize,
        record: PermissionRecord<'_>,
    ) -> Result<ScopeCandidateModeOverride> {
        let level = ModeOverrideLevel::Scope;
        Ok(ScopeCandidateModeOverride {
            mode: level.permitted(index, &self.mode, &self.exact_quote, record)?,
            source_ref_id: self.source_ref_id,
            exact_quote: self.exact_quote.clone(),
        })
    }
}

impl SliceModeOverrideDraft {
    /// The override the work node at `index` may hold, given what its `input_id` names.
    pub fn permitted(
        &self,
        index: usize,
        record: PermissionRecord<'_>,
    ) -> Result<SliceCandidateModeOverride> {
        let level = ModeOverrideLevel::Slice;
        Ok(SliceCandidateModeOverride {
            mode: level.permitted(index, &self.mode, &self.exact_quote, record)?,
            input_id: self.input_id,
            exact_quote: self.exact_quote.clone(),
        })
    }
}

#[cfg(test)]
mod tests;
