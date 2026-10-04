use super::*;
use sha2::{Digest, Sha256};

type EvidenceArtifactRow = (Uuid, String, i64, String, String, String, i64, String);

fn artifact(value: EvidenceArtifactRow) -> Result<PipelineEvidenceArtifact> {
    Ok(PipelineEvidenceArtifact {
        artifact_id: value.0,
        digest: value.1,
        size: value.2,
        format: value.3,
        provenance: value.4,
        target: value.5,
        revision: value.6,
        readiness: serde_json::from_value(serde_json::Value::String(value.7))
            .map_err(storage_error)?,
    })
}

pub(crate) async fn register(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &RegisterPipelineEvidenceArtifact,
) -> Result<PipelineEvidenceArtifactOutcome> {
    let existing: Option<EvidenceArtifactRow> = sqlx::query_as("SELECT artifact_id,digest,size,format,provenance,target,revision,readiness FROM pipeline_evidence_artifacts WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3")
        .bind(tenant).bind(workspace).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if let Some(row) = existing {
        if row.1 != request.digest
            || row.2 != request.size
            || row.3 != request.format
            || row.4 != request.provenance
            || row.5 != request.target
        {
            return Err(Error::InputConflict);
        }
        return Ok(PipelineEvidenceArtifactOutcome {
            artifact: artifact(row)?,
            replay: true,
        });
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO pipeline_evidence_artifacts(tenant_id,workspace_id,artifact_id,revision,digest,size,format,provenance,target,readiness,request_id) VALUES($1,$2,$3,1,$4,$5,$6,$7,$8,'uploading',$9)")
        .bind(tenant).bind(workspace).bind(id).bind(&request.digest).bind(request.size).bind(&request.format).bind(&request.provenance).bind(&request.target).bind(request.request_id).execute(&mut **tx).await.map_err(storage_error)?;
    let _ = session;
    Ok(PipelineEvidenceArtifactOutcome {
        artifact: PipelineEvidenceArtifact {
            artifact_id: id,
            digest: request.digest.clone(),
            size: request.size,
            format: request.format.clone(),
            provenance: request.provenance.clone(),
            target: request.target.clone(),
            revision: 1,
            readiness: PipelineEvidenceArtifactReadiness::Uploading,
        },
        replay: false,
    })
}

pub(crate) async fn finalize(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &FinalizePipelineEvidenceArtifact,
) -> Result<PipelineEvidenceArtifactOutcome> {
    let row: (String,i64,String,String,String,String,String) = sqlx::query_as("SELECT digest,size,format,provenance,target,readiness,body FROM pipeline_evidence_artifacts WHERE tenant_id=$1 AND workspace_id=$2 AND artifact_id=$3 AND revision=$4 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.artifact_id).bind(request.revision).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    let digest = format!("{:x}", Sha256::digest(request.body.as_bytes()));
    if row.5 != "uploading" {
        if row.6 != request.body || digest != row.0 {
            return Err(Error::InputConflict);
        }
        return Ok(PipelineEvidenceArtifactOutcome {
            artifact: artifact((
                request.artifact_id,
                row.0,
                row.1,
                row.2,
                row.3,
                row.4,
                request.revision,
                row.5,
            ))?,
            replay: true,
        });
    }
    let valid = request.body.len() as i64 == row.1 && digest.eq_ignore_ascii_case(&row.0);
    let readiness = if valid { "ready" } else { "rejected" };
    sqlx::query("UPDATE pipeline_evidence_artifacts SET body=$5,readiness=$6,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND artifact_id=$3 AND revision=$4")
        .bind(tenant).bind(workspace).bind(request.artifact_id).bind(request.revision).bind(&request.body).bind(readiness).execute(&mut **tx).await.map_err(storage_error)?;
    let _ = session;
    Ok(PipelineEvidenceArtifactOutcome {
        artifact: artifact((
            request.artifact_id,
            row.0,
            row.1,
            row.2,
            row.3,
            row.4,
            request.revision,
            readiness.into(),
        ))?,
        replay: false,
    })
}

pub(crate) async fn read(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    _principal: Uuid,
    request: &ReadPipelineEvidenceArtifact,
) -> Result<PipelineEvidenceArtifactPage> {
    let row: (String,String,i64,String,String,String,i64,String,String) = sqlx::query_as("SELECT digest,format,size,provenance,target,readiness,revision,body,body FROM pipeline_evidence_artifacts WHERE tenant_id=$1 AND workspace_id=$2 AND artifact_id=$3 AND revision=$4")
        .bind(tenant).bind(workspace).bind(request.artifact_id).bind(request.revision).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    let (start, end) = page_bounds(&row.7, request.offset, request.limit)?;
    let complete = end == row.7.len();
    Ok(PipelineEvidenceArtifactPage {
        artifact: artifact((
            request.artifact_id,
            row.0,
            row.2,
            row.1,
            row.3,
            row.4,
            row.6,
            row.5,
        ))?,
        offset: request.offset,
        limit: request.limit,
        fragment: row.7[start..end].to_owned(),
        complete,
        next_offset: (!complete).then_some(end as u32),
    })
}

fn page_bounds(body: &str, offset: u32, limit: u32) -> Result<(usize, usize)> {
    let start = offset as usize;
    if limit == 0 || start > body.len() || !body.is_char_boundary(start) {
        return Err(Error::InvalidArguments);
    }
    let mut end = start.saturating_add(limit as usize).min(body.len());
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    if end == start && start < body.len() {
        return Err(Error::InvalidArguments);
    }
    Ok((start, end))
}

#[cfg(test)]
mod paging_tests {
    use super::*;

    #[test]
    fn evidence_paging_handles_unicode_boundaries_and_byte_budgets() {
        let body = "aé🙂z";
        assert_eq!(page_bounds(body, 0, 2), Ok((0, 1)));
        assert_eq!(page_bounds(body, 1, 2), Ok((1, 3)));
        assert_eq!(page_bounds(body, 3, 4), Ok((3, 7)));
        assert_eq!(page_bounds(body, 7, 1), Ok((7, 8)));
        for (offset, limit) in [
            (2, 4),
            (4, 4),
            (1, 1),
            (3, 3),
            (9, 1),
            (u32::MAX, 1),
            (0, 0),
        ] {
            assert_eq!(
                page_bounds(body, offset, limit),
                Err(Error::InvalidArguments)
            );
        }
        assert_eq!(page_bounds(body, 8, 1), Ok((8, 8)));
        assert_eq!(page_bounds("", 0, 1), Ok((0, 0)));
        assert_eq!(page_bounds("ascii", 1, 2), Ok((1, 3)));
        assert_eq!(page_bounds(body, 0, u32::MAX), Ok((0, 8)));
    }

    #[test]
    fn evidence_paging_reconstructs_unicode_with_bounded_advancing_cursors() {
        let body = "é🙂aé🙂z";
        let mut offset = 0;
        let mut reconstructed = String::new();
        loop {
            let (start, end) = page_bounds(body, offset, 4).unwrap();
            assert!(end - start <= 4);
            reconstructed.push_str(&body[start..end]);
            if end == body.len() {
                break;
            }
            assert!(end > start);
            offset = end as u32;
        }
        assert_eq!(reconstructed, body);
    }
}
