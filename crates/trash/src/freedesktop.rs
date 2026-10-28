//! Freedesktop.org Trash specification 1.0:
//! <https://specifications.freedesktop.org/trash-spec/trashspec-1.0.html>

use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use log::debug;
use log::warn;

use crate::Error;
use crate::FsError;
use crate::TrashItem;
use crate::move_no_replace;
use crate::restore_from;

pub(crate) fn delete(path: PathBuf) -> Result<TrashItem, Error> {
    let home_trash = canonicalize_path_or_parents(&home_trash()?)?;
    let mount_points = sorted_mount_points()?;
    let home_trash_topdir = topdir_containing(&home_trash, &mount_points);
    let topdir = topdir_containing(&path, &mount_points);
    debug!("trashing {path:?}: topdir {topdir:?}, home trash topdir {home_trash_topdir:?}");
    if topdir == home_trash_topdir {
        return move_to_trash(&path, &home_trash).map_err(fs_error);
    }

    // The spec allows either using the home trash or refusing when the
    // volume's trash can't be used, e.g. a read-only mount root.
    match volume_trash(topdir).and_then(|trash| move_to_trash(&path, &trash)) {
        Ok(item) => Ok(item),
        Err((_, error)) if error.kind() == ErrorKind::PermissionDenied => {
            debug!("volume trash on {topdir:?} unusable ({error}), falling back to the home trash");
            move_to_trash(&path, &home_trash).map_err(fs_error)
        }
        Err(error) => Err(fs_error(error)),
    }
}

pub(crate) fn restore(item: TrashItem) -> Result<(), Error> {
    let info_file = PathBuf::from(&item.id);
    let file_in_trash = file_in_trash(&info_file).ok_or_else(|| Error::NoLongerInTrash { path: info_file.clone() })?;
    restore_from(&file_in_trash, &item)?;
    fs::remove_file(&info_file).map_err(|error| fs_error((info_file, error)))
}

fn fs_error((path, source): FsError) -> Error {
    Error::FileSystem { path, source }
}

/// `<trash>/info/<name>.trashinfo` describes `<trash>/files/<name>`.
fn file_in_trash(info_file: &Path) -> Option<PathBuf> {
    let trash = info_file.parent()?.parent()?;
    Some(trash.join("files").join(info_file.file_stem()?))
}

/// Picks `$topdir/.Trash/$uid` when an admin-created `.Trash` is valid,
/// otherwise `$topdir/.Trash-$uid`, creating it if needed.
fn volume_trash(topdir: &Path) -> Result<PathBuf, FsError> {
    // SAFETY: `getuid` has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let admin_trash = topdir.join(".Trash");
    if admin_trash.is_dir() {
        if is_valid_admin_trash(&admin_trash)? {
            let user_trash = admin_trash.join(uid.to_string());
            if user_trash.is_dir() {
                return Ok(user_trash);
            }
        } else {
            warn!("ignoring {admin_trash:?}: it must be a sticky, non-symlink directory");
        }
    }

    let user_trash = topdir.join(format!(".Trash-{uid}"));
    if !user_trash.is_dir() {
        fs::create_dir(&user_trash).map_err(|error| (user_trash.clone(), error))?;
    }
    Ok(user_trash)
}

fn is_valid_admin_trash(path: &Path) -> Result<bool, FsError> {
    const STICKY_BIT: u32 = 0o1000;
    let metadata = path.symlink_metadata().map_err(|error| (path.to_owned(), error))?;
    Ok(!metadata.file_type().is_symlink() && metadata.permissions().mode() & STICKY_BIT != 0)
}

fn move_to_trash(src: &Path, trash: &Path) -> Result<TrashItem, FsError> {
    let files_dir = trash.join("files");
    let info_dir = trash.join("info");
    fs::create_dir_all(&files_dir).map_err(|error| (files_dir.clone(), error))?;
    fs::create_dir_all(&info_dir).map_err(|error| (info_dir.clone(), error))?;

    let name = src.file_name().expect("trashed paths are canonical and have a file name");
    let original_parent = src.parent().expect("trashed paths are canonical and have a parent").to_owned();

    // The `.trashinfo` file is created first with `create_new`, reserving the
    // name in the trash, so concurrent trashing never overwrites an item.
    let mut attempt = 0usize;
    loop {
        attempt += 1;
        let mut name_in_trash = name.to_owned();
        if attempt > 1 {
            name_in_trash.push(format!(".{attempt}"));
        }
        let mut info_name = name_in_trash.clone();
        info_name.push(".trashinfo");
        let info_file = info_dir.join(info_name);

        let mut file = match OpenOptions::new().create_new(true).write(true).open(&info_file) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err((info_file, error)),
        };
        let written = write!(file, "[Trash Info]\nPath={}\nDeletionDate={}\n", encode_uri_path(src), deletion_date());
        drop(file);

        let result = written
            .map_err(|error| (info_file.clone(), error))
            .and_then(|()| move_no_replace(src, &files_dir.join(&name_in_trash)));
        match result {
            Ok(()) => {
                return Ok(TrashItem { id: info_file.into_os_string(), name: name.to_owned(), original_parent });
            }
            Err((path, error)) => {
                if let Err(remove_error) = fs::remove_file(&info_file) {
                    warn!("failed to remove {info_file:?} after a failed trash: {remove_error}");
                }
                if error.kind() != ErrorKind::AlreadyExists {
                    return Err((path, error));
                }
            }
        }
    }
}

