use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;
use std::time::Instant;

use futures::Stream;
use futures::StreamExt as _;
use gpui::BackgroundExecutor;
use gpui::Task;
use notify::Event;
use notify::EventKind;
use parking_lot::Mutex;
use util::ResultExt;
use util::paths::SanitizedPath;

use crate::Fs;
use crate::PathEvent;
use crate::PathEventKind;
use crate::Watcher;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsWatcherKind {
    Native,
    Poll,
}

impl OsWatcherKind {
    /// Poll watches and native macOS/Windows watches cover their whole subtree;
    /// native Linux (inotify) watches cover one directory.
    fn is_recursive(self) -> bool {
        self == Self::Poll || cfg!(target_os = "macos")
    }
}

pub(crate) async fn watch(
    fs: Arc<dyn Fs>,
    native_watcher: Arc<OsWatcher>,
    poll_watcher: Arc<OsWatcher>,
    executor: BackgroundExecutor,
    path: &Path,
    latency: Duration,
) -> (Pin<Box<dyn Send + Stream<Item = Vec<PathEvent>>>>, Arc<dyn Watcher>) {
    let (tx, rx) = async_channel::unbounded();
    let pending_paths: Arc<Mutex<Vec<PathEvent>>> = Arc::default();

    let watcher: Arc<dyn Watcher> =
        Arc::new(FsWatcher::new(native_watcher, poll_watcher, fs.clone(), executor.clone(), tx, pending_paths.clone()));

    if let Err(e) = watcher.add(path) {
        log::warn!("Failed to watch {}:\n{e}", path.display());
    }

    // Check if path is a symlink and follow the target parent
    if let Ok(mut target) = fs.read_link(path).await {
        log::trace!("watch symlink {path:?} -> {target:?}");
        // Check if symlink target is relative path, if so make it absolute
        if target.is_relative()
            && let Some(parent) = path.parent()
        {
            target = parent.join(target);
            if let Ok(canonical) = fs.canonicalize(&target).await {
                target = SanitizedPath::new(&canonical).as_path().to_path_buf();
            }
        }
        watcher.add(&target).ok();
        // Skipped for poll watchers: PollWatcher::watch() recursively scans
        // at registration, blocking on large virtual filesystem mounts
        if let Some(parent) = target.parent()
            && !fs.requires_poll_watcher(parent)
        {
            watcher.add(parent).log_err();
        }
    }

    (
        Box::pin(rx.filter_map({
            let watcher = watcher.clone();
            move |_| {
                let _ = watcher.clone();
                let pending_paths = pending_paths.clone();
                let executor = executor.clone();
                async move {
                    if !latency.is_zero() {
                        executor.timer(latency).await;
                    }
                    let paths = std::mem::take(&mut *pending_paths.lock());
                    log::debug!("pending path events: {:?}", paths);
                    (!paths.is_empty()).then_some(paths)
                }
            }
        })),
        watcher,
    )
}

pub struct FsWatcher {
    native_watcher: Arc<OsWatcher>,
    poll_watcher: Arc<OsWatcher>,
    fs: Arc<dyn Fs>,
    executor: BackgroundExecutor,
    tx: async_channel::Sender<()>,
    pending_path_events: Arc<Mutex<Vec<PathEvent>>>,
    registrations: Arc<Mutex<HashMap<WatchKey, FsWatcherRegistration>>>,
    pending_registrations: Arc<Mutex<HashMap<Arc<std::path::Path>, Task<()>>>>,
}

#[derive(Clone)]
struct FsWatcherRegistration {
    id: WatcherRegistrationId,
    os_watcher: Arc<OsWatcher>,
}

impl FsWatcher {
    pub(crate) fn new(
        native_watcher: Arc<OsWatcher>,
        poll_watcher: Arc<OsWatcher>,
        fs: Arc<dyn Fs>,
        executor: BackgroundExecutor,
        tx: async_channel::Sender<()>,
        pending_path_events: Arc<Mutex<Vec<PathEvent>>>,
    ) -> Self {
        Self {
            native_watcher,
            poll_watcher,
            fs,
            executor,
            tx,
            pending_path_events,
            registrations: Arc::default(),
            pending_registrations: Arc::default(),
        }
    }

    fn add_existing_path(&self, path: Arc<Path>) -> anyhow::Result<()> {
        let case_insensitive = !self.fs.is_path_case_sensitive(&path);
        let key = WatchKey::for_registration(SanitizedPath::new(&path), case_insensitive);
        let existing = self.registrations.lock().get(&key).cloned();
        if let Some(existing) = existing {
            log::trace!("path to watch is already registered: {path:?}");
            return existing.os_watcher.rewatch_if_stale(
                &mut existing.os_watcher.state.lock(),
                SanitizedPath::new(&path),
                &key,
            );
        }
        match register_existing_path(
            &self.native_watcher,
            &self.poll_watcher,
            self.fs.as_ref(),
            path.clone(),
            case_insensitive,
            self.tx.clone(),
            self.pending_path_events.clone(),
        )? {
            Some(registration) => {
                self.registrations.lock().insert(key, registration);
            }
            None => {
                // Registration was skipped (e.g. the native watch-limit cooldown
                // is active). Retry in the background rather than silently leaving
                // the path unwatched forever.
                log::warn!("watch registration for {path:?} was skipped; retrying in background");
                self.add_pending_path(path);
            }
        }
        Ok(())
    }

