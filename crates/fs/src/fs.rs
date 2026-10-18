pub mod fs_watcher;
mod git_clone_progress;

use parking_lot::Mutex;
use slotmap::{KeyData, SlotMap};
use std::ffi::OsString;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::time::Instant;
use util::maybe;

use anyhow::{Context as _, Result};
use futures::stream::iter;
use gpui::App;
use gpui::BackgroundExecutor;
use gpui::Global;
use gpui::ReadGlobal as _;
use gpui::SharedString;

use std::ffi::CString;
use util::command::{Stdio, new_command};

use std::os::fd::{AsFd, AsRawFd};

use std::os::unix::ffi::OsStrExt;

use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

#[cfg(target_os = "macos")]
use std::mem::MaybeUninit;

use futures::{AsyncRead, Stream, StreamExt, future::BoxFuture};
use git::repository::{GitRepository, RealGitRepository};

use rope::Rope;
use serde::{Deserialize, Serialize};
use smol::io::AsyncWriteExt;
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;
use text::LineEnding;

pub trait Watcher: Send + Sync {
    fn add(&self, path: &Path) -> Result<()>;
    fn remove(&self, path: &Path) -> Result<()>;
}

pub trait ReadSeek: io::Read + io::Seek {}

impl<T: io::Read + io::Seek + ?Sized> ReadSeek for T {}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum PathEventKind {
    Removed,
    Created,
    Changed,
    Rescan,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct PathEvent {
    pub path: PathBuf,
    pub kind: Option<PathEventKind>,
}

impl From<PathEvent> for PathBuf {
    fn from(event: PathEvent) -> Self {
        event.path
    }
}

#[async_trait::async_trait]
pub trait Fs: Send + Sync {
    async fn create_dir(&self, path: &Path) -> Result<()>;
    async fn create_symlink(&self, path: &Path, target: PathBuf) -> Result<()>;
    async fn create_file(&self, path: &Path, options: CreateOptions) -> Result<()>;
    async fn create_file_with(
        &self,
        path: &Path,
        content: Pin<&mut (dyn AsyncRead + Send)>,
    ) -> Result<()>;
    async fn copy_file(&self, source: &Path, target: &Path, options: CopyOptions) -> Result<()>;
    async fn rename(&self, source: &Path, target: &Path, options: RenameOptions) -> Result<()>;

    /// Removes a directory from the filesystem.
    /// There is no expectation that the directory will be preserved in the
    /// system trash.
    async fn remove_dir(&self, path: &Path, options: RemoveOptions) -> Result<()>;

    /// Moves a file or directory to the system trash.
    /// Returns a [`TrashedEntry`] that can be used to keep track of the
    /// location of the trashed item in the system's trash.
    async fn trash(&self, path: &Path, options: RemoveOptions) -> Result<TrashId>;

    /// Removes a file from the filesystem.
    /// There is no expectation that the file will be preserved in the system
    /// trash.
    async fn remove_file(&self, path: &Path, options: RemoveOptions) -> Result<()>;

    async fn open_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>>;
    async fn open_sync(&self, path: &Path) -> Result<Box<dyn ReadSeek + Send + Sync>>;
    async fn load(&self, path: &Path) -> Result<String> {
        Ok(String::from_utf8(self.load_bytes(path).await?)?)
    }
    async fn load_bytes(&self, path: &Path) -> Result<Vec<u8>>;
    async fn atomic_write(&self, path: PathBuf, text: String) -> Result<()>;
    async fn save(&self, path: &Path, text: &Rope, line_ending: LineEnding) -> Result<()>;
    async fn write(&self, path: &Path, content: &[u8]) -> Result<()>;
    async fn canonicalize(&self, path: &Path) -> Result<PathBuf>;
    async fn is_file(&self, path: &Path) -> bool;
    async fn is_dir(&self, path: &Path) -> bool;
    async fn metadata(&self, path: &Path) -> Result<Option<Metadata>>;
    async fn read_link(&self, path: &Path) -> Result<PathBuf>;
    async fn read_dir(
        &self,
        path: &Path,
    ) -> Result<Pin<Box<dyn Send + Stream<Item = Result<PathBuf>>>>>;

    /// Creates the native file watcher now rather than on the first `watch`, so a
    /// failure to start it (e.g. inotify instance limits) can be reported at startup.
    fn start_native_watcher(&self) -> Result<()> {
        Ok(())
    }

    /// Whether `path` exists, without following a final symlink. Synchronous
    /// because watches are registered synchronously by the worktree scanner.
    fn path_exists(&self, path: &Path) -> bool;
    /// Whether the volume holding `path` compares file names case-sensitively.
    fn is_path_case_sensitive(&self, path: &Path) -> bool;
    /// Whether `path` sits on a filesystem where native file watching does not
    /// deliver events (network mounts and some FUSE mounts), so it must be polled.
    fn requires_poll_watcher(&self, path: &Path) -> bool;