/// Local time as `YYYY-MM-DDThh:mm:ss`.
fn deletion_date() -> String {
    // SAFETY: `time` accepts a null pointer, and `localtime_r` only writes to
    // the `tm` it is given.
    let tm = unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm = std::mem::zeroed::<libc::tm>();
        libc::localtime_r(&raw const now, &raw mut tm);
        tm
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

/// Percent-encodes each component; paths are bytes and may not be UTF-8.
fn encode_uri_path(path: &Path) -> String {
    let mut encoded = String::new();
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => {
                encoded.push('/');
                encoded.push_str(&urlencoding::encode_binary(part.as_bytes()));
            }
            other => {
                encoded.push('/');
                encoded.push_str(&other.as_os_str().to_string_lossy());
            }
        }
    }
    if encoded.is_empty() { "/".to_owned() } else { encoded }
}

/// `$XDG_DATA_HOME/Trash`, or `~/.local/share/Trash`.
fn home_trash() -> Result<PathBuf, Error> {
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(data_home).join("Trash"));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home).join(".local/share/Trash"));
    }
    Err(Error::Unknown { description: "neither XDG_DATA_HOME nor HOME is set".into() })
}

/// Canonicalizes the longest existing ancestor of `path` and appends the rest,
/// so a home trash that doesn't exist yet still resolves symlinked homes
/// (e.g. `/home` → `/var/home`).
fn canonicalize_path_or_parents(path: &Path) -> Result<PathBuf, Error> {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        match existing.canonicalize() {
            Ok(canonical) => return Ok(missing.iter().rev().fold(canonical, |path, name| path.join(name))),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing.push(existing.file_name().ok_or_else(|| Error::CanonicalizePath { original: path.into() })?);
                existing = existing.parent().ok_or_else(|| Error::CanonicalizePath { original: path.into() })?;
            }
            Err(source) => return Err(Error::FileSystem { path: existing.to_owned(), source }),
        }
    }
}

fn topdir_containing<'a>(path: &Path, sorted_mount_points: &'a [PathBuf]) -> &'a Path {
    sorted_mount_points
        .iter()
        .map(PathBuf::as_path)
        .find(|mount_point| path.starts_with(mount_point))
        .unwrap_or(Path::new("/"))
}

/// Mount points, longest first, so the first prefix match is the innermost.
fn sorted_mount_points() -> Result<Vec<PathBuf>, Error> {
    let mounts = fs::read("/proc/mounts")
        .or_else(|_| fs::read("/etc/mtab"))
        .map_err(|_| Error::Unknown { description: "neither /proc/mounts nor /etc/mtab could be read".into() })?;
    let mut mount_points = parse_mount_points(&mounts);
    if mount_points.is_empty() {
        return Err(Error::Unknown { description: "no mount points found".into() });
    }
    mount_points.sort_unstable_by_key(|mount_point| std::cmp::Reverse(mount_point.as_os_str().len()));
    Ok(mount_points)
}

/// Reads the second field of each fstab-style line, undoing the octal
/// escapes (`\040` for space, etc.).
fn parse_mount_points(mounts: &[u8]) -> Vec<PathBuf> {
    mounts
        .split(|byte| *byte == b'\n')
        .filter_map(|line| line.split(|byte| *byte == b' ').filter(|field| !field.is_empty()).nth(1))
        .map(|field| {
            let mut bytes = Vec::with_capacity(field.len());
            let mut index = 0;
            while index < field.len() {
                let octal = field.get(index + 1..index + 4).filter(|digits| digits.iter().all(u8::is_ascii_digit));
                match (field[index], octal) {
                    (b'\\', Some(digits)) => {
                        let value = digits.iter().fold(0u32, |value, digit| value * 8 + u32::from(digit - b'0'));
                        bytes.push(value as u8);
                        index += 4;
                    }
                    (byte, _) => {
                        bytes.push(byte);
                        index += 1;
                    }
                }
            }
            PathBuf::from(OsString::from_vec(bytes))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;
    use std::path::Path;
    use std::path::PathBuf;

    use super::canonicalize_path_or_parents;
    use super::encode_uri_path;
    use super::file_in_trash;
    use super::parse_mount_points;
    use super::topdir_containing;

    #[test]
    fn encodes_uri_paths() {
        assert_eq!(encode_uri_path(Path::new("/tmp/a b/c%d")), "/tmp/a%20b/c%25d");
        assert_eq!(encode_uri_path(Path::new("/")), "/");
        let invalid_utf8 = PathBuf::from(OsString::from_vec(b"/tmp/\xa8".to_vec()));
        assert_eq!(encode_uri_path(&invalid_utf8), "/tmp/%A8");
    }

    #[test]
    fn parses_mount_points() {
        let mounts = b"/dev/sda1 / ext4 rw 0 0\n/dev/sdb1 /mnt/my\\040disk ext4 rw 0 0\n\n";
        assert_eq!(parse_mount_points(mounts), [PathBuf::from("/"), PathBuf::from("/mnt/my disk")]);
    }

    #[test]
    fn finds_innermost_topdir() {
        let mount_points = [PathBuf::from("/mnt/disk"), PathBuf::from("/")];
        assert_eq!(topdir_containing(Path::new("/mnt/disk/a"), &mount_points), Path::new("/mnt/disk"));
        assert_eq!(topdir_containing(Path::new("/mnt/diskette"), &mount_points), Path::new("/"));
    }

    #[test]
    fn maps_info_file_to_trashed_file() {
        assert_eq!(file_in_trash(Path::new("/t/info/a.tar.gz.trashinfo")), Some(PathBuf::from("/t/files/a.tar.gz")));
    }

    #[test]
    fn canonicalizes_missing_paths_through_existing_parents() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_dir = dir.path().canonicalize().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&canonical_dir, &link).unwrap();
        assert_eq!(canonicalize_path_or_parents(&link.join("a/b")).unwrap(), canonical_dir.join("a/b"));
    }
}