    fn add_pending_path(&self, path: Arc<Path>) {
        let mut pending_registrations = self.pending_registrations.lock();
        if pending_registrations.contains_key(path.as_ref()) {
            return;
        }

        let task = self.executor.spawn(poll_path_until_created(
            self.native_watcher.clone(),
            self.poll_watcher.clone(),
            self.fs.clone(),
            self.executor.clone(),
            path.clone(),
            self.tx.clone(),
            self.pending_path_events.clone(),
            self.registrations.clone(),
            self.pending_registrations.clone(),
        ));
        pending_registrations.insert(path, task);
    }
}

impl Drop for FsWatcher {
    fn drop(&mut self) {
        self.pending_registrations.lock().clear();

        let mut registrations = HashMap::new();
        {
            let old = &mut self.registrations.lock();
            std::mem::swap(&mut **old, &mut registrations);
        }

        for (_, registration) in registrations {
            registration.os_watcher.remove(registration.id);
        }
    }
}

impl Watcher for FsWatcher {
    fn add(&self, path: &std::path::Path) -> anyhow::Result<()> {
        log::trace!("watcher add: {path:?}");

        let path: Arc<Path> = path.into();
        if path_covered_by_recursive_registration(&self.registrations.lock(), SanitizedPath::new(&path)) {
            log::trace!("path to watch is covered by an existing registration: {path:?}");
            return Ok(());
        }

        if self.pending_registrations.lock().contains_key(path.as_ref()) {
            log::trace!("path to watch is already pending: {path:?}");
            return Ok(());
        }

        if !self.fs.path_exists(&path) {
            self.add_pending_path(path);
            return Ok(());
        }

        self.add_existing_path(path)
    }

    fn remove(&self, path: &std::path::Path) -> anyhow::Result<()> {
        log::trace!("remove watched path: {path:?}");
        self.pending_registrations.lock().remove(path);

        let sanitized = SanitizedPath::new(path);
        let registration = {
            let mut registrations = self.registrations.lock();
            registrations
                .remove(&WatchKey::exact(sanitized))
                .or_else(|| registrations.remove(&WatchKey::folded(sanitized)))
        };
        if let Some(registration) = registration {
            registration.os_watcher.remove(registration.id);
        }
        Ok(())
    }
}

/// Whether a recursive registration on a strict ancestor of `path` already covers
/// it. Both key spellings are probed so a folded registration still matches; only
/// poll watches and native macOS/Windows watches are recursive.
fn path_covered_by_recursive_registration(
    registrations: &HashMap<WatchKey, FsWatcherRegistration>,
    path: &SanitizedPath,
) -> bool {
    path.as_path().ancestors().skip(1).any(|ancestor| {
        let ancestor = SanitizedPath::unchecked_new(ancestor);
        [WatchKey::exact(ancestor), WatchKey::folded(ancestor)]
            .iter()
            .any(|key| registrations.get(key).is_some_and(|registration| registration.os_watcher.is_recursive()))
    })
}

/// Detect whether a path requires polling instead of native file watching.
///
/// Returns `true` for filesystem types where inotify/FSEvents/ReadDirectoryChanges
/// silently fail to deliver events: 9P, NFS, CIFS/SMB, FUSE (sshfs), etc.
///
/// Can be overridden with the `ZED_FILE_WATCHER_MODE` environment variable:
/// - `native` — always use native OS watcher
/// - `poll` — always use polling
/// - `auto` (default) — auto-detect based on filesystem type
pub fn requires_poll_watcher(path: &Path) -> bool {
    match std::env::var("ZED_FILE_WATCHER_MODE").as_deref().unwrap_or("auto") {
        "native" => return false,
        "poll" => return true,
        _ => {}
    }

    #[cfg(target_os = "linux")]
    {
        detect_requires_poll_watcher_linux(path)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = path;
        false
    }
}

fn register_existing_path(
    native_watcher: &Arc<OsWatcher>,
    poll_watcher: &Arc<OsWatcher>,
    fs: &dyn Fs,
    path: Arc<Path>,
    case_insensitive: bool,
    tx: async_channel::Sender<()>,
    pending_path_events: Arc<Mutex<Vec<PathEvent>>>,
) -> anyhow::Result<Option<FsWatcherRegistration>> {
    let os_watcher = if fs.requires_poll_watcher(path.as_ref()) {
        log::info!("Using poll watcher ({}ms interval) for {}", poll_interval().as_millis(), path.display());

        poll_watcher
    } else {
        native_watcher
    };
    let root_path = SanitizedPath::new_arc(path.as_ref());
    let path_for_callback = path.clone();
    let Some(registration_id) = os_watcher.add(path, case_insensitive, move |event: &notify::Event| {
        log::trace!("watcher received event: {event:?}");
        push_notify_event(&tx, &pending_path_events, &root_path, case_insensitive, path_for_callback.as_ref(), event);
    })?
    else {
        return Ok(None);
    };
    Ok(Some(FsWatcherRegistration { id: registration_id, os_watcher: os_watcher.clone() }))
}

