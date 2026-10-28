//! Moves files and directories to the system trash and restores them.
//!
//! Vendored from <https://github.com/Byron/trash-rs> (MIT, see `LICENSE-MIT`),
//! merged with Zed's fork (<https://github.com/zed-industries/trash-rs>) for
//! returning the trashed item and restoring on macOS, and trimmed to macOS and
//! Linux (freedesktop).

#[cfg(target_os = "linux")]
mod freedesktop;
#[cfg(target_os = "macos")]
mod macos;

use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use freedesktop as platform;
#[cfg(target_os = "macos")]
use macos as platform;

/// An item in the trash, with enough information to restore it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItem {
    /// Platform-specific identifier of the item in the trash.
    ///
    /// * freedesktop: path to the `.trashinfo` file.
    /// * macOS: path to the item inside the trash.
    pub id: OsString,
    /// Name of the item at the time of trashing.
    pub name: OsString,
    /// Absolute path to the parent directory at the time of trashing.
    pub original_parent: PathBuf,
}

impl TrashItem {
    pub fn original_path(&self) -> PathBuf {
        self.original_parent.join(&self.name)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot trash a filesystem root")]
    TargetedRoot,
    #[error("could not canonicalize {}", original.display())]
    CanonicalizePath { original: PathBuf },
    #[error("{} is no longer in the trash", path.display())]
    NoLongerInTrash { path: PathBuf },
    #[error("{} already exists", path.display())]
    RestoreCollision { path: PathBuf },
    #[error("{}: {source}", path.display())]
    FileSystem { path: PathBuf, source: io::Error },
    #[error("{description}")]
    Unknown { description: String },
}

/// Moves `path` to the trash. Symlinks are trashed themselves, not their
/// targets.
pub fn delete_with_info(path: impl AsRef<Path>) -> Result<TrashItem, Error> {
    platform::delete(canonicalize_parent(path.as_ref())?)
}

/// Moves `item` back to its original path, failing with
/// [`Error::RestoreCollision`] if something exists there.
pub fn restore(item: TrashItem) -> Result<(), Error> {
    platform::restore(item)
}

type FsError = (PathBuf, io::Error);

/// Makes `path` absolute and resolves symlinks in its parent only.
fn canonicalize_parent(path: &Path) -> Result<PathBuf, Error> {
    let path = std::path::absolute(path).map_err(|source| Error::FileSystem { path: path.to_owned(), source })?;
    let parent = path.parent().ok_or(Error::TargetedRoot)?;
    let name = path.file_name().ok_or_else(|| Error::CanonicalizePath { original: path.clone() })?;
    let parent = parent.canonicalize().map_err(|_| Error::CanonicalizePath { original: parent.to_owned() })?;
    Ok(parent.join(name))
}

/// Like `try_exists`, but a broken symlink counts as existing.
fn virtually_exists(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(source) => Err(Error::FileSystem { path: path.to_owned(), source }),
    }
}

fn restore_from(file_in_trash: &Path, item: &TrashItem) -> Result<(), Error> {
    if !virtually_exists(file_in_trash)? {
        return Err(Error::NoLongerInTrash { path: file_in_trash.to_owned() });
    }
    let original_path = item.original_path();
    fs::create_dir_all(&item.original_parent)
        .map_err(|source| Error::FileSystem { path: item.original_parent.clone(), source })?;
    move_no_replace(file_in_trash, &original_path).map_err(|(path, source)| {
        if source.kind() == ErrorKind::AlreadyExists && path == original_path {
            Error::RestoreCollision { path }
        } else {
            Error::FileSystem { path, source }
        }
    })
}

