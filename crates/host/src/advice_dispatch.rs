use crate::advice_tools::AdviceInvocation;
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{RequestContext, Result};

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: AdviceInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    match invocation {
        AdviceInvocation::Config => service
            .advice_config(context)
            .await
            .map(|status| crate::advice_output::status(&status)),
        AdviceInvocation::Configure(request) => service
            .configure_advice(context, &request)
            .await
            .map(|status| crate::advice_output::status(&status)),
        AdviceInvocation::Audit { query, window } => service
            .advice_audit(context, &query)
            .await
            .and_then(|page| crate::advice_output::audit(&page, &query, &window, capacity)),
    }
}
