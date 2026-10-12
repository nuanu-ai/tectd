use crate::planning_read::Window;
use serde_json::Value;
use tect_domain::{AdviceAuditQuery, ConfigureAdvice, Error, Result};

pub(crate) enum AdviceInvocation {
    Config,
    Configure(ConfigureAdvice),
    Audit {
        query: AdviceAuditQuery,
        window: Window,
    },
}

pub(crate) fn parse(name: &str, mut arguments: Value) -> Result<AdviceInvocation> {
    let Some(fields) = arguments.as_object() else {
        return Err(Error::InvalidArguments);
    };
    if fields.values().any(Value::is_null) {
        return Err(Error::InvalidArguments);
    }
    match name {
        "advice_config" if fields.is_empty() => Ok(AdviceInvocation::Config),
        "advice_configure" => {
            let request: ConfigureAdvice =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            request.validate()?;
            Ok(AdviceInvocation::Configure(request))
        }
        "advice_audit" => {
            let window = crate::planning_read::extract(&mut arguments)?;
            let query: AdviceAuditQuery =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            query.validate()?;
            Ok(AdviceInvocation::Audit { query, window })
        }
        _ => Err(Error::InvalidArguments),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn invalid(name: &str, arguments: Value) -> bool {
        parse(name, arguments).is_err()
    }

    #[test]
    fn setting_routes_accept_only_their_exact_parameters() {
        assert!(matches!(
            parse("advice_config", json!({})),
            Ok(AdviceInvocation::Config)
        ));
        assert!(invalid("advice_config", json!({"workspace_id":"other"})));
        assert!(invalid("advice_config", json!([])));

        let Ok(AdviceInvocation::Configure(request)) = parse(
            "advice_configure",
            json!({"enabled":true,"daily_call_limit":40,"expected_revision":0}),
        ) else {
            panic!("expected a configure request")
        };
        assert_eq!(
            (
                request.enabled,
                request.daily_call_limit,
                request.expected_revision
            ),
            (true, Some(40), 0)
        );
        assert!(
            parse(
                "advice_configure",
                json!({"enabled":false,"expected_revision":3})
            )
            .is_ok()
        );
        for arguments in [
            json!({"enabled":true,"expected_revision":0}),
            json!({"enabled":true,"daily_call_limit":0,"expected_revision":0}),
            json!({"enabled":true,"daily_call_limit":40}),
            json!({"enabled":true,"daily_call_limit":40,"expected_revision":-1}),
            json!({"enabled":true,"daily_call_limit":null,"expected_revision":0}),
            json!({"enabled":"yes","daily_call_limit":40,"expected_revision":0}),
            json!({"enabled":true,"daily_call_limit":40,"expected_revision":0,"session":"x"}),
        ] {
            assert!(invalid("advice_configure", arguments));
        }
    }

    #[test]
    fn journal_route_takes_filters_a_cursor_and_read_windows() {
        let id = uuid::Uuid::new_v4();
        let Ok(AdviceInvocation::Audit { query, window }) = parse(
            "advice_audit",
            json!({"limit":2,"bodies":true,"point":"sizing","outcome":"advised","after":id,
                "since":"2026-10-12T00:00:00Z","offset_bytes":4096,"limit_bytes":512,
                "representation_digest":"a".repeat(64)}),
        ) else {
            panic!("expected a journal query")
        };
        assert_eq!(
            (query.limit, query.after, query.bodies),
            (2, Some(id), true)
        );
        assert_eq!(window.offset_bytes, Some(4096));
        assert!(matches!(
            parse("advice_audit", json!({"limit":0,"bodies":false})),
            Ok(AdviceInvocation::Audit { query, .. }) if !query.bodies
        ));
        for arguments in [
            json!({}),
            json!({"limit":1}),
            json!({"bodies":true}),
            json!({"limit":1,"bodies":null}),
            json!({"limit":1,"bodies":"false"}),
            json!({"limit":101,"bodies":true}),
            json!({"limit":1,"bodies":true,"outcome":"pending"}),
            json!({"limit":1,"bodies":true,"point":"Sizing"}),
            json!({"limit":1,"bodies":true,"until":"tomorrow"}),
            json!({"limit":1,"bodies":true,"after":null}),
            json!({"limit":1,"bodies":true,"limit_bytes":0}),
            json!({"limit":1,"bodies":true,"sql":"select 1"}),
        ] {
            assert!(invalid("advice_audit", arguments));
        }
        assert!(invalid("advice_unknown", json!({})));
    }
}
