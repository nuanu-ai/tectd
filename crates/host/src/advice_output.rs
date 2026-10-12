use crate::planning_read::Window;
use crate::responses;
use serde_json::{Value, json};
use tect_domain::{AdviceAuditPage, AdviceAuditQuery, AdviceConfigStatus, Error, Result};

/// The setting as one flat object. A workspace without a row is disabled at revision 0.
pub(crate) fn status(status: &AdviceConfigStatus) -> Value {
    let config = status.config.as_ref();
    responses::with_actions(
        json!({
            "enabled": config.is_some_and(|config| config.enabled),
            "daily_call_limit": config.and_then(|config| config.daily_call_limit),
            "revision": config.map_or(0, |config| config.revision),
            "updated_at": config.map(|config| &config.updated_at),
            "updated_by_session": config.map(|config| config.updated_by_session),
            "calls_today": status.calls_today,
            "provider_configured": status.provider_configured,
        }),
        Vec::new(),
        None,
    )
}

/// The journal page whole, or as byte windows once it exceeds the read budget.
pub(crate) fn audit(
    page: &AdviceAuditPage,
    query: &AdviceAuditQuery,
    window: &Window,
    capacity: usize,
) -> Result<Value> {
    let params = serde_json::to_value(query).map_err(|_| Error::TransportUnavailable)?;
    let next_page = match page.next_after {
        Some(after) => {
            let mut next = params.clone();
            next["after"] = json!(after);
            vec![responses::action("advice_audit", next)?]
        }
        None => Vec::new(),
    };
    crate::json_fragment::encode(
        page,
        next_page,
        capacity,
        window.borrowed(),
        json!({"route":"workspace.advisory.audit"}),
        "advice_audit",
        params,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{
        AdviceConfig, AdviceCounters, AdviceField, AdviceJournalEntry, AdviceOutcome,
        AdviceSendCertainty, AdviceWorkRefs,
    };
    use uuid::Uuid;

    fn entry(request_body: &str) -> AdviceJournalEntry {
        AdviceJournalEntry {
            id: Uuid::new_v4(),
            created_at: "2026-10-12T09:00:00.000000Z".into(),
            workspace_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            point: "sizing".into(),
            route: "scope.candidates.save".into(),
            work: AdviceWorkRefs::default(),
            called: true,
            outcome: Some(AdviceOutcome::Abstained),
            reason: None,
            send_certainty: AdviceSendCertainty::Sent,
            model: Some("jev-latest".into()),
            request_bytes: Some(request_body.len() as i64),
            response_bytes: Some(2),
            input_tokens: None,
            output_tokens: None,
            latency_ms: Some(12),
            request_body: Some(request_body.into()),
            response_body: Some("{}".into()),
            advice: Some(AdviceField::Abstained),
        }
    }

    fn query(limit: u32) -> AdviceAuditQuery {
        serde_json::from_value(json!({"limit":limit,"point":"sizing"})).unwrap()
    }

    #[test]
    fn absent_setting_reads_as_disabled_at_revision_zero() {
        let absent = status(&AdviceConfigStatus {
            config: None,
            calls_today: 0,
            provider_configured: false,
        });
        assert_eq!(
            absent,
            json!({"enabled":false,"daily_call_limit":null,"revision":0,"updated_at":null,
                "updated_by_session":null,"calls_today":0,"provider_configured":false,
                "actions":[],"recommended_action":null})
        );
        let session = Uuid::new_v4();
        let enabled = status(&AdviceConfigStatus {
            config: Some(AdviceConfig {
                enabled: true,
                daily_call_limit: Some(40),
                revision: 2,
                updated_at: "2026-10-12T09:00:00.000000Z".into(),
                updated_by_session: session,
            }),
            calls_today: 7,
            provider_configured: true,
        });
        assert_eq!(enabled["enabled"], true);
        assert_eq!(enabled["daily_call_limit"], 40);
        assert_eq!(enabled["revision"], 2);
        assert_eq!(enabled["updated_by_session"], json!(session));
        assert_eq!(enabled["calls_today"], 7);
    }

    #[test]
    fn small_page_is_whole_and_offers_the_next_page() {
        let mut page = AdviceAuditPage {
            counters: AdviceCounters::default(),
            entries: vec![entry("{}")],
            next_after: None,
        };
        let last = audit(&page, &query(1), &Window::default(), 8192).unwrap();
        assert_eq!(last["entries"][0]["advice"], json!({"status":"abstained"}));
        assert_eq!(last["entries"][0]["request_body"], "{}");
        assert_eq!(last["entries"][0]["input_tokens"], Value::Null);
        assert_eq!(last["actions"], json!([]));

        page.next_after = Some(page.entries[0].id);
        let more = audit(&page, &query(1), &Window::default(), 8192).unwrap();
        let next = &more["actions"][0]["arguments"];
        assert_eq!(next["route"], "workspace.advisory.audit");
        assert_eq!(
            next["params"],
            json!({"limit":1,"point":"sizing","after":page.entries[0].id})
        );
    }

    #[test]
    fn page_with_bodies_over_the_read_budget_reassembles_from_byte_windows() {
        let page = AdviceAuditPage {
            counters: AdviceCounters::default(),
            entries: vec![entry(&"task text ".repeat(2_000))],
            next_after: None,
        };
        // Windows cut the canonical JSON value of the page, not the struct field order.
        let expected = serde_json::to_vec(&serde_json::to_value(&page).unwrap()).unwrap();
        let mut window = Window::default();
        let mut assembled = Vec::new();
        loop {
            let part = audit(&page, &query(1), &window, 8192).unwrap();
            assert!(responses::encoded_len(&part).unwrap() <= 8192);
            assert_eq!(part["kind"], "fragment");
            assembled.extend_from_slice(part["text"].as_str().unwrap().as_bytes());
            let Some(next) = part["actions"].as_array().unwrap().first() else {
                break;
            };
            let call = crate::api::decode_public_call("query", next["arguments"].clone()).unwrap();
            let Ok(crate::advice_tools::AdviceInvocation::Audit { window: next, .. }) =
                crate::advice_tools::parse(call.name, call.arguments)
            else {
                panic!("continuation must be a journal query")
            };
            window = next;
        }
        assert_eq!(assembled, expected);
        assert_eq!(
            serde_json::from_slice::<AdviceAuditPage>(&assembled).unwrap(),
            page
        );
    }
}
