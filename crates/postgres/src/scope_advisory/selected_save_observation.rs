fn observation_digest(domain: &str, value: &serde_json::Value) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(storage_error)?;
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update([0]);
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

async fn observe_selected_save(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    actor: Uuid,
    request: &SelectedSaveObservationRequest,
) -> Result<SelectedSaveObservation> {
    if !request.valid()
        || !actor_session_exists(tx, tenant, workspace, actor, request.session_id).await?
    {
        return Err(Error::InvalidArguments);
    }
    let fingerprint = observation_digest(
        "tect.selected-save-observation-request/1",
        &serde_json::json!({
            "request_id": request.request_id,
            "opportunity_id": request.opportunity_id,
            "candidate_set_id": request.candidate_set_id,
            "caller_link_id": request.caller_link_id,
            "caller_receipt_request_id": request.caller_receipt_request_id,
            "target_revision": request.target_revision,
            "actor_id": actor,
            "session_id": request.session_id,
        }),
    )?;
    lock_scope_key(
        tx,
        tenant,
        workspace,
        "selected-save-observation",
        request.request_id,
    )
    .await?;
    type Existing = (
        Uuid,
        Uuid,
        Uuid,
        Uuid,
        Uuid,
        i64,
        Uuid,
        Uuid,
        String,
        String,
        Vec<String>,
        String,
        String,
    );
    let existing: Option<Existing> = sqlx::query_as(
        "SELECT observation_id,opportunity_id,candidate_set_id,caller_link_id,caller_receipt_request_id,\
                target_revision,actor_id,session_id,request_fingerprint,status,reason_codes,evidence_digest,qualification \
         FROM advisory_scope_selected_save_observation WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
    )
    .bind(tenant).bind(workspace).bind(request.request_id)
    .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if let Some(row) = existing {
        if row.1 != request.opportunity_id
            || row.2 != request.candidate_set_id
            || row.3 != request.caller_link_id
            || row.4 != request.caller_receipt_request_id
            || row.5 != request.target_revision
            || row.6 != actor
            || row.7 != request.session_id
            || row.8 != fingerprint
        {
            return Err(Error::InputConflict);
        }
        return Ok(SelectedSaveObservation {
            id: row.0,
            request_id: request.request_id,
            opportunity_id: row.1,
            candidate_set_id: row.2,
            caller_link_id: row.3,
            caller_receipt_request_id: row.4,
            target_revision: row.5,
            actor_id: row.6,
            session_id: row.7,
            status: if row.9 == "passed" {
                SelectedSaveObservationStatus::Passed
            } else {
                SelectedSaveObservationStatus::Failed
            },
            reason_codes: row.10,
            evidence_digest: row.11,
            qualification: row.12,
        });
    }

    // The row lock keeps a concurrent candidate write from changing the
    // postcondition between the revision read and the saved material read.
    let current_revision: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR SHARE",
    ).bind(tenant).bind(workspace).bind(request.candidate_set_id)
      .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let caller: Option<(Uuid, Uuid, Uuid, String, Uuid, i64)> = sqlx::query_as(
        "SELECT disposition_id,preservation_receipt_id,link_id,caller_operation,caller_request_id,caller_result_revision \
         FROM advisory_scope_caller_link WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3 \
           AND candidate_set_id=$4 AND link_id=$5",
    ).bind(tenant).bind(workspace).bind(request.opportunity_id)
      .bind(request.candidate_set_id).bind(request.caller_link_id)
      .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let caller_ok = caller.as_ref().is_some_and(|row| {
        row.2 == request.caller_link_id
            && row.3 == "save_draft"
            && row.4 == request.caller_receipt_request_id
            && row.5 == request.target_revision
    });

    let manifest_payload: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT aggregate_payload FROM advisory_scope_manifest WHERE tenant_id=$1 AND workspace_id=$2 \
         AND opportunity_id=$3 AND candidate_set_id=$4",
    ).bind(tenant).bind(workspace).bind(request.opportunity_id).bind(request.candidate_set_id)
      .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let manifest = manifest_payload
        .and_then(|value| serde_json::from_value::<ScopeConstructorManifest>(value).ok())
        .filter(|value| {
            value.validate(&Sha256ScopeDigest).is_ok()
                && value.source.candidate_set_id == request.candidate_set_id
        });
    let advice_row: Option<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT advice_id,aggregate_payload FROM advisory_scope_advice WHERE tenant_id=$1 AND workspace_id=$2 \
         AND opportunity_id=$3 AND candidate_set_id=$4",
    ).bind(tenant).bind(workspace).bind(request.opportunity_id).bind(request.candidate_set_id)
      .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let advice = advice_row.and_then(|(id, value)| {
        serde_json::from_value::<GuardedScopeAdvice>(value)
            .ok()
            .filter(|advice| {
                advice.id.0 == id
                    && manifest.as_ref().is_some_and(|manifest| {
                        validate_guarded_advice_binding(&Sha256ScopeDigest, manifest, advice)
                            .is_ok()
                    })
            })
    });
    let disposition_payload: Option<serde_json::Value> = if let Some(caller) = &caller {
        sqlx::query_scalar("SELECT aggregate_payload FROM advisory_scope_disposition WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3 AND candidate_set_id=$4 AND disposition_id=$5")
            .bind(tenant).bind(workspace).bind(request.opportunity_id).bind(request.candidate_set_id).bind(caller.0)
            .fetch_optional(&mut **tx).await.map_err(storage_error)?
    } else {
        None
    };
    let disposition = disposition_payload
        .and_then(|value| serde_json::from_value::<ScopeDispositionRevision>(value).ok())
        .filter(|value| {
            caller.as_ref().is_some_and(|caller| value.id == caller.0)
                && manifest
                    .as_ref()
                    .zip(advice.as_ref())
                    .is_some_and(|(manifest, advice)| {
                        value.validate(&Sha256ScopeDigest, manifest, advice).is_ok()
                    })
        });
    let selected_material = disposition
        .as_ref()
        .and_then(|value| value.selected_id.as_ref())
        .and_then(|id| manifest.as_ref().and_then(|manifest| manifest.eligible(id)))
        .map(|alternative| &alternative.material);

    let preservation_row: Option<(Uuid, String, serde_json::Value, serde_json::Value)> =
        if let Some(caller) = &caller {
            sqlx::query_as("SELECT disposition_id,status,observation_payload,result_payload FROM advisory_scope_preservation_receipt WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3 AND candidate_set_id=$4 AND receipt_id=$5")
            .bind(tenant).bind(workspace).bind(request.opportunity_id).bind(request.candidate_set_id).bind(caller.1)
            .fetch_optional(&mut **tx).await.map_err(storage_error)?
        } else {
            None
        };
    let preservation_ok =
        preservation_row.is_some_and(|(disposition_id, status, observed, result)| {
            if status != "passed"
                || !caller
                    .as_ref()
                    .is_some_and(|caller| caller.0 == disposition_id)
            {
                return false;
            }
            let Ok(observed) = serde_json::from_value::<FreshScopeObservation>(observed) else {
                return false;
            };
            let Ok(result) = serde_json::from_value::<ScopePreservationResult>(result) else {
                return false;
            };
            manifest
                .as_ref()
                .zip(advice.as_ref())
                .zip(disposition.as_ref())
                .is_some_and(|((manifest, advice), disposition)| {
                    evaluate_scope_preservation(
                        &Sha256ScopeDigest,
                        manifest,
                        advice,
                        disposition,
                        &observed,
                    )
                    .is_ok_and(|expected| {
                        expected == result
                            && matches!(expected.status, ScopePreservationStatus::Passed)
                    })
                })
        });
    let receipt_row: Option<(i64, Option<serde_json::Value>, Option<serde_json::Value>)> =
        sqlx::query_as(
            "SELECT result_revision,request_payload,result_payload FROM scope_candidate_receipts \
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 \
           AND operation='save_draft' AND request_id=$4",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(request.candidate_set_id)
        .bind(request.caller_receipt_request_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?;
    let receipt_ok = receipt_row
        .as_ref()
        .is_some_and(|(revision, input, output)| {
            if *revision != request.target_revision {
                return false;
            }
            let Some(input) = input.clone() else {
                return false;
            };
            let Ok(input) = serde_json::from_value::<SaveCandidateDraft>(input) else {
                return false;
            };
            let Some(output) = output.clone() else {
                return false;
            };
            let Ok(output) = serde_json::from_value::<StoredCandidateContext>(output) else {
                return false;
            };
            input.request_id == request.caller_receipt_request_id
                && input.candidate_set_id == request.candidate_set_id
                && input.selected_advisory.as_ref().is_some_and(|selected| {
                    selected.opportunity_id == request.opportunity_id
                        && caller
                            .as_ref()
                            .is_some_and(|caller| selected.disposition_id == caller.0)
                        && disposition.as_ref().is_some_and(|disposition| {
                            disposition.selected_id.as_ref() == Some(&selected.selected_id)
                        })
                })
                && input.revision.checked_add(1) == Some(request.target_revision)
                && output.context.candidate_set.revision == request.target_revision
                && output.context.candidate_set.id == request.candidate_set_id
                && selected_material.is_some_and(|material| output.draft.as_ref() == Some(material))
        });
    let saved_payload: Option<Option<serde_json::Value>> = sqlx::query_scalar(
        "SELECT payload FROM scope_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 \
         AND candidate_set_id=$3 AND set_revision=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.candidate_set_id)
    .bind(request.target_revision)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let saved_material = saved_payload
        .flatten()
        .and_then(|value| serde_json::from_value::<ResolvedCandidateDraft>(value).ok())
        .filter(|value| value.validate().is_ok());
    let observed_material_digest = saved_material
        .as_ref()
        .and_then(|value| scope_candidate_material_digest(&Sha256ScopeDigest, value).ok());
    let expected_material_digest = disposition
        .as_ref()
        .and_then(|value| value.selected_id.as_ref())
        .and_then(|id| manifest.as_ref().and_then(|manifest| manifest.eligible(id)))
        .map(|alternative| alternative.material_digest.clone());
    let material_ok = saved_material
        .as_ref()
        .zip(selected_material)
        .is_some_and(|(saved, selected)| saved == selected);
    let checks = SelectedSaveChecks {
        manifest: manifest.is_some(),
        advice: advice.is_some(),
        disposition: disposition.is_some() && selected_material.is_some(),
        preservation: preservation_ok,
        caller: caller_ok,
        receipt: receipt_ok,
        material: material_ok,
        revision: current_revision == Some(request.target_revision),
    };
    let (status, reason_codes) = evaluate_selected_save_checks(checks);
    let evidence = serde_json::json!({"schema":"tect.selected-save-observation/1", "checks":checks,
        "target_revision":request.target_revision, "current_revision":current_revision,
        "observed_material_digest":observed_material_digest,
        "expected_material_digest":expected_material_digest,
        "caller_link_id":request.caller_link_id, "caller_receipt_request_id":request.caller_receipt_request_id});
    let evidence_digest =
        observation_digest("tect.selected-save-observation-evidence/1", &evidence)?;
    let id = Uuid::new_v4();
    let status_name = match status {
        SelectedSaveObservationStatus::Passed => "passed",
        SelectedSaveObservationStatus::Failed => "failed",
    };
    sqlx::query("INSERT INTO advisory_scope_selected_save_observation \
        (tenant_id,workspace_id,observation_id,request_id,opportunity_id,candidate_set_id,caller_link_id,caller_receipt_request_id,target_revision,actor_id,session_id,request_fingerprint,status,reason_codes,evidence_digest,evidence_payload) \
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
        .bind(tenant).bind(workspace).bind(id).bind(request.request_id)
        .bind(request.opportunity_id).bind(request.candidate_set_id).bind(request.caller_link_id)
        .bind(request.caller_receipt_request_id).bind(request.target_revision).bind(actor)
        .bind(request.session_id).bind(fingerprint).bind(status_name).bind(&reason_codes)
        .bind(&evidence_digest).bind(evidence).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(SelectedSaveObservation {
        id,
        request_id: request.request_id,
        opportunity_id: request.opportunity_id,
        candidate_set_id: request.candidate_set_id,
        caller_link_id: request.caller_link_id,
        caller_receipt_request_id: request.caller_receipt_request_id,
        target_revision: request.target_revision,
        actor_id: actor,
        session_id: request.session_id,
        status,
        reason_codes,
        evidence_digest,
        qualification: "unresolved".into(),
    })
}