/// Whether the volume backing `path` does case-insensitive name lookups, used to
/// pick exact vs. folded matching.
#[cfg(target_os = "macos")]
pub(crate) fn case_insensitive_path(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;

    // `pathconf(_PC_CASE_SENSITIVE)` returns 1 (sensitive), 0 (insensitive), or -1
    // on error; default errors to insensitive (the APFS/HFS+ default).
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return true;
    };
    // SAFETY: We just initialized c_path, so it's a valid pointer
    unsafe { libc::pathconf(c_path.as_ptr(), libc::_PC_CASE_SENSITIVE) == 0 }
}

/// Whether `path` is `root` or sits beneath it, folding case on case-insensitive
/// volumes so a differently-cased spelling still matches.
fn path_is_under(path: &SanitizedPath, root: &SanitizedPath, case_insensitive: bool) -> bool {
    if case_insensitive {
        let path = path.as_path().to_string_lossy().to_lowercase();
        let root = root.as_path().to_string_lossy().to_lowercase();
        Path::new(&path).starts_with(Path::new(&root))
    } else {
        path.starts_with(root)
    }
}

/// Lookup key for a watch path, shared by add, remove, and dispatch so they all
/// agree on whether two spellings denote the same directory.
///
/// On case-sensitive volumes the exact (sanitized) path is the key, so genuinely
/// distinct directories stay distinct. On case-insensitive volumes the folded
/// (lowercased) spelling is the key, so any casing of a directory collides.
///
/// The two variants are distinct map keys, so a case-sensitive registration can
/// never be hit by a folded lookup (or vice versa); dispatch can therefore probe
/// both forms of an event path without risking a cross-rule false match.
///
/// Folded keys use path-component spelling so separators and `.` components
/// agree with exact path equality. macOS also normalizes Unicode before folding.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum WatchKey {
    Exact(Arc<Path>),
    Folded(Arc<str>),
}

impl WatchKey {
    fn exact(path: &SanitizedPath) -> Self {
        Self::Exact(Arc::from(path.as_path()))
    }

    fn folded(path: &SanitizedPath) -> Self {
        // Event ancestors omit trailing separators, even when registration paths include them.
        let normalized_path = path.as_path().components().collect::<std::path::PathBuf>();
        let lossy = normalized_path.to_string_lossy();
        // macOS (APFS/HFS+) compares names normalization-insensitively (NFC vs
        // NFD), and FSEvents can report NFD while a config/LSP supplies NFC, so
        // normalize before folding case. Windows (NTFS) and Linux are
        // normalization-sensitive, so there we only fold case.

        #[cfg(target_os = "macos")]
        let folded = {
            use unicode_normalization::UnicodeNormalization as _;
            lossy.chars().nfc().collect::<String>().to_lowercase()
        };

        #[cfg(target_os = "linux")]
        let folded = lossy.to_lowercase();

        Self::Folded(folded.into())
    }

    fn for_registration(path: &SanitizedPath, case_insensitive: bool) -> Self {
        if case_insensitive { Self::folded(path) } else { Self::exact(path) }
    }
}

async fn poll_path_until_created(
    native_watcher: Arc<OsWatcher>,
    poll_watcher: Arc<OsWatcher>,
    fs: Arc<dyn Fs>,
    executor: BackgroundExecutor,
    path: Arc<Path>,
    tx: async_channel::Sender<()>,
    pending_path_events: Arc<Mutex<Vec<PathEvent>>>,
    registrations: Arc<Mutex<HashMap<WatchKey, FsWatcherRegistration>>>,
    pending_registrations: Arc<Mutex<HashMap<Arc<Path>, Task<()>>>>,
) {
    loop {
        executor.timer(poll_interval()).await;

        if !pending_registrations.lock().contains_key(path.as_ref()) {
            return;
        }

        if !fs.path_exists(&path) {
            continue;
        }

        // Probe case sensitivity now that the path exists, rather than at add
        // time when it didn't.
        let case_insensitive = if fs.is_fake() {
            !fs.is_path_case_sensitive(&path)
        } else {
            smol::unblock({
                let path = path.clone();
                let fs = fs.clone();
                move || !fs.is_path_case_sensitive(&path)
            })
            .await
        };
        let key = WatchKey::for_registration(SanitizedPath::new(&path), case_insensitive);

        if registrations.lock().contains_key(&key) {
            pending_registrations.lock().remove(path.as_ref());
            return;
        }

        let register = {
            let path = path.clone();
            let tx = tx.clone();
            let pending_path_events = pending_path_events.clone();
            let fs = fs.clone();
            let native_watcher = native_watcher.clone();
            let poll_watcher = poll_watcher.clone();
            move || {
                register_existing_path(
                    &native_watcher,
                    &poll_watcher,
                    fs.as_ref(),
                    path,
                    case_insensitive,
                    tx,
                    pending_path_events,
                )
            }
        };
        let registration = if fs.is_fake() { register() } else { smol::unblock(register).await };

        match registration {
            Ok(Some(registration)) => {
                {
                    let mut pending_registrations = pending_registrations.lock();
                    if pending_registrations.remove(path.as_ref()).is_none() {
                        registration.os_watcher.remove(registration.id);
                        return;
                    }
                    registrations.lock().insert(key, registration);
                }
                enqueue_path_events(
                    &tx,
                    &pending_path_events,
                    vec![
                        PathEvent { path: path.to_path_buf(), kind: Some(PathEventKind::Created) },
                        PathEvent { path: path.to_path_buf(), kind: Some(PathEventKind::Rescan) },
                    ],
                );
                return;
            }
            Ok(None) => {}
            Err(error) => {
                log::warn!("failed to watch newly-created path {path:?}: {error}; retrying");
            }
        }
    }
}