    async fn watch(
        &self,
        path: &Path,
        latency: Duration,
    ) -> (
        Pin<Box<dyn Send + Stream<Item = Vec<PathEvent>>>>,
        Arc<dyn Watcher>,
    );

    fn open_repo(
        &self,
        abs_dot_git: &Path,
        git_binary_path: Option<&Path>,
    ) -> Result<Arc<dyn GitRepository>>;
    async fn git_init(&self, abs_work_directory: &Path, fallback_branch_name: String)
    -> Result<()>;
    async fn git_clone(&self, abs_work_directory: &Path, repo_url: &str) -> Result<()>;
    async fn git_config(&self, abs_work_directory: &Path, args: Vec<String>) -> Result<String>;
    fn is_fake(&self) -> bool;
    async fn is_case_sensitive(&self) -> bool;
    fn subscribe_to_jobs(&self) -> JobEventReceiver;

    /// Returns the original absolute path of the item identified by `trash_id`.
    fn original_path_for_trash_id(&self, trash_id: TrashId) -> Option<PathBuf>;

    /// Restores the item identified by `trash_id`, moving it from the system's
    /// trash back to its original path.
    async fn restore(&self, trash_id: TrashId) -> std::result::Result<PathBuf, TrashRestoreError>;
}

// We use our own type rather than `trash::TrashItem` directly to avoid carrying
// over fields we don't need (e.g. `time_deleted`) and to insulate callers and
// tests from changes to that crate's API surface.
/// Represents a file or directory that has been moved to the system trash,
/// retaining enough information to restore it to its original location.
#[derive(Clone, PartialEq, Debug)]
struct TrashedEntry {
    /// Platform-specific identifier for the file/directory in the trash.
    ///
    /// * Freedesktop – Path to the `.trashinfo` file.
    /// * macOS & Windows – Full path to the file/directory in the system's
    /// trash.
    pub id: OsString,
    /// Name of the file/directory at the time of trashing, including extension.
    pub name: OsString,
    /// Absolute path to the parent directory at the time of trashing.
    pub original_parent: PathBuf,
}

impl From<trash::TrashItem> for TrashedEntry {
    fn from(item: trash::TrashItem) -> Self {
        Self {
            id: item.id,
            name: item.name,
            original_parent: item.original_parent,
        }
    }
}

impl TrashedEntry {
    fn into_trash_item(self) -> trash::TrashItem {
        trash::TrashItem {
            id: self.id,
            name: self.name,
            original_parent: self.original_parent,
            // `TrashedEntry` doesn't preserve `time_deleted` as we don't
            // currently need it for restore, so we default it to 0 here.
            time_deleted: 0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TrashRestoreError {
    #[error("The specified `path` ({}) was not found in the system's trash.", path.display())]
    NotFound { path: PathBuf },
    #[error("File or directory ({}) already exists at the restore destination.", path.display())]
    Collision { path: PathBuf },
    // This should never occur, the only way to get a TrashId is to undo
    // consumes the Change::Trashed. We worry about remoting duplicate messages
    // we do not want to crash the app then which is why this error is there.
    #[error("The item was already restored")]
    AlreadyRestored,
    #[error("Unknown error ({description})")]
    Unknown { description: String },
}

impl From<trash::Error> for TrashRestoreError {
    fn from(err: trash::Error) -> Self {
        match err {
            trash::Error::RestoreCollision { path, .. } => Self::Collision { path },
            trash::Error::Unknown { description } => Self::Unknown { description },
            other => Self::Unknown {
                description: other.to_string(),
            },
        }
    }
}

struct GlobalFs(Arc<dyn Fs>);

impl Global for GlobalFs {}

impl dyn Fs {
    /// Returns the global [`Fs`].
    pub fn global(cx: &App) -> Arc<Self> {
        GlobalFs::global(cx).0.clone()
    }

    /// Sets the global [`Fs`].
    pub fn set_global(fs: Arc<Self>, cx: &mut App) {
        cx.set_global(GlobalFs(fs));
    }
}

#[derive(Copy, Clone, Default)]
pub struct CreateOptions {
    pub overwrite: bool,
    pub ignore_if_exists: bool,
}

#[derive(Copy, Clone, Default)]
pub struct CopyOptions {
    pub overwrite: bool,
    pub ignore_if_exists: bool,
}

#[derive(Copy, Clone, Default)]
pub struct RenameOptions {
    pub overwrite: bool,
    pub ignore_if_exists: bool,
    /// Whether to create parent directories if they do not exist.
    pub create_parents: bool,
}

#[derive(Copy, Clone, Default)]
pub struct RemoveOptions {
    pub recursive: bool,
    pub ignore_if_not_exists: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct Metadata {
    pub inode: u64,
    pub mtime: MTime,
    pub is_symlink: bool,
    pub is_dir: bool,
    pub len: u64,
    pub is_fifo: bool,
    pub is_executable: bool,
    pub is_writable: bool,
}

/// Filesystem modification time. The purpose of this newtype is to discourage use of operations
/// that do not make sense for mtimes. In particular, it is not always valid to compare mtimes using
/// `<` or `>`, as there are many things that can cause the mtime of a file to be earlier than it
/// was. See ["mtime comparison considered harmful" - apenwarr](https://apenwarr.ca/log/20181113).
///
/// Do not derive Ord, PartialOrd, or arithmetic operation traits.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct MTime(SystemTime);

pub type JobId = usize;

#[derive(Clone, Debug)]
pub struct JobInfo {
    pub start: Instant,
    pub message: SharedString,
    pub id: JobId,
}

#[derive(Debug, Clone)]
pub enum JobEvent {
    Started { info: JobInfo },
    Updated { id: JobId, message: SharedString },
    Completed { id: JobId },
}

pub type JobEventSender = futures::channel::mpsc::UnboundedSender<JobEvent>;
pub type JobEventReceiver = futures::channel::mpsc::UnboundedReceiver<JobEvent>;

struct JobTracker {
    id: JobId,
    subscribers: Arc<Mutex<Vec<JobEventSender>>>,
}

impl JobTracker {
    fn new(info: JobInfo, subscribers: Arc<Mutex<Vec<JobEventSender>>>) -> Self {
        let id = info.id;
        {
            let mut subs = subscribers.lock();
            subs.retain(|sender| {
                sender
                    .unbounded_send(JobEvent::Started { info: info.clone() })
                    .is_ok()
            });
        }
        Self { id, subscribers }
    }

    fn update(&self, message: SharedString) {
        let mut subscribers = self.subscribers.lock();
        subscribers.retain(|sender| {
            sender
                .unbounded_send(JobEvent::Updated {
                    id: self.id,
                    message: message.clone(),
                })
                .is_ok()
        });
    }
}

impl Drop for JobTracker {
    fn drop(&mut self) {
        let mut subs = self.subscribers.lock();
        subs.retain(|sender| {
            sender
                .unbounded_send(JobEvent::Completed { id: self.id })
                .is_ok()
        });
    }
}

impl MTime {
    /// Conversion intended for persistence and testing.
    pub fn from_seconds_and_nanos(secs: u64, nanos: u32) -> Self {
        MTime(UNIX_EPOCH + Duration::new(secs, nanos))
    }

    /// Conversion intended for persistence.
    pub fn to_seconds_and_nanos_for_persistence(self) -> Option<(u64, u32)> {
        self.0
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| (duration.as_secs(), duration.subsec_nanos()))
    }

    /// Returns the value wrapped by this `MTime`, for presentation to the user. The name including
    /// "_for_user" is to discourage misuse - this method should not be used when making decisions
    /// about file dirtiness.
    pub fn timestamp_for_user(self) -> SystemTime {
        self.0
    }

    /// Temporary method to split out the behavior changes from introduction of this newtype.
    pub fn bad_is_greater_than(self, other: MTime) -> bool {
        self.0 > other.0
    }
}

impl From<proto::Timestamp> for MTime {
    fn from(timestamp: proto::Timestamp) -> Self {
        MTime(timestamp.into())
    }
}

impl From<MTime> for proto::Timestamp {
    fn from(mtime: MTime) -> Self {
        mtime.0.into()
    }
}

slotmap::new_key_type! { pub struct TrashId; }

impl TrashId {
    pub fn from_proto(value: u64) -> Self {
        KeyData::from_ffi(value).into()
    }

    pub fn to_proto(self) -> u64 {
        self.0.as_ffi()
    }
}

pub struct RealFs {
    this: std::sync::Weak<Self>,
    executor: BackgroundExecutor,
    native_watcher: Arc<fs_watcher::OsWatcher>,
    poll_watcher: Arc<fs_watcher::OsWatcher>,
    next_job_id: Arc<AtomicUsize>,
    job_event_subscribers: Arc<Mutex<Vec<JobEventSender>>>,
    trash: Arc<Mutex<SlotMap<TrashId, TrashedEntry>>>,
    is_case_sensitive: AtomicU8,
}

pub trait FileHandle: Send + Sync + std::fmt::Debug {
    fn current_path(&self, fs: &Arc<dyn Fs>) -> Result<PathBuf>;
}

impl FileHandle for std::fs::File {
    #[cfg(target_os = "linux")]
    fn current_path(&self, _: &Arc<dyn Fs>) -> Result<PathBuf> {
        let fd = self.as_fd();
        let fd_path = format!("/proc/self/fd/{}", fd.as_raw_fd());
        let new_path = std::fs::read_link(fd_path)?;
        if new_path
            .file_name()
            .is_some_and(|f| f.to_string_lossy().ends_with(" (deleted)"))
        {
            anyhow::bail!("file was deleted")
        };

        Ok(new_path)
    }

    #[cfg(target_os = "macos")]
    fn current_path(&self, _: &Arc<dyn Fs>) -> Result<PathBuf> {
        use std::{
            ffi::{CStr, OsStr},
            os::unix::ffi::OsStrExt,
        };

        let fd = self.as_fd();
        let mut path_buf = MaybeUninit::<[u8; libc::PATH_MAX as usize]>::uninit();

        let result = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETPATH, path_buf.as_mut_ptr()) };
        anyhow::ensure!(result != -1, "fcntl returned -1");

        // SAFETY: `fcntl` will initialize the path buffer.
        let c_str = unsafe { CStr::from_ptr(path_buf.as_ptr().cast()) };
        anyhow::ensure!(!c_str.is_empty(), "Could find a path for the file handle");
        let path = PathBuf::from(OsStr::from_bytes(c_str.to_bytes()));
        Ok(path)
    }
}

pub struct RealWatcher {}

impl RealFs {
    pub fn new(executor: BackgroundExecutor) -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            this: this.clone(),
            native_watcher: fs_watcher::OsWatcher::new(
                fs_watcher::OsWatcherKind::Native,
                executor.clone(),
            ),
            poll_watcher: fs_watcher::OsWatcher::new(
                fs_watcher::OsWatcherKind::Poll,
                executor.clone(),
            ),
            executor,
            next_job_id: Arc::new(AtomicUsize::new(0)),
            job_event_subscribers: Arc::new(Mutex::new(Vec::new())),
            trash: Arc::new(Mutex::new(SlotMap::with_key())),
            is_case_sensitive: Default::default(),
        })
    }
}

