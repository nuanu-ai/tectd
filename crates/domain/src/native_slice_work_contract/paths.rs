//! Lexical declaration path boundaries; filesystem freshness remains an executor obligation.
use super::NativeSourcePath;

pub(super) fn normalized_path(s: &str, root: bool) -> bool {
    root && s == "."
        || !s.is_empty()
            && !s.starts_with('/')
            && !s.contains(['\\', '\0', ':'])
            && s.split('/').all(|p| !matches!(p, "" | "." | ".."))
}
pub(super) fn path(s: &str, root: bool) -> bool {
    normalized_path(s, root)
        && !s.split('/').any(|p| matches!(p, ".git" | ".tect"))
        && !beneath(s, "tect/workspace")
}
pub(super) fn beneath(path: &str, root: &str) -> bool {
    root == "." || path == root || path.strip_prefix(root).is_some_and(|s| s.starts_with('/'))
}
pub(super) fn contains(path: &NativeSourcePath, root: &NativeSourcePath) -> bool {
    path.source_id == root.source_id && beneath(&path.path, &root.path)
}