fn enqueue_path_events(
    tx: &smol::channel::Sender<()>,
    pending_path_events: &Arc<Mutex<Vec<PathEvent>>>,
    mut path_events: Vec<PathEvent>,
) {
    if path_events.is_empty() {
        return;
    }

    path_events.sort();
    let mut pending_paths = pending_path_events.lock();
    if pending_paths.is_empty() {
        tx.try_send(()).ok();
    }
    coalesce_pending_rescans(&mut pending_paths, &mut path_events);
    util::extend_sorted(&mut *pending_paths, path_events, usize::MAX, |a, b| a.path.cmp(&b.path));
}

fn push_notify_event(
    tx: &smol::channel::Sender<()>,
    pending_path_events: &Arc<Mutex<Vec<PathEvent>>>,
    root_path: &SanitizedPath,
    case_insensitive: bool,
    watched_root: &Path,
    event: &notify::Event,
) {
    let kind = match event.kind {
        EventKind::Create(_) => Some(PathEventKind::Created),
        EventKind::Modify(_) => Some(PathEventKind::Changed),
        EventKind::Remove(_) => Some(PathEventKind::Removed),
        _ => None,
    };
    let mut path_events = event
        .paths
        .iter()
        .filter_map(|event_path| {
            let event_path = SanitizedPath::new(event_path);
            path_is_under(event_path, root_path, case_insensitive)
                .then(|| PathEvent { path: event_path.as_path().to_path_buf(), kind })
        })
        .collect::<Vec<_>>();

    if event.need_rescan() {
        // A rescan event names the subtrees that lost sync (FSEvents
        // `MustScanSubDirs`); only a pathless one (inotify queue overflow) means
        // the whole watched root is suspect.
        if path_events.is_empty() {
            path_events.push(PathEvent { path: watched_root.to_path_buf(), kind: Some(PathEventKind::Rescan) });
        } else {
            for path_event in &mut path_events {
                path_event.kind = Some(PathEventKind::Rescan);
            }
        }
    }
    log::trace!("path_events: {:?}", path_events);
    enqueue_path_events(tx, pending_path_events, path_events);
}

fn coalesce_pending_rescans(pending_paths: &mut Vec<PathEvent>, path_events: &mut Vec<PathEvent>) {
    if !path_events.iter().any(|event| event.kind == Some(PathEventKind::Rescan)) {
        return;
    }

    let mut new_rescan_paths: Vec<std::path::PathBuf> =
        path_events.iter().filter(|e| e.kind == Some(PathEventKind::Rescan)).map(|e| e.path.clone()).collect();
    new_rescan_paths.sort_unstable();

    let mut deduped_rescans: Vec<std::path::PathBuf> = Vec::with_capacity(new_rescan_paths.len());
    for path in new_rescan_paths {
        if deduped_rescans.iter().any(|ancestor| path != *ancestor && path.starts_with(ancestor)) {
            continue;
        }
        deduped_rescans.push(path);
    }

    deduped_rescans.retain(|new_path| {
        !pending_paths.iter().any(|pending| is_covered_rescan(pending.kind, new_path, &pending.path))
    });

    if !deduped_rescans.is_empty() {
        pending_paths.retain(|pending| {
            !deduped_rescans.iter().any(|rescan_path| {
                pending.path == *rescan_path || is_covered_rescan(pending.kind, &pending.path, rescan_path)
            })
        });
    }

    path_events.retain(|event| event.kind != Some(PathEventKind::Rescan) || deduped_rescans.contains(&event.path));
}

