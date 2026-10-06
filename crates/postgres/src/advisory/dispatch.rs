mod authored_source {
    include!("dispatch/authored_source.rs");
}
use authored_source::{
    authored_scope_source, require_current_authored_scope_source,
    terminalize_stale_authored_dispatch,
};

include!("dispatch/authorization.rs");
include!("dispatch/budget_reservation.rs");
include!("dispatch/budget_consumption.rs");
include!("dispatch/lifecycle.rs");
include!("dispatch/finalization.rs");
