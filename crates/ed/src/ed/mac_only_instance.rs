use std::fs::File;
use std::fs::TryLockError;
use std::fs::{self};
use std::sync::OnceLock;

/// Held for the lifetime of the process; the OS releases the lock on exit,
/// so a crash never leaves a stale lock behind.
static INSTANCE_LOCK: OnceLock<File> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsOnlyInstance {
    Yes,
    No,
}

/// Takes an exclusive `flock` on `<state_dir>/ed.lock`. The lock lives under
/// this app's own XDG state directory, so it is per user and never collides
/// with an upstream ed instance.
pub fn ensure_only_instance() -> IsOnlyInstance {
    let path = paths::state_dir().join(format!("{}.lock", paths::APP_NAME));
    let file = match fs::create_dir_all(paths::state_dir())
        .and_then(|()| File::options().create(true).truncate(false).write(true).open(&path))
    {
        Ok(file) => file,
        Err(err) => {
            // Avoid failing to start when the lock file is unusable; a second
            // instance may then start alongside this one.
            log::warn!("Error opening single instance lock {path:?}: {err}");
            return IsOnlyInstance::Yes;
        }
    };

    match file.try_lock() {
        Ok(()) => {
            INSTANCE_LOCK.set(file).ok();
            IsOnlyInstance::Yes
        }
        Err(TryLockError::WouldBlock) => IsOnlyInstance::No,
        Err(TryLockError::Error(err)) => {
            log::warn!("Error locking single instance lock {path:?}: {err}");
            IsOnlyInstance::Yes
        }
    }
}