fn rename_without_replace(source: &Path, target: &Path) -> io::Result<()> {
    let source = path_to_c_string(source)?;
    let target = path_to_c_string(target)?;

    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(source.as_ptr(), target.as_ptr(), libc::RENAME_EXCL) };

    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };

    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn path_to_c_string(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path contains interior NUL: {}", path.display()),
        )
    })
}

// On Unix targets, std::fs::ReadDir panics in its Drop implementation
// when an unexpected error is returned from closedir(2). We hit this
// condition in production; one cause seems to be macOS's FSEventStream
// incorrectly closing fds it doesn't own, resulting in closedir returning
// EBADF, see https://github.com/zed-industries/zed/issues/59952#issuecomment-5080178879.
//
// We also see occasional errors like ENXIO and ETIMEDOUT that seem to
// come from network or other exotic filesystems.
//
// To avoid crashing the app in this situation, we use the rustix analogue of
// ReadDir, which doesn't have this panic in drop.

fn read_dir_entries(path: PathBuf) -> Result<impl Send + Iterator<Item = Result<PathBuf>>> {
    use rustix::fs::{Dir, Mode, OFlags};
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let directory_fd = rustix::fs::open(
        &path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .with_context(|| format!("failed to open directory {path:?}"))?;
    let directory =
        Dir::new(directory_fd).with_context(|| format!("failed to read directory {path:?}"))?;

    Ok(directory.filter_map(move |entry| {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                return Some(Err(anyhow::Error::new(error)
                    .context(format!("failed to read directory entry in {path:?}"))));
            }
        };
        let name = entry.file_name().to_bytes();
        if name == b"." || name == b".." {
            return None;
        }
        Some(Ok(path.join(OsStr::from_bytes(name))))
    }))
}

