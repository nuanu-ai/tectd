//! Actual authenticated Unix host wire; technical approval is synthetic only.
use super::*;
use std::os::unix::fs::PermissionsExt;
use tokio::net::UnixListener;

fn params(request: &CompareTechnicalDeliveryMechanisms) -> Value {
    json!({"task_id":request.task_id,"expected_task_revision":request.expected_task_revision,
        "operating_verification_digest":request.operating_verification_digest,
        "evidence_reference":{"artifact_id":request.evidence_reference.artifact_id,
            "artifact_version":request.evidence_reference.artifact_version,
            "content_sha256":request.evidence_reference.content_sha256}})
}

#[tokio::test]
#[ignore = "requires exact owned socket-only PG18.6; SYNTHETIC approval control"]
async fn approved_matrix_technical_host_wire_preserves_effects_and_denies_stale_revoked_workspace()
{
    let f = control_fixture().await;
    let svc = Arc::new(service(&f.runtime, &f.operating, vec![f.approved.clone()]));
    let directory =
        std::path::PathBuf::from("/private/tmp").join(format!("jev-v05-wire-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, svc));
    let before = read_effect_counts(&f.admin_pool, f.owner.tenant_id).await;
    for _ in 0..2 {
        let result = tect_host::call_tool(
            &socket,
            &f.compare_context,
            "compare_technical_delivery_mechanisms",
            params(&f.request),
        )
        .await
        .unwrap();
        assert_eq!(result["state"], "compared");
        assert_eq!(
            result["comparison"]["eligible_approach_ids"],
            json!(["reuse"])
        );
        assert_eq!(
            read_effect_counts(&f.admin_pool, f.owner.tenant_id).await,
            before,
            "read/repeated read cannot create advice or caller effects"
        );
    }
    let mut stale = f.request.clone();
    stale.expected_task_revision += 1;
    assert!(matches!(
        tect_host::call_tool(
            &socket,
            &f.compare_context,
            "compare_technical_delivery_mechanisms",
            params(&stale)
        )
        .await,
        Err(Error::StaleRevision)
    ));
    let mut foreign = f.compare_context.clone();
    foreign.workspace_key = format!("foreign-{}", Uuid::new_v4());
    assert!(matches!(
        tect_host::call_tool(
            &socket,
            &foreign,
            "compare_technical_delivery_mechanisms",
            params(&f.request)
        )
        .await,
        Err(Error::SessionWorkspaceMismatch)
    ));
    sqlx::query("UPDATE agent_sessions SET revoked=true WHERE id=$1")
        .bind(f.sessions[1])
        .execute(&f.admin_pool)
        .await
        .unwrap();
    assert!(matches!(
        tect_host::call_tool(
            &socket,
            &f.compare_context,
            "compare_technical_delivery_mechanisms",
            params(&f.request)
        )
        .await,
        Err(Error::SessionRevoked)
    ));
    assert_eq!(
        read_effect_counts(&f.admin_pool, f.owner.tenant_id).await,
        before,
        "stale/revoked/wrong workspace cannot persist forbidden effects"
    );
    server.abort();
    let _ = server.await;
    std::fs::remove_file(socket).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
