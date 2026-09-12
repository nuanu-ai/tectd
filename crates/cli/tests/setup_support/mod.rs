use crate::recovery_support::Mcp;
use serde_json::json;
use sqlx::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone)]
pub struct DeniedTarget {
    pub setup_id: Uuid,
    pub revision: i64,
    pub directory: PathBuf,
    pub content: String,
    pub notes: String,
}

async fn setup_rows(pool: &PgPool, id: Uuid) -> Vec<String> {
    let mut rows = vec![
        sqlx::query_scalar::<_, String>(
            "SELECT xmin::text || ':' || row_to_json(s)::text \
             FROM workspace_setups s WHERE id=$1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap(),
    ];
    rows.extend(
        sqlx::query_scalar::<_, String>(
            "SELECT xmin::text || ':' || row_to_json(i)::text \
             FROM workspace_setup_inputs i WHERE setup_id=$1 ORDER BY sequence",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap(),
    );
    rows
}

pub async fn setup_table_counts(pool: &PgPool, tenant: Uuid) -> (i64, i64, i64) {
    let directories =
        sqlx::query_scalar("SELECT count(*) FROM setup_session_directories WHERE tenant_id=$1")
            .bind(tenant)
            .fetch_one(pool)
            .await
            .unwrap();
    let setups = sqlx::query_scalar("SELECT count(*) FROM workspace_setups WHERE tenant_id=$1")
        .bind(tenant)
        .fetch_one(pool)
        .await
        .unwrap();
    let inputs =
        sqlx::query_scalar("SELECT count(*) FROM workspace_setup_inputs WHERE tenant_id=$1")
            .bind(tenant)
            .fetch_one(pool)
            .await
            .unwrap();
    (directories, setups, inputs)
}

pub async fn assert_denied(pool: &PgPool, client: &mut Mcp, target: &DeniedTarget, expected: &str) {
    let before_rows = setup_rows(pool, target.setup_id).await;
    let file = target.directory.join("AGENTS.md");
    let before_file = std::fs::read(&file).ok();
    let calls = [
        (
            "get_setup",
            json!({"setup_id":target.setup_id,"after_input":0,"limit":25}),
        ),
        (
            "save_setup",
            json!({
                "setup_id":target.setup_id,"revision":target.revision,
                "input_cursor":1,"ready":false,"content":"attacker replacement"
            }),
        ),
        (
            "record_setup_input",
            json!({
                "setup_id":target.setup_id,"revision":target.revision,
                "request_id":Uuid::new_v4(),"input":"attacker input"
            }),
        ),
        (
            "apply_setup",
            json!({"setup_id":target.setup_id,"revision":target.revision}),
        ),
    ];
    for (name, arguments) in calls {
        let payload = client.call_error(name, arguments).await;
        assert_eq!(payload["error"]["code"], expected, "{name}: {payload}");
        let encoded = payload.to_string();
        assert!(!encoded.contains(&target.content), "{name} exposed content");
        assert!(!encoded.contains(&target.notes), "{name} exposed notes");
        assert!(payload.get("setup").is_none(), "{name} exposed setup");
        assert!(payload.get("inputs").is_none(), "{name} exposed inputs");
    }
    assert!(
        setup_rows(pool, target.setup_id).await == before_rows,
        "denied calls changed protected setup rows"
    );
    assert!(
        std::fs::read(&file).ok() == before_file,
        "denied calls changed protected setup file"
    );
}
