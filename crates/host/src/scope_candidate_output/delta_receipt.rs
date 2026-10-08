use super::*;
use tect_domain::CandidateDeltaReceipt;

pub(crate) fn delta(receipt: CandidateDeltaReceipt, capacity: usize) -> Result<Value> {
    let set = receipt.candidate_set_id;
    let context = current_context_action(set)?;
    let help =
        crate::api::schema_help_action("command", &json!({"route":"scope.candidates.save"}))?
            .ok_or(Error::InternalInvariant)?;
    let mut value = serde_json::to_value(receipt).map_err(Error::invalid_arguments_from)?;
    value["next_step"] = json!(
        "Read the current candidate context, then save a complete native draft with local labels for additions. Preserve existing candidate IDs and revisions. Use the native IDs assigned by that save for review and Scope opening; delta IDs cannot be reviewed or opened. Refresh captures context and does not import delta candidates."
    );
    within(with_actions(value, vec![context, help], Some(0)), capacity)
}
