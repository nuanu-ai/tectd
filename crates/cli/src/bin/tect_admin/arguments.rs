#[derive(Parser)]
#[command(name = "tect-admin")]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Backup {
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        runtime_role: String,
    },
    Restore {
        /// Restore and verify into a sealed database; managed recovery is required before use.
        #[arg(long)]
        staged: bool,
        #[arg(long = "from")]
        from: PathBuf,
        #[arg(long)]
        database: String,
        #[arg(long)]
        runtime_role: String,
    },
    Migrate {
        #[arg(long)]
        runtime_role: String,
        #[arg(long)]
        enable_durable_knowledge: bool,
        #[arg(long)]
        enable_knowledge_vector_search: bool,
    },
    Enroll {
        #[arg(long)]
        tenant: Option<Uuid>,
        #[arg(long = "source-root")]
        source_roots: Vec<PathBuf>,
        #[arg(long = "setup-root")]
        setup_roots: Vec<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    EnrollVerifier {
        #[arg(long)]
        tenant: Uuid,
        #[arg(long)]
        workspace: Uuid,
        #[arg(long)]
        out: PathBuf,
    },
    EnsureTenant {
        #[arg(long)]
        tenant: Uuid,
    },
    RegisterHost {
        #[arg(long)]
        tenant: Uuid,
        #[arg(long)]
        auth_file: PathBuf,
        #[arg(long = "source-root")]
        source_roots: Vec<PathBuf>,
        #[arg(long = "setup-root")]
        setup_roots: Vec<PathBuf>,
    },
    GrantSetupRoot {
        #[arg(long)]
        host_id: String,
        #[arg(long)]
        setup_root: PathBuf,
    },
    RevokeHost {
        #[arg(long)]
        host_id: Uuid,
    },
    RevokeSession {
        #[arg(long)]
        session_id: Uuid,
    },
    KnowledgeSuppressionExport {
        #[arg(long)]
        out: PathBuf,
    },
    KnowledgeSuppressionApply {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        expected_lineage: Uuid,
        #[arg(long)]
        expected_sequence: i64,
        #[arg(long)]
        expected_digest: String,
    },
}