fn is_covered_rescan(kind: Option<PathEventKind>, path: &Path, ancestor: &Path) -> bool {
    kind == Some(PathEventKind::Rescan) && path != ancestor && path.starts_with(ancestor)
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct WatcherRegistrationId(u32);

struct WatcherRegistrationState {
    callback: Arc<dyn Fn(&notify::Event) + Send + Sync>,
    key: WatchKey,
    path: Arc<SanitizedPath>,
}

struct PathRegistrationState {
    watcher_ids: Vec<WatcherRegistrationId>,
    has_os_watcher: bool,
    stale: bool,
}

/// The registered watch paths for one backend, keyed by [`WatchKey`] so that
/// add (dedup), remove, and dispatch share a single notion of path identity.
#[derive(Default)]
struct WatchPaths(HashMap<WatchKey, PathRegistrationState>);

impl WatchPaths {
    fn contains(&self, key: &WatchKey) -> bool {
        self.0.contains_key(key)
    }

    fn get_mut(&mut self, key: &WatchKey) -> Option<&mut PathRegistrationState> {
        self.0.get_mut(key)
    }

    fn entry(&mut self, key: WatchKey) -> std::collections::hash_map::Entry<'_, WatchKey, PathRegistrationState> {
        self.0.entry(key)
    }

    fn remove(&mut self, key: &WatchKey) {
        self.0.remove(key);
    }

    /// True if a recursive registration on a strict ancestor already covers
    /// `path`. Only poll watches and native macOS/Windows watches are recursive.
    fn covered_by_recursive_ancestor(&self, path: &SanitizedPath, recursive: bool) -> bool {
        if !recursive {
            return false;
        }
        path.as_path().ancestors().skip(1).any(|ancestor| {
            let ancestor = SanitizedPath::unchecked_new(ancestor);
            self.0.contains_key(&WatchKey::exact(ancestor)) || self.0.contains_key(&WatchKey::folded(ancestor))
        })
    }

    /// Collects the watcher ids of every registration whose directory is an
    /// ancestor of (or equal to) `path`. Both exact and folded keys are probed,
    /// so a real-cased event path matches a folded registration and vice versa.
    fn watcher_ids_covering(&self, path: &SanitizedPath, ids: &mut Vec<WatcherRegistrationId>) {
        for ancestor in path.as_path().ancestors() {
            let ancestor = SanitizedPath::unchecked_new(ancestor);
            if let Some(registration) = self.0.get(&WatchKey::exact(ancestor)) {
                ids.extend_from_slice(&registration.watcher_ids);
            }
            if let Some(registration) = self.0.get(&WatchKey::folded(ancestor)) {
                ids.extend_from_slice(&registration.watcher_ids);
            }
        }
    }
}

struct WatcherState {
    watchers: HashMap<WatcherRegistrationId, WatcherRegistrationState>,
    paths: WatchPaths,
    cooldown_until: Option<Instant>,
    last_registration: WatcherRegistrationId,
}

impl WatcherState {
    fn is_native_watch_limit_cooldown_active(&self) -> bool {
        self.cooldown_until.is_some_and(|cooldown_until| cooldown_until > Instant::now())
    }

    fn start_native_watch_limit_cooldown(&mut self, path: &Path) {
        let should_log = !self.is_native_watch_limit_cooldown_active();
        self.cooldown_until = Some(Instant::now() + *NATIVE_WATCH_LIMIT_COOLDOWN);
        if should_log {
            log::warn!(
                "OS file watch limit reached while watching {path:?}; skipping new native file watcher registrations for {} seconds",
                NATIVE_WATCH_LIMIT_COOLDOWN.as_secs()
            );
        }
    }

    fn remove_registration(&mut self, id: WatcherRegistrationId) -> Option<Arc<SanitizedPath>> {
        let registration_state = self.watchers.remove(&id)?;
        let path_state = self.paths.get_mut(&registration_state.key)?;
        path_state.watcher_ids.retain(|&existing| existing != id);
        if !path_state.watcher_ids.is_empty() {
            return None;
        }

        let was_actually_watched = path_state.has_os_watcher;
        self.paths.remove(&registration_state.key);

        was_actually_watched.then_some(registration_state.path)
    }
}

pub(crate) trait WatchBackend: Send {
    fn watch(&mut self, path: &Path, mode: notify::RecursiveMode) -> notify::Result<()>;
    fn unwatch(&mut self, path: &Path) -> notify::Result<()>;
}

impl<T: notify::Watcher + Send> WatchBackend for T {
    fn watch(&mut self, path: &Path, mode: notify::RecursiveMode) -> notify::Result<()> {
        notify::Watcher::watch(self, path, mode)
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        notify::Watcher::unwatch(self, path)
    }
}

pub struct OsWatcher {
    kind: OsWatcherKind,
    recursive: bool,
    state: Arc<Mutex<WatcherState>>,

