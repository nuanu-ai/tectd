use super::*;

impl EscalatePipelineDelivery {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil() {
            return Err(schema_refusal(
                "WP6-DELIVERY-ESCALATE-REQUEST-ID",
                "arguments.params.request_id",
                "non-nil request UUID",
                "nil UUID",
            ));
        }
        if self.run_id.is_nil() {
            return Err(schema_refusal(
                "WP6-DELIVERY-ESCALATE-RUN-ID",
                "arguments.params.run_id",
                "non-nil run UUID",
                "nil UUID",
            ));
        }
        if self.run_revision < 1 {
            return Err(schema_refusal(
                "WP6-DELIVERY-ESCALATE-REVISION",
                "arguments.params.run_revision",
                "positive run revision",
                "nonpositive",
            ));
        }
        if self.phase_id.trim().is_empty() {
            return Err(schema_refusal(
                "WP6-DELIVERY-ESCALATE-PHASE-ID",
                "arguments.params.phase_id",
                "nonblank phase ID",
                "blank",
            ));
        }
        if self.reason.trim().is_empty() {
            return Err(schema_refusal(
                "WP6-DELIVERY-ESCALATE-REASON",
                "arguments.params.reason",
                "nonblank reason",
                "blank",
            ));
        }
        Ok(())
    }
}
