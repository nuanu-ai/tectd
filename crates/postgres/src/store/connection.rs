use super::*;

impl PgStore {
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self> {
        if max_connections == 0 {
            return Err(Error::InvalidConfiguration);
        }
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(url)
            .await
            .map_err(storage_error)?;
        if let Err(error) = runtime::verify_runtime_role(&pool).await {
            pool.close().await;
            return Err(error);
        }
        Ok(Self { pool })
    }

    /// Test convenience for pools whose runtime-role contract is established by the fixture.
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl PgUnitOfWork {
    pub(crate) fn principal_role(&self) -> Result<PrincipalRole> {
        self.identity
            .as_ref()
            .map(|identity| identity.role)
            .ok_or(Error::Forbidden)
    }
}
