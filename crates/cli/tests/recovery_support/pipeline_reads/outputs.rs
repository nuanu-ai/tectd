use super::*;

/// Explicit immutable Output read; stored completion digest is not representation SHA.
pub async fn read_pipeline_output(
    client: &mut Mcp,
    run_id: Uuid,
    output_id: Uuid,
    digest: &str,
) -> FixtureResult<ResolvedRead> {
    require(
        !run_id.is_nil() && !output_id.is_nil(),
        "nil output read identity",
    )?;
    require(
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "invalid stored output digest",
    )?;
    let arguments = json!({"route":ROUTE,"params":{"run_id":run_id,"view":"output","output_id":output_id,"digest":digest}});
    bytes::read_output(client, &arguments).await
}
