//! Jev's advice binds: the agent follows it and departs only where it holds the advice
//! plainly wrong, stating why. Here is what is kept of Jev's answer about one
//! candidate, how a review states a departure from it, and the refusal of a ready
//! review while advice that asks for a change is open.
use super::{PIPELINE_POINT, SIZING_POINT, SizingLevel, pipeline, sizing};
use crate::{Error, Refusal, RefusalCode, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

/// Where a review states its departures; the refusal points here.
const DEPARTURES_PATH: &str = "/params/review/advice_departures";
/// The longest list of open items a refusal carries, in bytes. It names as many
/// items as fit and counts the rest, so that the refusal keeps its usual size.
const NAMED_OPEN_BYTES: usize = 2048;

/// The advice points a ready review is held to. `intake` is not one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingPoint {
    Sizing,
    Pipeline,
}

impl BindingPoint {
    /// The code of the advice point, as the journal holds it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sizing => SIZING_POINT,
            Self::Pipeline => PIPELINE_POINT,
        }
    }

    /// What Jev chose in a kept answer of this point, as a refusal says it.
    fn chose(self, answer: &Value) -> Option<String> {
        match self {
            Self::Sizing => sizing::chose(answer),
            Self::Pipeline => pipeline::chose(answer),
        }
    }
}

/// Jev's answer about one candidate a draft save created or changed, as it is kept
/// for the review of that draft.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateAdvice {
    pub level: SizingLevel,
    pub candidate_id: Uuid,
    /// The revision the store holds the candidate under: the answer is about that one.
    pub candidate_revision: i64,
    /// Whether the answer asks for the candidate to be changed.
    pub asks_change: bool,
    /// The candidate's entry of the reply field: what Jev chose and its
    /// probabilities, and no text of the draft.
    pub answer: Value,
}

/// A kept answer that asks for a change and that no review has departed from.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenAdvice {
    /// The id of the kept answer itself.
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub candidate_revision: i64,
    pub point: BindingPoint,
    pub answer: Value,
}

/// One departure a review states: the advice of one point about one candidate is
/// plainly wrong, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdviceDeparture {
    pub candidate_id: Uuid,
    pub point: BindingPoint,
    pub reason: String,
}

/// The departures a review states are held to two rules: each gives a reason, and
/// one candidate and point is named once.
pub fn validate_advice_departures(departures: &[AdviceDeparture]) -> Result<()> {
    let mut named = BTreeSet::new();
    for (index, departure) in departures.iter().enumerate() {
        if departure.reason.trim().is_empty() {
            return Err(Error::invalid_arguments_at(
                format!("advice departure at index {index} requires a nonblank reason"),
                format!("{DEPARTURES_PATH}/{index}/reason"),
            ));
        }
        if !named.insert((departure.candidate_id, departure.point)) {
            return Err(Error::invalid_arguments_at(
                format!(
                    "advice departure at index {index} repeats candidate {} and point {}",
                    departure.candidate_id,
                    departure.point.as_str()
                ),
                format!("{DEPARTURES_PATH}/{index}"),
            ));
        }
    }
    Ok(())
}

impl SizingLevel {
    /// The level whose review was refused over open advice, when this is that refusal.
    pub fn of_open_advice(refusal: &Refusal) -> Option<Self> {
        if refusal.code != RefusalCode::AdviceNotFollowed {
            return None;
        }
        [Self::ProgramToScopes, Self::ScopeToSlices]
            .into_iter()
            .find(|level| refusal.rule.as_deref() == Some(level.open_advice_rule()))
    }

    const fn open_advice_rule(self) -> &'static str {
        match self {
            Self::ProgramToScopes => "SCOPE-REVIEW-ADVICE-OPEN",
            Self::ScopeToSlices => "SLICE-REVIEW-ADVICE-OPEN",
        }
    }

    /// The command that saves a draft and a review of this level.
    const fn save_route(self) -> &'static str {
        match self {
            Self::ProgramToScopes => "scope.candidates.save",
            Self::ScopeToSlices => "slice.candidates.save",
        }
    }

    /// What a review of this level does with the advice open on the draft it reviews:
    /// the kept answers it departs from, each with the reason it states.
    ///
    /// `open` holds the kept answers of the candidate set that ask for a change and
    /// have no departure. One counts while its candidate stands in `draft`, the
    /// candidates of the draft under review with their revisions, under the revision
    /// the answer is about. A review that says ready is refused while one of them has
    /// no departure among `stated`. A departure about a candidate without open advice
    /// records nothing and is no error.
    pub fn review_departures<'a>(
        self,
        ready: bool,
        draft: &[(Uuid, i64)],
        open: &[OpenAdvice],
        stated: &'a [AdviceDeparture],
    ) -> Result<Vec<(Uuid, &'a str)>> {
        let mut recorded = Vec::new();
        let mut standing = Vec::new();
        // In draft order, and for one candidate `sizing` before `pipeline`.
        for (id, revision) in draft {
            let mut about = open
                .iter()
                .filter(|advice| {
                    advice.candidate_id == *id && advice.candidate_revision == *revision
                })
                .collect::<Vec<_>>();
            about.sort_by_key(|advice| advice.point);
            for advice in about {
                let departure = stated
                    .iter()
                    .find(|stated| stated.candidate_id == *id && stated.point == advice.point);
                match departure {
                    Some(departure) => recorded.push((advice.id, departure.reason.as_str())),
                    None => standing.push(advice),
                }
            }
        }
        if ready && !standing.is_empty() {
            return Err(self.not_followed(&standing)?);
        }
        Ok(recorded)
    }

    /// The refusal of a ready review: it names the open items and says what closes them.
    fn not_followed(self, standing: &[&OpenAdvice]) -> Result<Error> {
        Ok(Error::Refused(Box::new(
            Refusal::new(RefusalCode::AdviceNotFollowed)
                .with_message(RefusalCode::AdviceNotFollowed.message())
                .with_rule(self.open_advice_rule())
                .with_path(DEPARTURES_PATH)
                .with_expected(
                    "each named candidate changed as advised and the draft saved again, or \
                     advice_departures with a reason for each named candidate and point",
                )
                .with_actual(named(standing)?)
                .with_next_action(self.save_route())
                .with_required("advice_followed_or_departure_reasons"),
        )))
    }
}

/// The open items as a refusal names them: candidate id, point and what Jev chose.
fn named(standing: &[&OpenAdvice]) -> Result<String> {
    let mut items = Vec::new();
    let mut bytes = 0;
    for (index, advice) in standing.iter().enumerate() {
        // A kept answer was written by the point it belongs to and reads back whole.
        let chose = advice
            .point
            .chose(&advice.answer)
            .ok_or(Error::InternalInvariant)?;
        let item = format!(
            "candidate {} {}: {chose}",
            advice.candidate_id,
            advice.point.as_str()
        );
        // The first item is always named; the ones that no longer fit are counted.
        if index > 0 && bytes + item.len() > NAMED_OPEN_BYTES {
            items.push(format!("and {} more", standing.len() - index));
            break;
        }
        bytes += item.len();
        items.push(item);
    }
    Ok(format!(
        "open advice on {}: {}",
        counted(standing.len()),
        items.join("; ")
    ))
}

fn counted(items: usize) -> String {
    format!("{items} item{}", if items == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests;