#[async_trait::async_trait]
impl Fs for RealFs {
    async fn create_dir(&self, path: &Path) -> Result<()> {
        Ok(smol::fs::create_dir_all(path).await?)
    }

    async fn create_symlink(&self, path: &Path, target: PathBuf) -> Result<()> {
        smol::fs::unix::symlink(target, path).await?;

        Ok(())
    }

    async fn create_file(&self, path: &Path, options: CreateOptions) -> Result<()> {
        let mut open_options = smol::fs::OpenOptions::new();
        open_options.write(true).create(true);
        if options.overwrite {
            open_options.truncate(true);
        } else if !options.ignore_if_exists {
            open_options.create_new(true);
        }
        open_options
            .open(path)
            .await
            .with_context(|| format!("Failed to create file at {:?}", path))?;
        Ok(())
    }

    async fn create_file_with(
        &self,
        path: &Path,
        content: Pin<&mut (dyn AsyncRead + Send)>,
    ) -> Result<()> {
        let mut file = smol::fs::File::create(&path)
            .await
            .with_context(|| format!("Failed to create file at {:?}", path))?;
        futures::io::copy(content, &mut file).await?;
        Ok(())
    }

    async fn copy_file(&self, source: &Path, target: &Path, options: CopyOptions) -> Result<()> {
        if !options.overwrite && smol::fs::metadata(target).await.is_ok() {
            if options.ignore_if_exists {
                return Ok(());
            } else {
                anyhow::bail!("{target:?} already exists");
            }
        }

        smol::fs::copy(source, target).await?;
        Ok(())
    }

