//! Descriptor-relative, no-clobber publication for initial workspace instructions.

mod unix;

use tect_application::SetupFiles;
use tect_domain::{FileObservation, FilePublication, Result, SetupDirectory};

/// Host-local filesystem implementation. Authorization remains in the application layer;
/// `resolve_directory` receives the freshly authenticated host grants for each operation.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalSetupFiles;

impl SetupFiles for LocalSetupFiles {
    fn resolve_directory(&self, path: &str, current_roots: &[String]) -> Result<SetupDirectory> {
        unix::resolve_directory(path, current_roots)
    }

    fn inspect(&self, directory: &SetupDirectory, max_bytes: usize) -> Result<FileObservation> {
        unix::inspect(directory, max_bytes)
    }

    fn publish(&self, directory: &SetupDirectory, content: &str) -> Result<FilePublication> {
        unix::publish(directory, content)
    }
}

#[cfg(test)]
mod tests;
