//! Stateless, bound workspace state reads; fragment pins cover one exact document/page.
use crate::planning_read::Window;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tect_application::WorkspaceService;
use tect_domain::{Error, RequestContext, Result};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum View {
    Root,
    CandidateSets,
    NativePlanning,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Origin {
    Opened,
    State,
    Discovery,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Params {
    pub view: View,
    pub origin: Origin,
    pub action_seed: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_capacity: Option<usize>,
}
pub(crate) struct Query {
    pub params: Params,
    pub window: Window,
}
pub(crate) fn parse(mut arguments: Value) -> Result<Query> {
    if arguments
        .as_object()
        .is_none_or(|o| o.values().any(Value::is_null))
    {
        return Err(Error::InvalidArguments);
    }
    let window = crate::planning_read::extract(&mut arguments)?;
    let params: Params =
        serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
    if params.action_seed.is_nil()
        || params.limit.is_some_and(|n| !(1..=25).contains(&n))
        || params.view == View::Root && (params.after.is_some() || params.limit.is_some())
        || params.origin != Origin::Discovery
            && (params.task_directory.is_some() || params.observation_capacity.is_some())
        || params
            .observation_capacity
            .is_some_and(|n| n > crate::frame::MAX_FRAME_BYTES)
    {
        return Err(Error::InvalidArguments);
    }
    if let Some(path) = params.task_directory.as_deref() {
        tect_domain::validate_setup_path(path)?;
    }
    Ok(Query { params, window })
}
pub(crate) fn schema() -> Value {
    crate::planning_read::schema(json!({"type":"object","additionalProperties":false,
    "required":["view","origin","action_seed"],"properties":{
        "view":{"enum":["root","candidate_sets","native_planning"]},
        "origin":{"enum":["opened","state","discovery"]},
        "action_seed":{"type":"string","format":"uuid"},
        "after":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":25},
        "task_directory":{"type":"string"},"observation_capacity":{"type":"integer","minimum":0,"maximum":crate::frame::MAX_FRAME_BYTES}
    },"allOf":[
        {"if":{"properties":{"view":{"const":"root"}}},"then":{"not":{"anyOf":[{"required":["after"]},{"required":["limit"]}]}}},
        {"if":{"properties":{"origin":{"enum":["opened","state"]}}},"then":{"not":{"anyOf":[{"required":["task_directory"]},{"required":["observation_capacity"]}]}}}
    ]}))
}
pub(crate) fn params(
    origin: Origin,
    seed: Uuid,
    task_directory: Option<String>,
    observation_capacity: Option<usize>,
) -> Params {
    Params {
        view: View::Root,
        origin,
        action_seed: seed,
        after: None,
        limit: None,
        task_directory,
        observation_capacity,
    }
}
pub(crate) fn action(params: &Params) -> Result<Value> {
    crate::api::ready_action(
        "workspace_state",
        serde_json::to_value(params).map_err(|_| Error::TransportUnavailable)?,
    )
}
pub(crate) fn read(
    value: Value,
    query: &Query,
    capacity: usize,
    terminal: Vec<Value>,
) -> Result<Value> {
    crate::json_fragment::encode(
        &json!({"state":value}),
        terminal,
        capacity,
        query.window.borrowed(),
        json!({"view":query.params.view,"origin":query.params.origin}),
        "workspace_state",
        serde_json::to_value(&query.params).map_err(|_| Error::TransportUnavailable)?,
    )
}
pub(crate) async fn execute(
    context: &RequestContext,
    service: &WorkspaceService,
    query: Query,
    capacity: usize,
) -> Result<Value> {
    let p = &query.params;
    if p.view == View::Root {
        let (state, file) = match p.origin {
            Origin::Opened => (service.get_state(context).await?, None),
            Origin::State => (crate::slice_dispatch::state(context, service).await?, None),
            Origin::Discovery => {
                let discovery = service
                    .inspect_setup_readonly(
                        context,
                        p.task_directory.as_deref(),
                        p.observation_capacity
                            .unwrap_or(crate::frame::MAX_FRAME_BYTES),
                    )
                    .await?;
                (discovery.state, Some(discovery.file))
            }
        };
        let value =
            crate::workspace_output::logical(&state, file.as_ref(), p.origin == Origin::Opened, p)?;
        return read(value, &query, capacity, vec![]);
    }
    let (mut value, calls, next) = match p.view {
        View::CandidateSets => {
            let (workspace, session, page) = service
                .read_candidate_sets_bound(context, p.after.as_deref(), p.limit.unwrap_or(25))
                .await?;
            let calls = crate::workspace_output::candidate_actions(&page.candidate_sets)?;
            (
                json!({"view":p.view,"workspace":workspace,"session":session,"candidate_sets":page.candidate_sets,"next_after":page.next_after}),
                calls,
                page.next_after,
            )
        }
        View::NativePlanning => {
            let (workspace, session, mut page) = service
                .read_native_planning_bound(context, p.after.as_deref(), p.limit.unwrap_or(25))
                .await?;
            if p.origin == Origin::State {
                crate::slice_dispatch::enrich_native(context, service, &mut page.native_planning)
                    .await?;
            }
            let mut calls = vec![];
            for summary in &page.native_planning {
                calls.extend(crate::workspace_output::native_actions(summary)?);
            }
            (
                json!({"view":p.view,"workspace":workspace,"session":session,"native_planning":page.native_planning,"next_after":page.next_after}),
                calls,
                page.next_after,
            )
        }
        View::Root => unreachable!(),
    };
    let terminal = if let Some(next) = next {
        let mut params = p.clone();
        params.after = Some(next.encode());
        vec![action(&params)?]
    } else {
        vec![]
    };
    let mut business = calls;
    business.extend(terminal.clone());
    let recommended = (!business.is_empty()).then_some(0);
    value = crate::responses::with_actions(value, business, recommended);
    read(value, &query, capacity, terminal)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_workspace_route_parameters() {
        let good = json!({"view":"root","origin":"opened","action_seed":Uuid::new_v4()});
        assert!(parse(good.clone()).is_ok());
        for (key, value) in [
            ("limit", json!(25)),
            ("after", json!("w:bad")),
            ("workspace_id", json!(Uuid::new_v4())),
            ("task_directory", json!("/tmp")),
            ("offset_bytes", Value::Null),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(parse(bad).is_err());
        }
        let call = crate::api::decode_public_call(
            "query",
            json!({"route":"workspace_state","params":good}),
        )
        .unwrap();
        assert_eq!(call.name, "workspace_state");
        assert_eq!(
            crate::api::definitions()["tools"].as_array().unwrap().len(),
            5
        );
        assert!(crate::api::decode_public_call("get_state", json!({})).is_ok());
        let help = crate::api::help(
            crate::api::parse_help(
                json!({"mode":"describe","tool":"query","route":"workspace_state"}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            help["params_schema"]["properties"]["limit_bytes"]["maximum"],
            4096
        );
        assert_eq!(
            help["params_schema"]["properties"]["view"]["enum"],
            json!(["root", "candidate_sets", "native_planning"])
        );
    }
}