    async fn rename(&self, source: &Path, target: &Path, options: RenameOptions) -> Result<()> {
        if options.create_parents {
            if let Some(parent) = target.parent() {
                self.create_dir(parent).await?;
            }
        }

        if options.overwrite {
            smol::fs::rename(source, target).await?;
            return Ok(());
        }

        let use_metadata_fallback = {
            {
                let source = source.to_path_buf();
                let target = target.to_path_buf();
                match self
                    .executor
                    .spawn(async move { rename_without_replace(&source, &target) })
                    .await
                {
                    Ok(()) => return Ok(()),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        if options.ignore_if_exists {
                            return Ok(());
                        }
                        return Err(error.into());
                    }
                    Err(error)
                        if error.raw_os_error().is_some_and(|code| {
                            code == libc::ENOSYS
                                || code == libc::ENOTSUP
                                || code == libc::EOPNOTSUPP
                                || code == libc::EINVAL
                        }) =>
                    {
                        // For case when filesystem or kernel does not support atomic no-overwrite rename.
                        // EINVAL is returned by FUSE-based filesystems (e.g. NTFS via ntfs-3g)
                        // that don't support RENAME_NOREPLACE.
                        true
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        };

        if use_metadata_fallback && smol::fs::metadata(target).await.is_ok() {
            if options.ignore_if_exists {
                return Ok(());
            } else {
                anyhow::bail!("{target:?} already exists");
            }
        }

        smol::fs::rename(source, target).await?;
        Ok(())
    }

    async fn remove_dir(&self, path: &Path, options: RemoveOptions) -> Result<()> {
        let result = if options.recursive {
            smol::fs::remove_dir_all(path).await
        } else {
            smol::fs::remove_dir(path).await
        };
        match result {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound && options.ignore_if_not_exists => {
                Ok(())
            }
            Err(err) => Err(err)?,
        }
    }

    async fn remove_file(&self, path: &Path, options: RemoveOptions) -> Result<()> {
        match smol::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound && options.ignore_if_not_exists => {
                Ok(())
            }
            Err(err) => Err(err)?,
        }
    }

    async fn trash(&self, path: &Path, _options: RemoveOptions) -> Result<TrashId> {
        // We must make the path absolute or trash will make a weird abomination
        // of the zed working directory (not usually the worktree) and whatever
        // the path variable holds.
        // We deliberately use `std::path::absolute` instead of `canonicalize`
        // to avoid resolving symlinks. Otherwise trashing a symlink would trash
        // its target and leave the link behind.
        let path = std::path::absolute(path).context("Could not make the path absolute")?;

        let entry = smol::unblock(move || trash::delete_with_info(path))
            .await
            .context("Could not trash file or dir")?
            .into();

        Ok(self.trash.lock().insert(entry))
    }

    async fn open_sync(&self, path: &Path) -> Result<Box<dyn ReadSeek + Send + Sync>> {
        Ok(Box::new(std::fs::File::open(path)?))
    }

    async fn open_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);

