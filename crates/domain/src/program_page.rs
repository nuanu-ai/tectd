use crate::{Error, Program, ProgramStatus, ProgramStep, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramInput {
    pub id: Uuid,
    pub sequence: i64,
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramSummary {
    pub id: Uuid,
    pub status: ProgramStatus,
    pub revision: i64,
    pub name: Option<String>,
    pub current_step: ProgramStep,
}

impl ProgramSummary {
    pub fn cursor(&self) -> ProgramCursor {
        ProgramCursor {
            ready: self.current_step == ProgramStep::Ready,
            id: self.id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramCursor {
    pub ready: bool,
    pub id: Uuid,
}

impl ProgramCursor {
    pub fn parse(value: &str) -> Result<Self> {
        let (tier, value) = value.split_once(':').ok_or(Error::InvalidArguments)?;
        let ready = match tier {
            "w" => false,
            "r" => true,
            _ => return Err(Error::InvalidArguments),
        };
        let id = Uuid::parse_str(value).map_err(|_| Error::InvalidArguments)?;
        if id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        Ok(Self { ready, id })
    }

    pub fn encode(self) -> String {
        format!("{}:{}", if self.ready { "r" } else { "w" }, self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramPage {
    pub program: Program,
    pub inputs: Vec<ProgramInput>,
    pub next_after_input: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramList {
    pub programs: Vec<ProgramSummary>,
    pub next_after: Option<String>,
}
