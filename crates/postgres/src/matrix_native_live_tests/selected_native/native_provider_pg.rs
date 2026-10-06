//! Actual native adapter authorization rejection on owned synthetic PG only.
use super::*;
use serde_json::{Value, json};
use tect_application::{SignedMatrixBudgetPreflight, Store};
use tect_host::jev_matrix_advice::native_provider::{
    JevNativeMatrixConfig, JevNativeMatrixProvider,
};
include!("native_provider_pg_fixture.rs");

#[tokio::test]
#[ignore = "requires exact owned isolated PostgreSQL fixture"]
async fn native_matrix_pg_rejects_loopback_destination_before_dispatch() {
    tokio::time::timeout(std::time::Duration::from_secs(60),async {
 let (admin_pool,runtime_pool)=crate::technical_decision_comparison_pg_tests::isolated_pg::connect_and_migrate().await;
 let runtime_url=std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
 let plain=Arc::new(PgStore::connect(&runtime_url,4).await.unwrap());
 let initial=fixtures::service(plain.clone());let owner=admin::enroll_host(&admin_pool,None,vec![]).await.unwrap();
 let owner_context=fixtures::context(&owner.auth,&format!("matrix-native-{}",Uuid::new_v4()));
 let workspace=initial.open_workspace(&owner_context).await.unwrap().workspace.unwrap().id;
 let (_,source,effective)=fixtures::bound_source(&initial,&admin_pool,&owner,&owner_context,workspace).await;
 let task=source.revision.task_id;
 let (budget,keys)=signed_test_budget(workspace,owner.principal_id);
 let store=Arc::new(plain.as_ref().clone().with_budget_owner_keys(keys));
 let (configured,evidence,profile,_,synthetic_sends)=matrix::configured(store.clone(),workspace,&source,&effective);
 let mut unit=store.begin(tect_application::TransactionMode::ReadWrite).await.unwrap();unit.authenticate(&owner.auth).await.unwrap();unit.set_tenant(owner.tenant_id).await.unwrap();unit.advisory_budget_policy_store().unwrap().install_budget_policy(workspace,&budget).await.unwrap();unit.commit().await.unwrap();
 let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();let endpoint=reqwest::Url::parse(&format!("http://{}/v1/systemone",listener.local_addr().unwrap())).unwrap();
 let model=AdvisoryModelConfiguration{model:"jev-1.13.0".into()};
 let provider=JevNativeMatrixProvider::new(JevNativeMatrixConfig{provider_identity:MatrixProviderIdentity{provider_profile_ref:profile.clone(),model_configuration:model.clone(),destination:endpoint.to_string(),wire_version:"tect.matrix-typesafe-native/1".into(),ranking_policy:MatrixRankingPolicy::StrictV1},endpoint,timeout:std::time::Duration::from_secs(2),maximum_request_bytes:45_000,maximum_response_bytes:4096},"synthetic-local-key".into()).unwrap();
 // Preserve synthetic evidence validator but replace only the actual transport and signed budget adapter.
 let configured=configured.with_matrix_advisory_adapters(Arc::new(provider),Arc::new(SignedMatrixBudgetPreflight));
 let verifier=admin::prepare_verifier_enrollment(&admin_pool,owner.tenant_id,workspace).await.unwrap().try_commit().await.unwrap();assert_ne!(verifier.principal_id,owner.principal_id);
 let verifier_context=fixtures::context(&verifier.auth,&owner_context.workspace_key);configured.open_workspace(&verifier_context).await.unwrap();
 let VerifiedMatrixTask::Context(record)=configured.verify_matrix_task(&verifier_context,&VerifyMatrixTask{task_id:task,expected_revision:1,input_digest:source.revision.input_digest.clone(),evidence}).await.unwrap() else{panic!("ContextV2 required")};assert_eq!(record.verifier_principal,verifier.principal_id.to_string());
 configured.configure_advisory(&owner_context,&ConfigureWorkspaceAdvisory{expected_revision:0,mode:WorkspaceAdvisoryMode::Optional,provider_profile_ref:Some(profile),model_configuration:Some(model)}).await.unwrap();
 let request=RequestEngineeringAdvisory{task_id:task,expected_task_revision:1,request_key:format!("native-positive-{}",Uuid::new_v4()),session_preference:AdvisoryRequestPreference::UseWorkspace,request_preference:AdvisoryRequestPreference::UseWorkspace};
 let result=configured.request_engineering_advisory(&owner_context,&request).await;
 assert_eq!(result,Err(Error::InputConflict),"PG authority requires exact TypeSafe destination");
 assert!(tokio::time::timeout(std::time::Duration::from_millis(150),listener.accept()).await.is_err(),"rejected authorization performs no send");
 assert_eq!(synthetic_sends.load(Ordering::SeqCst),0);
 let dispatches:i64=sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2").bind(owner.tenant_id).bind(workspace).fetch_one(&admin_pool).await.unwrap();assert_eq!(dispatches,0);
 let observations:i64=sqlx::query_scalar("SELECT count(*) FROM advisory_provider_observations WHERE tenant_id=$1 AND workspace_id=$2").bind(owner.tenant_id).bind(workspace).fetch_one(&admin_pool).await.unwrap();assert_eq!(observations,0);
 println!("native Matrix synthetic PG guard: distinct verification retained; exact-destination rejection, zero dispatch/raw/provider sends");
 runtime_pool.close().await;admin_pool.close().await;
 }).await.expect("finite synthetic fixture deadline");
}