        Ok(Arc::new(options.open(path)?))
    }

    async fn load(&self, path: &Path) -> Result<String> {
        let path = path.to_path_buf();
        self.executor
            .spawn(async move {
                std::fs::read_to_string(&path)
                    .with_context(|| format!("Failed to read file {}", path.display()))
            })
            .await
    }

    async fn load_bytes(&self, path: &Path) -> Result<Vec<u8>> {
        let path = path.to_path_buf();
        let bytes = self
            .executor
            .spawn(async move { std::fs::read(path) })
            .await?;
        Ok(bytes)
    }

    async fn atomic_write(&self, path: PathBuf, data: String) -> Result<()> {
        smol::unblock(move || {
            // Use the directory of the destination as temp dir to avoid
            // invalid cross-device link error, and XDG_CACHE_DIR for fallback.
            // See https://github.com/zed-industries/zed/pull/8437 for more details.
            let mut tmp_file =
                tempfile::NamedTempFile::new_in(path.parent().unwrap_or(paths::temp_dir()))?;
            tmp_file.write_all(data.as_bytes())?;
            tmp_file.persist(path)?;
            anyhow::Ok(())
        })
        .await?;

        Ok(())
    }

    async fn save(&self, path: &Path, text: &Rope, line_ending: LineEnding) -> Result<()> {
        let buffer_size = text.summary().len.min(10 * 1024);
        if let Some(path) = path.parent() {
            self.create_dir(path)
                .await
                .with_context(|| format!("Failed to create directory at {:?}", path))?;
        }
        let file = smol::fs::File::create(path)
            .await
            .with_context(|| format!("Failed to create file at {:?}", path))?;
        let mut writer = smol::io::BufWriter::with_capacity(buffer_size, file);
        for chunk in text::chunks_with_line_ending(text, line_ending) {
            writer.write_all(chunk.as_bytes()).await?;
        }
        writer.flush().await?;
        Ok(())
    }

    async fn write(&self, path: &Path, content: &[u8]) -> Result<()> {
        if let Some(path) = path.parent() {
            self.create_dir(path)
                .await
                .with_context(|| format!("Failed to create directory at {:?}", path))?;
        }
        let path = path.to_owned();
        let contents = content.to_owned();
        self.executor
            .spawn(async move {
                std::fs::write(path, contents)?;
                Ok(())
            })
            .await
    }

    async fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        let path = path.to_owned();
        self.executor
            .spawn(async move {
                let result = std::fs::canonicalize(&path);

                result.with_context(|| format!("canonicalizing {path:?}"))
            })
            .await
    }

    async fn is_file(&self, path: &Path) -> bool {
        let path = path.to_owned();
        self.executor
            .spawn(async move { std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) })
            .await
    }

    async fn is_dir(&self, path: &Path) -> bool {
        let path = path.to_owned();
        self.executor
            .spawn(async move { std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()) })
            .await
    }

    async fn metadata(&self, path: &Path) -> Result<Option<Metadata>> {
        let path_buf = path.to_owned();
        let symlink_metadata = match self
            .executor
            .spawn(async move { std::fs::symlink_metadata(&path_buf) })
            .await
        {
            Ok(metadata) => metadata,
            Err(err) => {
                return match err.kind() {
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => Ok(None),
                    _ => Err(anyhow::Error::new(err)),
                };
            }
        };

        let is_symlink = symlink_metadata.file_type().is_symlink();
        let metadata = if is_symlink {
            let path_buf = path.to_path_buf();
            // Read target metadata, if the target exists
            match self
                .executor
                .spawn(async move { std::fs::metadata(path_buf) })
                .await
            {
                Ok(target_metadata) => target_metadata,
                Err(err) => {
                    if err.kind() != io::ErrorKind::NotFound {
                        // TODO: Also FilesystemLoop when that's stable
                        log::warn!(
                            "Failed to read symlink target metadata for path {path:?}: {err}"
                        );
                    }
                    // For a broken or recursive symlink, return the symlink metadata. (Or
                    // as edge cases, a symlink into a directory we can't read, which is hard
                    // to distinguish from just being broken.)
                    symlink_metadata
                }
            }
        } else {
            symlink_metadata
        };

        let inode = metadata.ino();

        let is_fifo = metadata.file_type().is_fifo();

        let is_executable = metadata.is_file() && metadata.permissions().mode() & 0o111 != 0;

        Ok(Some(Metadata {
            inode,
            mtime: MTime(metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH)),
            len: metadata.len(),
            is_symlink,
            is_dir: metadata.file_type().is_dir(),
            is_fifo,
            is_executable,
            is_writable: !metadata.permissions().readonly(),
        }))
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let path = path.to_owned();
        let path = self
            .executor
            .spawn(async move { std::fs::read_link(&path) })
            .await?;
        Ok(path)
    }

    async fn read_dir(
        &self,
        path: &Path,
    ) -> Result<Pin<Box<dyn Send + Stream<Item = Result<PathBuf>>>>> {
        let path = path.to_owned();
        let entries = self
            .executor
            .spawn(async move { read_dir_entries(path) })
            .await?;
        Ok(Box::pin(iter(entries)))
    }

    fn start_native_watcher(&self) -> Result<()> {
        self.native_watcher.ensure_backend()
    }

    fn path_exists(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok()
    }

    fn is_path_case_sensitive(&self, path: &Path) -> bool {
        !fs_watcher::case_insensitive_path(path)
    }

    fn requires_poll_watcher(&self, path: &Path) -> bool {
        fs_watcher::requires_poll_watcher(path)
    }

    async fn watch(
        &self,
        path: &Path,
        latency: Duration,
    ) -> (
        Pin<Box<dyn Send + Stream<Item = Vec<PathEvent>>>>,
        Arc<dyn Watcher>,
    ) {
        let this = self
            .this
            .upgrade()
            .expect("RealFs is only constructed inside an Arc");
        fs_watcher::watch(
            this,
            self.native_watcher.clone(),
            self.poll_watcher.clone(),
            self.executor.clone(),
            path,
            latency,
        )
        .await
    }

    fn open_repo(
        &self,
        dotgit_path: &Path,
        git_binary_path: Option<&Path>,
    ) -> Result<Arc<dyn GitRepository>> {
        Ok(Arc::new(RealGitRepository::new(
            dotgit_path,
            git_binary_path.map(|path| path.to_path_buf()),
            self.executor.clone(),
        )?))
    }

    async fn git_init(
        &self,
        abs_work_directory_path: &Path,
        fallback_branch_name: String,
    ) -> Result<()> {
        let result = new_command("git")
            .current_dir(abs_work_directory_path)
            .args(&["config", "--global", "--get", "init.defaultBranch"])
            .output()
            .await;

        // In case the `git config` command fails, which would be the case if
        // the user doesn't have an `init.defaultBranch` value set, we'll just
        // default to the provided `fallback_branch_name`.
        let branch_name = match result {
            Ok(output) if !output.stdout.is_empty() => String::from_utf8(output.stdout)?,
            _ => fallback_branch_name,
        };

        new_command("git")
            .current_dir(abs_work_directory_path)
            .args(&["init", "-b"])
            .arg(branch_name.trim())
            .output()
            .await?;

        Ok(())
    }

    async fn git_clone(&self, abs_work_directory: &Path, repo_url: &str) -> Result<()> {
        let job_id = self.next_job_id.fetch_add(1, Ordering::SeqCst);
        let job_info = JobInfo {
            id: job_id,
            start: Instant::now(),
            message: SharedString::from(format!("Cloning {}", repo_url)),
        };

        let job_tracker = JobTracker::new(job_info, self.job_event_subscribers.clone());
        let mut child = new_command("git")
            .current_dir(abs_work_directory)
            .args(["clone", "--progress", repo_url])
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "/usr/bin/false")
            .env("SSH_ASKPASS", "/usr/bin/false")
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("GCM_INTERACTIVE", "never")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stderr = child
            .stderr
            .take()
            .context("failed to read git clone progress")?;
        let stderr_output = git_clone_progress::read(stderr, |message| {
            job_tracker.update(message.into());
        })
        .await?;
        let status = child.status().await?;

        if !status.success() {
            anyhow::bail!(
                "git clone failed: {}",
                git_clone_progress::failure_message(&stderr_output)
            );
        }

        Ok(())
    }

    /// Runs `git config` with the given arguments.
    /// Will return `Ok` if the commands exit status is `0`, with the stdout
    /// contents. Otherwise returns `Err` with the stderr contents.
    async fn git_config(&self, abs_work_directory: &Path, args: Vec<String>) -> Result<String> {
        let output = new_command("git")
            .current_dir(abs_work_directory)
            .args([String::from("config")].into_iter().chain(args))
            .output()
            .await?;

        if !output.status.success() {
            let err = String::from_utf8(output.stderr)?;
            anyhow::bail!(err);
        }

        String::from_utf8(output.stdout).map_err(Into::into)
    }

    fn is_fake(&self) -> bool {
        false
    }

    fn subscribe_to_jobs(&self) -> JobEventReceiver {
        let (sender, receiver) = futures::channel::mpsc::unbounded();
        self.job_event_subscribers.lock().push(sender);
        receiver
    }

    /// Checks whether the file system is case sensitive by attempting to create two files
    /// that have the same name except for the casing.
    ///
    /// It creates both files in a temporary directory it removes at the end.
    async fn is_case_sensitive(&self) -> bool {
        const UNINITIALIZED: u8 = 0;
        const CASE_SENSITIVE: u8 = 1;
        const NOT_CASE_SENSITIVE: u8 = 2;

        // Note we could CAS here, but really, if we race we do this work twice at worst which isn't a big deal.
        let load = self.is_case_sensitive.load(Ordering::Acquire);
        if load != UNINITIALIZED {
            return load == CASE_SENSITIVE;
        }
        let temp_dir = self.executor.spawn(async { TempDir::new() });
        let res = maybe!(async {
            let temp_dir = temp_dir.await?;
            let test_file_1 = temp_dir.path().join("case_sensitivity_test.tmp");
            let test_file_2 = temp_dir.path().join("CASE_SENSITIVITY_TEST.TMP");

            let create_opts = CreateOptions {
                overwrite: false,
                ignore_if_exists: false,
            };

            // Create file1
            self.create_file(&test_file_1, create_opts).await?;

            // Now check whether it's possible to create file2
            let case_sensitive = match self.create_file(&test_file_2, create_opts).await {
                Ok(_) => Ok(true),
                Err(e) => {
                    if let Some(io_error) = e.downcast_ref::<io::Error>() {
                        if io_error.kind() == io::ErrorKind::AlreadyExists {
                            Ok(false)
                        } else {
                            Err(e)
                        }
                    } else {
                        Err(e)
                    }
                }
            };

            temp_dir.close()?;
            case_sensitive
        }).await.unwrap_or_else(|e| {
            log::error!(
                "Failed to determine whether filesystem is case sensitive (falling back to true) due to error: {e:#}"
            );
            true
        });
        self.is_case_sensitive.store(
            if res {
                CASE_SENSITIVE
            } else {
                NOT_CASE_SENSITIVE
            },
            Ordering::Release,
        );
        res
    }

    fn original_path_for_trash_id(&self, trash_id: TrashId) -> Option<PathBuf> {
        self.trash
            .lock()
            .get(trash_id)
            .map(|entry| entry.original_parent.join(&entry.name))
    }

    async fn restore(&self, trash_id: TrashId) -> std::result::Result<PathBuf, TrashRestoreError> {
        let trashed_entry = self
            .trash
            .lock()
            .get(trash_id)
            .cloned()
            .ok_or(TrashRestoreError::AlreadyRestored)?;

        let restored_item_path = trashed_entry.original_parent.join(&trashed_entry.name);

        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::Builder::new()
            .name("restore trashed item".to_string())
            .spawn(move || {
                let res = trash::restore_all([trashed_entry.into_trash_item()]);
                tx.send(res)
            })
            .expect("The OS can spawn a threads");

        rx.await.expect("Restore all never panics")?;
        self.trash.lock().remove(trash_id);
        Ok(restored_item_path)
    }
}