    // Hold state across backend mutations. Callbacks don't acquire state or
    // invert the lock order.
    backend: Mutex<Option<Box<dyn WatchBackend>>>,
    event_tx: async_channel::Sender<notify::Result<notify::Event>>,
    _dispatch_task: Task<()>,
    // Closing the event channel can wake the dispatch task. Keep its scheduler
    // alive through channel and task teardown, even when our owner drops first.
    _executor: BackgroundExecutor,
}

impl OsWatcher {
    /// Watches with notify's backend of `kind`, created lazily on first use so
    /// that a failure to start it can be reported by `ensure_backend`.
    pub fn new(kind: OsWatcherKind, executor: BackgroundExecutor) -> Arc<Self> {
        Self::with_backend(kind, executor, None)
    }

    /// Watches with the given backend in place of notify's; `event_sink` is how
    /// that backend reports events, standing in for notify's `EventHandler`.
    pub(crate) fn with_backend(
        kind: OsWatcherKind,
        executor: BackgroundExecutor,
        backend: Option<Box<dyn WatchBackend>>,
    ) -> Arc<Self> {
        let (event_tx, event_rx) = async_channel::unbounded();
        let state = Arc::new(Mutex::new(WatcherState {
            watchers: HashMap::default(),
            paths: WatchPaths::default(),
            cooldown_until: None,
            last_registration: WatcherRegistrationId::default(),
        }));
        let dispatch_task = executor.spawn({
            let state = state.clone();
            async move {
                let mut rescan_history = RescanHistory::default();
                while let Ok(first) = event_rx.recv().await {
                    dispatch_batch(kind, &state, &mut rescan_history, first, &event_rx);
                }
            }
        });
        Arc::new(Self {
            kind,
            recursive: kind.is_recursive(),
            state,
            backend: Mutex::new(backend),
            event_tx,
            _dispatch_task: dispatch_task,
            _executor: executor,
        })
    }

