async fn register_source(
    &mut self,
    workspace_id: Uuid,
    host_id: Uuid,
    location: &SourceLocation,
) -> Result<RegisteredSource> {
    let tenant_id = self.tenant_id()?;
    sources::register_source(
        self.transaction()?,
        tenant_id,
        workspace_id,
        host_id,
        location,
    )
    .await
}

async fn source_worktrees(
    &mut self,
    workspace_id: Uuid,
    host_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<WorktreeSummary>> {
    let tenant_id = self.tenant_id()?;
    sources::source_worktrees(self.transaction()?, tenant_id, workspace_id, host_id, ids).await
}

async fn replace_selection(
    &mut self,
    workspace_id: Uuid,
    host_id: Uuid,
    session_id: Uuid,
    ids: &[Uuid],
) -> Result<()> {
    let tenant_id = self.tenant_id()?;
    sources::replace_selection(
        self.transaction()?,
        tenant_id,
        workspace_id,
        host_id,
        session_id,
        ids,
    )
    .await
}

async fn selected_worktrees(
    &mut self,
    workspace_id: Uuid,
    host_id: Uuid,
    session_id: Uuid,
) -> Result<Vec<WorktreeSummary>> {
    let tenant_id = self.tenant_id()?;
    sources::selected_worktrees(
        self.transaction()?,
        tenant_id,
        workspace_id,
        host_id,
        session_id,
    )
    .await
}

async fn list_sources(
    &mut self,
    workspace_id: Uuid,
    host_id: Uuid,
    after: Option<Uuid>,
    limit: u32,
) -> Result<Vec<RegisteredSource>> {
    let tenant_id = self.tenant_id()?;
    sources::list_sources(
        self.transaction()?,
        tenant_id,
        workspace_id,
        host_id,
        after,
        limit,
    )
    .await
}

async fn ensure_program(
    &mut self,
    workspace_id: Uuid,
    session_id: Uuid,
    input: &NewProgramInput,
) -> Result<Program> {
    let tenant_id = self.tenant_id()?;
    programs::ensure_program(
        self.transaction()?,
        tenant_id,
        workspace_id,
        session_id,
        input,
    )
    .await
}

async fn program(
    &mut self,
    workspace_id: Uuid,
    program_id: Uuid,
    for_update: bool,
) -> Result<Option<Program>> {
    let tenant_id = self.tenant_id()?;
    programs::program(
        self.transaction()?,
        tenant_id,
        workspace_id,
        program_id,
        for_update,
    )
    .await
}

async fn program_input(
    &mut self,
    workspace_id: Uuid,
    program_id: Uuid,
    request_id: Uuid,
) -> Result<Option<ProgramInput>> {
    let tenant_id = self.tenant_id()?;
    programs::program_input(
        self.transaction()?,
        tenant_id,
        workspace_id,
        program_id,
        request_id,
    )
    .await
}

async fn insert_program_input(
    &mut self,
    workspace_id: Uuid,
    program_id: Uuid,
    session_id: Uuid,
    sequence: i64,
    input: &NewProgramInput,
) -> Result<ProgramInput> {
    let tenant_id = self.tenant_id()?;
    programs::insert_program_input(
        self.transaction()?,
        tenant_id,
        workspace_id,
        program_id,
        session_id,
        sequence,
        input,
    )
    .await
}

async fn update_program(&mut self, program: &Program) -> Result<()> {
    let tenant_id = self.tenant_id()?;
    programs::update_program(self.transaction()?, tenant_id, program).await
}

async fn program_inputs(
    &mut self,
    workspace_id: Uuid,
    program_id: Uuid,
    after: i64,
    limit: u32,
) -> Result<Vec<ProgramInput>> {
    let tenant_id = self.tenant_id()?;
    programs::program_inputs(
        self.transaction()?,
        tenant_id,
        workspace_id,
        program_id,
        after,
        limit,
    )
    .await
}

async fn list_programs(
    &mut self,
    workspace_id: Uuid,
    after: Option<ProgramCursor>,
    limit: u32,
) -> Result<Vec<ProgramSummary>> {
    let tenant_id = self.tenant_id()?;
    programs::list_programs(self.transaction()?, tenant_id, workspace_id, after, limit).await
}
