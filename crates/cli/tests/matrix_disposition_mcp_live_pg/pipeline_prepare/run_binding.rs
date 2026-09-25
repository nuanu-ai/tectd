use super::*;

pub(super) async fn exercise(
    pool: &PgPool,
    workspace: Uuid,
    owner: &mut Mcp,
    opened: &Value,
    scope: Uuid,
) {
    let slice = &opened["created"];
    let begin = json!({
        "request_id": Uuid::new_v4(),
        "scope_id": scope,
        "slice_id": slice["id"],
        "slice_revision": slice["revision"],
        "definition_version": slice["verification_plan_source_definition_version"],
        "qualification_reason": "Owner explicitly starts the selected synthetic pipeline."
    });
    let begun = route(owner, "command", "slice.pipeline.begin", begin.clone()).await;
    let run = &begun["created"]["run"];
    assert_eq!(run["selected_option_id"], slice["selected_option_id"]);
    assert_eq!(run["verification_plan_id"], slice["verification_plan_id"]);
    assert_eq!(
        run["verification_plan_version"],
        slice["verification_plan_source_definition_version"]
    );
    assert_eq!(
        run["verification_plan_digest"],
        slice["verification_plan_digest"]
    );

    let stored: (String, String, String, String) = sqlx::query_as(
        "SELECT selected_option_id,verification_plan_id,verification_plan_version,\
         verification_plan_digest FROM slice_pipeline_runs WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(Uuid::parse_str(run["id"].as_str().unwrap()).unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(stored.0, slice["selected_option_id"].as_str().unwrap());
    assert_eq!(stored.1, slice["verification_plan_id"].as_str().unwrap());
    assert_eq!(
        stored.2,
        slice["verification_plan_source_definition_version"]
            .as_str()
            .unwrap()
    );
    assert_eq!(
        stored.3,
        slice["verification_plan_digest"].as_str().unwrap()
    );
    let replay = route(owner, "command", "slice.pipeline.begin", begin).await;
    assert_eq!(replay["replay"]["run"], *run);
}