    /// The callback a substitute backend uses to report events, standing
    /// in for the `EventHandler` notify would have been constructed with.
    pub(crate) fn event_sink(&self) -> impl Fn(notify::Result<notify::Event>) + Send + Sync + 'static {
        let event_tx = self.event_tx.clone();
        move |event| {
            enqueue(&event_tx, event);
        }
    }

    pub(crate) const fn is_recursive(&self) -> bool {
        self.recursive
    }

    fn add(
        &self,
        path: Arc<std::path::Path>,
        case_insensitive: bool,
        cb: impl Fn(&notify::Event) + Send + Sync + 'static,
    ) -> anyhow::Result<Option<WatcherRegistrationId>> {
        let path = SanitizedPath::from_arc(path);
        let key = WatchKey::for_registration(&path, case_insensitive);
        let mut state = self.state.lock();
        let path_already_covered = state.paths.covered_by_recursive_ancestor(&path, self.recursive);
        let path_already_registered = state.paths.contains(&key);

        if path_already_registered {
            self.rewatch_if_stale(&mut state, &path, &key)?;
        } else if !path_already_covered {
            if self.kind == OsWatcherKind::Native && state.is_native_watch_limit_cooldown_active() {
                return Ok(None);
            }

            match self.watch(path.as_path()) {
                Ok(()) => {}
                Err(error) if self.kind == OsWatcherKind::Native && is_max_files_watch_error(&error) => {
                    state.start_native_watch_limit_cooldown(path.as_path());
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }

        let id = state.last_registration;
        state.last_registration = WatcherRegistrationId(id.0 + 1);

        let registration_state = WatcherRegistrationState { callback: Arc::new(cb), key: key.clone(), path };
        state.watchers.insert(id, registration_state);
        state.paths.entry(key).and_modify(|registration| registration.watcher_ids.push(id)).or_insert_with(|| {
            PathRegistrationState { watcher_ids: vec![id], has_os_watcher: !path_already_covered, stale: false }
        });

        Ok(Some(id))
    }

    fn rewatch_if_stale(&self, state: &mut WatcherState, path: &SanitizedPath, key: &WatchKey) -> anyhow::Result<()> {
        let Some(path_state) = state.paths.get_mut(key) else {
            return Ok(());
        };
        if !path_state.stale {
            return Ok(());
        }

        log::trace!("rewatching stale path: {path:?}");
        // A renamed directory's watch survives under the old path; unwatch it
        // so the backend won't skip installation as a duplicate.
        self.unwatch(path.as_path()).log_err();
        self.watch(path.as_path())?;
        path_state.stale = false;
        Ok(())
    }

    pub fn remove(&self, id: WatcherRegistrationId) {
        let mut state = self.state.lock();
        let Some(path) = state.remove_registration(id) else {
            return;
        };
        self.unwatch(path.as_path()).log_err();
    }

    fn watch(&self, path: &Path) -> anyhow::Result<()> {
        self.ensure_backend()?;
        self.backend.lock().as_mut().expect("watcher backend initialized").watch(
            path,
            if self.recursive { notify::RecursiveMode::Recursive } else { notify::RecursiveMode::NonRecursive },
        )?;
        Ok(())
    }

    fn unwatch(&self, path: &Path) -> anyhow::Result<()> {
        let watcher = self.backend.lock().as_mut().map(|watcher| watcher.unwatch(path));

        match watcher {
            // inotify auto-removes a watch when its directory is deleted, so a
            // later unwatch races that and fails with a benign error. Either way
            // the path is no longer watched, which is all we wanted.
            Some(Err(error)) if !matches!(error.kind, notify::ErrorKind::WatchNotFound) => Err(error.into()),
            _ => Ok(()),
        }
    }

    pub(crate) fn ensure_backend(&self) -> anyhow::Result<()> {
        // The lock is held across creation: with a check-then-insert under two
        // separate lock acquisitions, concurrent callers could each create a
        // watcher and the loser's insert would silently drop the winner's
        // watcher along with every path registered on it.
        let mut backend = self.backend.lock();
        if backend.is_none() {
            *backend = Some(match self.kind {
                OsWatcherKind::Native => {
                    // CORE excludes Access events, which Zed discards anyway. Without this,
                    // the default mask subscribes to inotify OPEN/CLOSE_* on Linux, so every
                    // file read in a watched directory would queue events, increasing the
                    // risk of queue overflows (and thus full rescans) under read-heavy
                    // workloads like grep or language server indexing.
                    let config = notify::Config::default().with_event_kinds(notify::EventKindMask::CORE);
                    Box::new(<notify::RecommendedWatcher as notify::Watcher>::new(self.event_sink(), config)?)
                }
                OsWatcherKind::Poll => {
                    let config = notify::Config::default().with_poll_interval(*POLL_INTERVAL);
                    Box::new(notify::PollWatcher::new(self.event_sink(), config)?)
                }
            });
        }
        Ok(())
    }
}

fn dispatch(kind: OsWatcherKind, state: &Mutex<WatcherState>, event: notify::Result<notify::Event>) {
    let event = match event {
        Ok(event) => event,
        Err(error) => {
            log::warn!("{kind:?} watcher error: {error}");
            return;
        }
    };

    log::trace!("handle {kind:?} watcher event: {event:?}");

    let callbacks = {
        let mut state = state.lock();
        // Invalidate before callbacks can trigger a scan.
        if !kind.is_recursive() && event.need_rescan() {
            // Overflow may have dropped removal events.
            for path_state in state.paths.0.values_mut() {
                path_state.stale = true;
            }
        } else if !kind.is_recursive()
            && matches!(event.kind, EventKind::Remove(_) | EventKind::Modify(notify::event::ModifyKind::Name(_)))
        {
            for path in &event.paths {
                let path = SanitizedPath::new(path);
                for event_key in [WatchKey::exact(path), WatchKey::folded(path)] {
                    let Some(path_state) = state.paths.get_mut(&event_key) else {
                        continue;
                    };
                    path_state.stale = true;
                    if !matches!(event.kind, EventKind::Modify(_)) {
                        continue;
                    }
                    // Descendant watches follow the directory without their own rename events.
                    for (key, path_state) in &mut state.paths.0 {
                        let descendant = match (key, &event_key) {
                            (WatchKey::Exact(key), WatchKey::Exact(event_key)) => key.starts_with(event_key),
                            (WatchKey::Folded(key), WatchKey::Folded(event_key)) => {
                                Path::new(key.as_ref()).starts_with(Path::new(event_key.as_ref()))
                            }
                            _ => false,
                        };
                        if descendant {
                            path_state.stale = true;
                        }
                    }
                }
            }
        }
        if event.need_rescan() {
            state.watchers.values().map(|registration| registration.callback.clone()).collect::<Vec<_>>()
        } else {
            let mut ids = Vec::new();
            for path in &event.paths {
                let sanitized = SanitizedPath::new(path);
                state.paths.watcher_ids_covering(sanitized, &mut ids);
            }
            ids.sort_unstable_by_key(|id| id.0);
            ids.dedup();
            ids.into_iter()
                .filter_map(|id| state.watchers.get(&id))
                .map(|registration| registration.callback.clone())
                .collect::<Vec<_>>()
        }
    };

    for callback in callbacks {
        callback(&event);
    }
}

const RESCAN_PATH_HISTORY_CAPACITY: usize = 10;

#[derive(Default)]
struct RescanHistory {
    paths: VecDeque<PathBuf>,
}

impl RescanHistory {
    fn record(&mut self, event: &notify::Event) -> Option<String> {
        let report = event.need_rescan().then(|| {
            // Capture before inserting the rescan so the report includes all ten
            // preceding paths, even when this is the start of an overflow burst.
            format!(
                "reason={:?}, paths={:?}, recent_paths={:?}",
                event.info().unwrap_or("unspecified"),
                event.paths,
                self.paths,
            )
        });
        let first_path = event.paths.len().saturating_sub(RESCAN_PATH_HISTORY_CAPACITY);
        for path in event.paths.iter().skip(first_path) {
            if self.paths.len() == RESCAN_PATH_HISTORY_CAPACITY {
                self.paths.pop_front();
            }
            self.paths.push_back(path.clone());
        }
        report
    }
}

fn dispatch_batch(
    kind: OsWatcherKind,
    state: &Mutex<WatcherState>,
    rescan_history: &mut RescanHistory,
    first: notify::Result<notify::Event>,
    event_rx: &async_channel::Receiver<notify::Result<notify::Event>>,
) {
    // A single backend overflow can enqueue many pathless rescan markers, each
    // of which rescans every watched root, so one covers the entire
    // drained batch. Rescans that name paths only cover those subtrees and are
    // all dispatched; ordinary events still run.
    let mut rescan_dispatched = false;

    for event in std::iter::once(first).chain(std::iter::from_fn(|| event_rx.try_recv().ok())) {
        if event.as_ref().is_ok_and(|event| event.need_rescan() && event.paths.is_empty()) {
            if rescan_dispatched {
                continue;
            }
            rescan_dispatched = true;
        }

        // Log writes can block, so keep them off the native watcher reader.
        if let Ok(event) = &event
            && let Some(report) = rescan_history.record(event)
        {
            log::error!("{kind:?} filesystem watcher requested rescan: {report}");
        }
        dispatch(kind, state, event);
    }
}

fn enqueue(event_tx: &async_channel::Sender<notify::Result<notify::Event>>, event: notify::Result<notify::Event>) {
    if matches!(event, Ok(Event { kind: EventKind::Access(_), .. })) {
        return;
    }

    // A failed send only happens once the dispatch thread has shut down, at
    // which point there's nothing left to dispatch to.
    event_tx.try_send(event).ok();
}

fn is_max_files_watch_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<notify::Error>().is_some_and(|error| matches!(&error.kind, notify::ErrorKind::MaxFilesWatch))
}