/// Moves `src` to `dst` without replacing anything at `dst`.
///
/// `rename` overwrites silently, so `dst` is first claimed with an empty
/// placeholder; an existing `dst` fails with `AlreadyExists` on `dst`. Falls
/// back to copy + remove across filesystems.
fn move_no_replace(src: &Path, dst: &Path) -> Result<(), FsError> {
    let is_dir = fs::symlink_metadata(src).map_err(|error| (src.to_owned(), error))?.is_dir();
    let placeholder =
        if is_dir { fs::create_dir(dst) } else { OpenOptions::new().create_new(true).write(true).open(dst).map(drop) };
    placeholder.map_err(|error| (dst.to_owned(), error))?;

    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::CrossesDevices => {
            log::debug!("cross-device move of {src:?}, falling back to copy + remove");
            copy_recursive(src, dst)?;
            let removed = if is_dir { fs::remove_dir_all(src) } else { fs::remove_file(src) };
            removed.map_err(|error| (src.to_owned(), error))
        }
        Err(error) => {
            let _ = if is_dir { fs::remove_dir(dst) } else { fs::remove_file(dst) };
            Err((src.to_owned(), error))
        }
    }
}

/// Copies `src` onto `dst`, which may be an empty placeholder. Symlinks are
/// copied as links.
fn copy_recursive(src: &Path, dst: &Path) -> Result<(), FsError> {
    let file_type = fs::symlink_metadata(src).map_err(|error| (src.to_owned(), error))?.file_type();
    if file_type.is_dir() {
        fs::create_dir_all(dst).map_err(|error| (dst.to_owned(), error))?;
        for entry in fs::read_dir(src).map_err(|error| (src.to_owned(), error))? {
            let entry = entry.map_err(|error| (src.to_owned(), error))?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else if file_type.is_symlink() {
        let target = fs::read_link(src).map_err(|error| (src.to_owned(), error))?;
        let _ = fs::remove_file(dst);
        std::os::unix::fs::symlink(target, dst).map_err(|error| (dst.to_owned(), error))?;
    } else {
        fs::copy(src, dst).map_err(|error| (src.to_owned(), error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::Error;
    use crate::delete_with_info;
    use crate::move_no_replace;
    use crate::restore;

    #[test]
    fn move_no_replace_refuses_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::write(&src, "new").unwrap();
        fs::write(&dst, "old").unwrap();

        let (path, error) = move_no_replace(&src, &dst).unwrap_err();
        assert_eq!(path, dst);
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&dst).unwrap(), "old");
        assert_eq!(fs::read_to_string(&src).unwrap(), "new");
    }

    #[test]
    fn move_no_replace_moves_directories_and_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("file"), "content").unwrap();
        std::os::unix::fs::symlink("missing", src.join("link")).unwrap();
        let dst = dir.path().join("dst");

        move_no_replace(&src, &dst).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(dst.join("file")).unwrap(), "content");
        assert_eq!(fs::read_link(dst.join("link")).unwrap().to_str(), Some("missing"));
    }

    // Uses the real system trash.
    #[test]
    fn trash_and_restore_roundtrip() {
        let tempdir = tempfile::tempdir().unwrap();
        let dir = tempdir.path().canonicalize().unwrap();
        let file = dir.join("trash-roundtrip.txt");
        fs::write(&file, "content").unwrap();
        let folder = dir.join("trash-roundtrip-dir");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("nested"), "nested").unwrap();
        let link = dir.join("trash-roundtrip-link");
        std::os::unix::fs::symlink(&file, &link).unwrap();

        let file_item = delete_with_info(&file).unwrap();
        let folder_item = delete_with_info(&folder).unwrap();
        let link_item = delete_with_info(&link).unwrap();
        assert!(!file.exists() && !folder.exists() && link.symlink_metadata().is_err());

        restore(link_item).unwrap();
        restore(folder_item).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), file);
        assert_eq!(fs::read_to_string(folder.join("nested")).unwrap(), "nested");

        fs::write(&file, "replacement").unwrap();
        let error = restore(file_item.clone()).unwrap_err();
        assert!(matches!(error, Error::RestoreCollision { ref path } if *path == file), "{error:?}");
        assert_eq!(fs::read_to_string(&file).unwrap(), "replacement");

        fs::remove_file(&file).unwrap();
        restore(file_item.clone()).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "content");
        assert!(matches!(restore(file_item), Err(Error::NoLongerInTrash { .. })));
    }
}