impl Watcher for RealWatcher {
    fn add(&self, _: &Path) -> Result<()> {
        Ok(())
    }

    fn remove(&self, _: &Path) -> Result<()> {
        Ok(())
    }
}

pub async fn copy_recursive<'a>(
    fs: &'a dyn Fs,
    source: &'a Path,
    target: &'a Path,
    options: CopyOptions,
) -> Result<()> {
    for (item, is_dir) in read_dir_items(fs, source).await? {
        let Ok(item_relative_path) = item.strip_prefix(source) else {
            continue;
        };
        let target_item = if item_relative_path == Path::new("") {
            target.to_path_buf()
        } else {
            target.join(item_relative_path)
        };
        if is_dir {
            if !options.overwrite && fs.metadata(&target_item).await.is_ok_and(|m| m.is_some()) {
                if options.ignore_if_exists {
                    continue;
                } else {
                    anyhow::bail!("{target_item:?} already exists");
                }
            }
            let _ = fs
                .remove_dir(
                    &target_item,
                    RemoveOptions {
                        recursive: true,
                        ignore_if_not_exists: true,
                    },
                )
                .await;
            fs.create_dir(&target_item).await?;
        } else {
            fs.copy_file(&item, &target_item, options).await?;
        }
    }
    Ok(())
}

/// Recursively reads all of the paths in the given directory.
///
/// Returns a vector of tuples of (path, is_dir).
pub async fn read_dir_items<'a>(fs: &'a dyn Fs, source: &'a Path) -> Result<Vec<(PathBuf, bool)>> {
    let mut items = Vec::new();
    read_recursive(fs, source, &mut items).await?;
    Ok(items)
}

fn read_recursive<'a>(
    fs: &'a dyn Fs,
    source: &'a Path,
    output: &'a mut Vec<(PathBuf, bool)>,
) -> BoxFuture<'a, Result<()>> {
    use futures::future::FutureExt;

    async move {
        let metadata = fs
            .metadata(source)
            .await?
            .with_context(|| format!("path does not exist: {source:?}"))?;

        if metadata.is_dir {
            output.push((source.to_path_buf(), true));
            let mut children = fs.read_dir(source).await?;
            while let Some(child_path) = children.next().await {
                if let Ok(child_path) = child_path {
                    read_recursive(fs, &child_path, output).await?;
                }
            }
        } else {
            output.push((source.to_path_buf(), false));
        }
        Ok(())
    }
    .boxed()
}

// todo(windows)
// can we get file id not open the file twice?
// https://github.com/rust-lang/rust/issues/63010
