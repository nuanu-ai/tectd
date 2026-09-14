use crate::storage_error;
use sqlx::PgPool;
use tect_domain::{Error, Result};

const SHAPES: &str = r#"
@prefix dk: <urn:tect:dk:> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
dk:ConstraintRevisionShape a sh:NodeShape ; sh:targetClass dk:ConstraintRevision ;
 sh:property [ sh:path dk:revision ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:title ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:statement ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:modality ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ; sh:in ( dk:must dk:must_not ) ] ;
 sh:property [ sh:path dk:action ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:target ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ] ;
 sh:property [ sh:path dk:conditionCount ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:exceptionCount ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:condition ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:exception ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:source ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ] ;
 sh:property [ sh:path dk:purpose ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ; sh:in ( dk:execution_constraint ) ] ;
 sh:property [ sh:path dk:versionResolution ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ; sh:in ( dk:current_accepted ) ] ;
 sh:property [ sh:path dk:bindingKind ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ; sh:in ( dk:workspace dk:slice_phase ) ] ;
 sh:property [ sh:path dk:unit ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ] .
dk:SourceFragmentShape a sh:NodeShape ; sh:targetClass dk:SourceFragment ;
 sh:property [ sh:path dk:title ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:uri ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:text ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:sha256 ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] .
dk:PublicationEventShape a sh:NodeShape ; sh:targetClass dk:PublicationEvent ;
 sh:property [ sh:path dk:unit ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ] ;
 sh:property [ sh:path dk:revisionRef ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ] ;
 sh:property [ sh:path dk:operation ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ; sh:in ( dk:create dk:revise dk:retract ) ] ;
 sh:property [ sh:path dk:reason ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:authorityBasis ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path dk:actorPrincipal ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ] ;
 sh:property [ sh:path dk:actorSession ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ] ;
 sh:property [ sh:path dk:profile ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI ; sh:in ( dk:general_constraint ) ] ;
 sh:property [ sh:path dk:profileVersion ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] .
"#;

const VALID: &str = r#"
@prefix dk: <urn:tect:dk:> .
<urn:tect:dk:activation:revision> a dk:ConstraintRevision ; dk:revision "1" ; dk:title "t" ; dk:statement "s" ; dk:modality dk:must ; dk:action "a" ; dk:target <urn:target> ; dk:conditionCount "0" ; dk:exceptionCount "0" ; dk:source <urn:tect:dk:activation:source> ; dk:purpose dk:execution_constraint ; dk:versionResolution dk:current_accepted ; dk:bindingKind dk:workspace ; dk:unit <urn:tect:dk:activation:unit> .
<urn:tect:dk:activation:source> a dk:SourceFragment ; dk:title "s" ; dk:uri "urn:source" ; dk:text "source" ; dk:sha256 "00" .
<urn:tect:dk:activation:event> a dk:PublicationEvent ; dk:unit <urn:tect:dk:activation:unit> ; dk:revisionRef <urn:tect:dk:activation:revision> ; dk:operation dk:create ; dk:reason "r" ; dk:authorityBasis "a" ; dk:actorPrincipal <urn:tect:principal:00000000-0000-0000-0000-000000000001> ; dk:actorSession <urn:tect:session:00000000-0000-0000-0000-000000000002> ; dk:profile dk:general_constraint ; dk:profileVersion "dk-1" .
"#;

const INVALID: &str = r#"
@prefix dk: <urn:tect:dk:> .
<urn:tect:dk:activation:invalid> a dk:ConstraintRevision ; dk:revision "1" ; dk:title "t" ; dk:statement "s" ; dk:modality "must" ; dk:action "a" ; dk:target <urn:target> ; dk:conditionCount "0" ; dk:exceptionCount "0" ; dk:source <urn:tect:dk:activation:invalid-source> ; dk:purpose dk:execution_constraint ; dk:versionResolution dk:current_accepted ; dk:bindingKind dk:workspace ; dk:unit <urn:tect:dk:activation:unit> .
<urn:tect:dk:activation:invalid-source> a dk:SourceFragment ; dk:title "s" ; dk:uri "urn:source" ; dk:text "source" ; dk:sha256 "00" .
"#;

