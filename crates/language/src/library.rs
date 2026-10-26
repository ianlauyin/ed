use std::path::Path;
use std::path::PathBuf;

/// The root directory of an external library that a file belongs to, such as a
/// crate in the cargo registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryRoot {
    pub path: PathBuf,
    pub name: String,
}

/// Recognizes a language's external library sources (dependencies, standard
/// library) from a file path, so they open as read-only library worktrees.
pub trait LibraryProvider: Send + Sync {
    fn library_root(&self, abs_path: &Path) -> Option<LibraryRoot>;
}