static POLL_INTERVAL: LazyLock<Duration> = LazyLock::new(|| {
    let poll_ms: u64 = std::env::var("ZED_FILE_WATCHER_POLL_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2000)
        .clamp(500, 30000);
    Duration::from_millis(poll_ms)
});

static NATIVE_WATCH_LIMIT_COOLDOWN: LazyLock<Duration> = LazyLock::new(|| {
    let cooldown_seconds: u64 = std::env::var("ZED_NATIVE_WATCH_LIMIT_COOLDOWN_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5)
        .clamp(0, 300);
    Duration::from_secs(cooldown_seconds)
});

pub fn poll_interval() -> Duration {
    *POLL_INTERVAL
}

#[cfg(target_os = "linux")]
pub(crate) const fn case_insensitive_path(_path: &Path) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn detect_requires_poll_watcher_linux(path: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = match CString::new(path.as_os_str().as_bytes()) {
        Ok(p) => p,
        Err(_) => return false,
    };

    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c_path.as_ptr(), &raw mut stat) } != 0 {
        return false;
    }

    const V9FS_MAGIC: u64 = 0x0102_1997;
    const NFS_SUPER_MAGIC: u64 = 0x0000_6969;
    const CIFS_MAGIC: u64 = 0xFF53_4D42;
    const SMB_SUPER_MAGIC: u64 = 0x0000_517B;
    const SMB2_MAGIC: u64 = 0xFE53_4D42;
    const FUSE_SUPER_MAGIC: u64 = 0x6573_5546;

    let fs_type = (stat.f_type as u64) & 0xFFFF_FFFF;
    if fs_type == FUSE_SUPER_MAGIC && is_virtiofs(path) {
        return false;
    }

    if fs_type == V9FS_MAGIC
        || fs_type == NFS_SUPER_MAGIC
        || fs_type == CIFS_MAGIC
        || fs_type == SMB_SUPER_MAGIC
        || fs_type == SMB2_MAGIC
        || fs_type == FUSE_SUPER_MAGIC
    {
        log::info!(
            "Detected network/virtual filesystem (type 0x{:x}) at {}, using poll watcher",
            fs_type,
            path.display()
        );
        return true;
    }

    false
}

#[cfg(target_os = "linux")]
fn is_virtiofs(path: &Path) -> bool {
    let Ok(mountinfo) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };

    let mut best_mount = None;
    for line in mountinfo.lines() {
        let fields = line.split(' ').collect::<Vec<_>>();
        let Some(separator) = fields.iter().position(|field| *field == "-") else {
            continue;
        };
        let (Some(mount_point), Some(fs_type)) = (fields.get(4), fields.get(separator + 1)) else {
            continue;
        };

        let mount_point =
            mount_point.replace("\\040", " ").replace("\\011", "\t").replace("\\012", "\n").replace("\\134", "\\");
        if path.starts_with(&mount_point) && best_mount.is_none_or(|(length, _)| mount_point.len() > length) {
            best_mount = Some((mount_point.len(), *fs_type));
        }
    }

    best_mount.is_some_and(|(_, fs_type)| fs_type == "virtiofs" || fs_type == "fuse.virtiofs")
}