pub async fn enable_durable_knowledge(pool: &PgPool, runtime_role: &str) -> Result<()> {
    let role = quote_identifier(runtime_role)?;
    let mut tx = pool.begin().await.map_err(storage_error)?;
    sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended('tect-dk-native-publisher',0))").execute(&mut *tx).await.map_err(storage_error)?;
    let stored_identity: (Option<String>, Option<i64>) = sqlx::query_as(
        "SELECT qualified_system_identifier,qualified_database_oid::bigint \
         FROM durable_knowledge_capability WHERE singleton FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(storage_error)?;
    let actual_identity =
        crate::knowledge_recovery::current_knowledge_database_identity_in_tx(&mut tx).await?;
    match stored_identity {
        (None, None) => {}
        (Some(system_identifier), Some(database_oid))
            if system_identifier == actual_identity.system_identifier
                && database_oid == i64::from(actual_identity.database_oid) => {}
        (Some(_), Some(_)) => return Err(Error::KnowledgeUnavailable),
        _ => return Err(Error::InvalidConfiguration),
    }
    sqlx::query("SELECT tenant_id,workspace_id FROM workspace_knowledge_state ORDER BY tenant_id,workspace_id FOR UPDATE")
        .fetch_all(&mut *tx).await.map_err(storage_error)?;
    let current: String = sqlx::query_scalar("SELECT CURRENT_USER")
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
    let extension:Option<(String,String,String)>=sqlx::query_as("SELECT e.extversion,r.rolname,n.nspname FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_roles r ON r.oid=e.extowner JOIN pg_catalog.pg_namespace n ON n.oid=e.extnamespace WHERE e.extname='pgrdf'").fetch_optional(&mut *tx).await.map_err(storage_error)?;
    if extension.is_none() {
        sqlx::query("CREATE EXTENSION pgrdf VERSION '0.6.34'")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
    }
    let (version,owner,schema):(String,String,String)=sqlx::query_as("SELECT e.extversion,r.rolname,n.nspname FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_roles r ON r.oid=e.extowner JOIN pg_catalog.pg_namespace n ON n.oid=e.extnamespace WHERE e.extname='pgrdf'").fetch_one(&mut *tx).await.map_err(storage_error)?;
    if version != "0.6.34" || owner != current || schema != "pgrdf" {
        return Err(Error::InvalidConfiguration);
    }
    let identity: (String, String) = sqlx::query_as("SELECT pgrdf.version(),pgrdf.build_id()")
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
    if identity.0 != "0.6.34" || identity.1 != "v0.6.34" {
        return Err(Error::InvalidConfiguration);
    }
    for statement in [
        format!("REVOKE ALL PRIVILEGES ON SCHEMA pgrdf FROM PUBLIC,{role}"),
        format!("REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA pgrdf FROM PUBLIC,{role}"),
        format!("REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA pgrdf FROM PUBLIC,{role}"),
        format!("REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA pgrdf FROM PUBLIC,{role}"),
        format!("REVOKE ALL PRIVILEGES ON ALL ROUTINES IN SCHEMA pgrdf FROM PUBLIC,{role}"),
    ] {
        sqlx::query(&statement)
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
    }
    let shapes: i64 = sqlx::query_scalar("SELECT pgrdf.add_graph('urn:tect:dk:shapes:dk-1')")
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
    sqlx::query("SELECT pgrdf.clear_graph($1)")
        .bind(shapes)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    sqlx::query("SELECT pgrdf.parse_turtle($1,$2)")
        .bind(SHAPES)
        .bind(shapes)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    let valid: i64 = sqlx::query_scalar("SELECT pgrdf.add_graph('urn:tect:dk:activation:valid')")
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
    let invalid: i64 =
        sqlx::query_scalar("SELECT pgrdf.add_graph('urn:tect:dk:activation:invalid')")
            .fetch_one(&mut *tx)
            .await
            .map_err(storage_error)?;
    sqlx::query("SELECT pgrdf.clear_graph($1)")
        .bind(valid)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    sqlx::query("SELECT pgrdf.clear_graph($1)")
        .bind(invalid)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    sqlx::query("SELECT pgrdf.parse_turtle($1,$2)")
        .bind(VALID)
        .bind(valid)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    sqlx::query("SELECT pgrdf.parse_turtle($1,$2)")
        .bind(INVALID)
        .bind(invalid)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    let good: serde_json::Value = sqlx::query_scalar("SELECT pgrdf.validate($1,$2,'native',true)")
        .bind(valid)
        .bind(shapes)
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
    let bad: serde_json::Value = sqlx::query_scalar("SELECT pgrdf.validate($1,$2,'native',true)")
        .bind(invalid)
        .bind(shapes)
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
    let nonvacuous:i64=sqlx::query_scalar("SELECT count(*) FROM pgrdf.construct('CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <urn:tect:dk:activation:valid> { ?s <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:tect:dk:ConstraintRevision> ; ?p ?o } }')").fetch_one(&mut *tx).await.map_err(storage_error)?;
    if good.get("conforms").and_then(|v| v.as_bool()) != Some(true)
        || bad.get("conforms").and_then(|v| v.as_bool()) != Some(false)
        || nonvacuous == 0
    {
        return Err(Error::InvalidConfiguration);
    }
    if sqlx::query_scalar::<_,bool>("SELECT pg_catalog.to_regprocedure('public.tect_dk2_native_publish(uuid,uuid,uuid,text,text,text)') IS NOT NULL")
        .fetch_one(&mut *tx).await.map_err(storage_error)?
    {
        crate::knowledge_lifecycle::rdf::qualify_native(&mut tx).await?;
    }
    sqlx::query("SELECT pgrdf.drop_graph($1,true)")
        .bind(valid)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    sqlx::query("SELECT pgrdf.drop_graph($1,true)")
        .bind(invalid)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    sqlx::query("UPDATE durable_knowledge_capability SET capability_ready=true,pgrdf_version='0.6.34',activated_at=pg_catalog.clock_timestamp(),qualified_system_identifier=$1,qualified_database_oid=$2::bigint::oid,qualified_at=pg_catalog.clock_timestamp() WHERE singleton")
        .bind(&actual_identity.system_identifier).bind(i64::from(actual_identity.database_oid))
        .execute(&mut *tx).await.map_err(storage_error)?;
    sqlx::query("INSERT INTO workspace_knowledge_state(tenant_id,workspace_id,capability_ready,pgrdf_version,activated_at) SELECT tenant_id,id,true,'0.6.34',pg_catalog.clock_timestamp() FROM workspaces ON CONFLICT(tenant_id,workspace_id) DO UPDATE SET capability_ready=true,pgrdf_version='0.6.34',activated_at=pg_catalog.clock_timestamp()")
        .execute(&mut *tx).await.map_err(storage_error)?;
    crate::knowledge_search_admin::grant_search_runtime(&mut tx, runtime_role).await?;
    crate::knowledge_search_admin::backfill_all(&mut tx).await?;
    for statement in [
        format!(
            "REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk_internal_native_publish(uuid,uuid,uuid,text,text,text),public.tect_dk_internal_native_read(uuid,uuid,uuid,bigint,uuid),public.tect_dk_internal_native_owned_residual(uuid,uuid,uuid),public.tect_dk2_internal_native_publish(uuid,uuid,uuid,text,text,text),public.tect_dk2_internal_native_read(uuid,uuid,uuid,bigint,uuid,boolean),public.tect_dk_internal_native_erase(uuid,uuid,uuid),public.tect_dk_internal_capability() FROM {role}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_dk_database_identity_ready(),public.tect_dk_native_publish(uuid,uuid,uuid,text,text,text),public.tect_dk_native_read(uuid,uuid,uuid,bigint,uuid),public.tect_dk_native_owned_residual(uuid,uuid,uuid),public.tect_dk2_native_publish(uuid,uuid,uuid,text,text,text),public.tect_dk2_native_read(uuid,uuid,uuid,bigint,uuid,boolean),public.tect_dk_native_erase(uuid,uuid,uuid) TO {role}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_dk_erased_no_change_proof_valid(jsonb) TO {role}"
        ),
    ] {
        sqlx::query(&statement)
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
    }
    let native_access:bool=sqlx::query_scalar("SELECT pg_catalog.has_schema_privilege($1,'pgrdf','USAGE') OR EXISTS(SELECT 1 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='pgrdf' AND pg_catalog.has_function_privilege($1,p.oid,'EXECUTE')) OR EXISTS(SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='pgrdf' AND (CASE WHEN c.relkind='S' THEN pg_catalog.has_sequence_privilege($1,c.oid,'USAGE') ELSE pg_catalog.has_table_privilege($1,c.oid,'SELECT') END))")
        .bind(runtime_role).fetch_one(&mut *tx).await.map_err(storage_error)?;
    let owns_wrapper:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='public' AND p.proname IN ('tect_dk_native_publish','tect_dk_native_read','tect_dk_native_owned_residual','tect_dk2_native_publish','tect_dk2_native_read','tect_dk_native_erase','tect_dk_internal_native_publish','tect_dk_internal_native_read','tect_dk_internal_native_owned_residual','tect_dk2_internal_native_publish','tect_dk2_internal_native_read','tect_dk_internal_native_erase','tect_dk_internal_capability') AND pg_catalog.pg_has_role($1,p.proowner,'MEMBER'))").bind(runtime_role).fetch_one(&mut *tx).await.map_err(storage_error)?;
    if native_access || owns_wrapper {
        return Err(Error::InvalidConfiguration);
    }
    tx.commit().await.map_err(storage_error)
}

fn quote_identifier(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 63
        || (!value.as_bytes()[0].is_ascii_alphabetic() && value.as_bytes()[0] != b'_')
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(Error::InvalidArguments);
    }
    Ok(format!("\"{value}\""))
}
