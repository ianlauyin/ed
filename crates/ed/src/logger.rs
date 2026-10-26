//! Backend for the `log` facade. The level is fixed at `info`: the log is kept for error review,
//! while debugging goes through edctl automation.

use std::fmt;
use std::fs;
use std::fs::File;
use std::io;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use time::OffsetDateTime;
use time::UtcOffset;
use time::macros::format_description;

const LEVEL: log::LevelFilter = log::LevelFilter::Info;

/// Crates whose warnings are noise; only their errors are kept.
/// usvg and its CSS parser warn on every partially invalid SVG.
const ERROR_ONLY_CRATES: &[&str] = &["usvg", "simplecss"];

/// The log file is copied to the old log file and truncated past this size.
const FILE_SIZE_MAX: u64 = 1024 * 1024;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";

static STDOUT: AtomicBool = AtomicBool::new(false);
static FILE: Mutex<Option<LogFile>> = Mutex::new(None);

struct LogFile {
    file: File,
    size: u64,
    path: &'static Path,
    old_path: &'static Path,
}

struct Logger;

pub fn init() {
    if let Err(err) = log::set_logger(&Logger) {
        eprintln!("{err}");
        return;
    }
    log::set_max_level(LEVEL);
}

pub fn init_output_stdout() {
    STDOUT.store(true, Ordering::Release);
}

pub fn init_output_file(path: &'static Path, old_path: &'static Path) -> io::Result<()> {
    let size = fs::metadata(path).map_or(0, |metadata| metadata.len());
    let log_file = if size >= FILE_SIZE_MAX {
        LogFile { file: rotate(path, old_path)?, size: 0, path, old_path }
    } else {
        LogFile { file: fs::OpenOptions::new().create(true).append(true).open(path)?, size, path, old_path }
    };
    *lock_file() = Some(log_file);
    Ok(())
}

pub fn flush() {
    if STDOUT.load(Ordering::Acquire) {
        _ = io::stdout().lock().flush();
    }
    if let Some(log_file) = lock_file().as_mut()
        && let Err(err) = log_file.file.flush()
    {
        eprintln!("Failed to flush log file: {err}");
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let level = metadata.level();
        if level > LEVEL {
            return false;
        }
        let crate_name = metadata.target().split("::").next().unwrap_or_default();
        level == log::Level::Error || !ERROR_ONLY_CRATES.contains(&crate_name)
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let module_path = record.module_path().or(record.file());
        // Only `log_err` and friends pass a file path as the module path; show line numbers for those.
        let line = record.line().filter(|_| module_path.is_some_and(|path| path.ends_with(".rs")));
        let (color, label) = match record.level() {
            log::Level::Error => ("\x1b[31m", "ERROR"),
            log::Level::Warn => ("\x1b[33m", "WARN "),
            _ => ("\x1b[32m", "INFO "),
        };

        if STDOUT.load(Ordering::Acquire) {
            let source = Source { module_path, line, ansi: true };
            _ = writeln!(
                io::stdout().lock(),
                "{LocalTimestamp} {ANSI_BOLD}{color}{label}{ANSI_RESET} {source} {}",
                record.args()
            );
        }

        let mut guard = lock_file();
        let Some(log_file) = guard.as_mut() else {
            return;
        };
        let source = Source { module_path, line, ansi: false };
        let entry = format!("{LocalTimestamp} {label} {source} {}\n", record.args());
        _ = log_file.file.write_all(entry.as_bytes());
        log_file.size += entry.len() as u64;
        if log_file.size > FILE_SIZE_MAX {
            match rotate(log_file.path, log_file.old_path) {
                Ok(file) => {
                    log_file.file = file;
                    log_file.size = 0;
                }
                Err(err) => {
                    eprintln!("Failed to open log file: {err}");
                    *guard = None;
                }
            }
        }
    }

    fn flush(&self) {
        flush();
    }
}

fn lock_file() -> std::sync::MutexGuard<'static, Option<LogFile>> {
    FILE.lock().unwrap_or_else(|err| {
        FILE.clear_poison();
        err.into_inner()
    })
}

fn rotate(path: &Path, old_path: &Path) -> io::Result<File> {
    if let Err(err) = fs::copy(path, old_path) {
        eprintln!("Log file rotation failed. Truncating log file anyways: {err}");
    }
    fs::OpenOptions::new().create(true).write(true).truncate(true).open(path)
}

struct Source<'a> {
    module_path: Option<&'a str>,
    line: Option<u32>,
    ansi: bool,
}

impl fmt::Display for Source<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (bold, reset) = if self.ansi { (ANSI_BOLD, ANSI_RESET) } else { ("", "") };
        write!(f, "[{bold}{}", self.module_path.unwrap_or("?"))?;
        if let Some(line) = self.line {
            write!(f, ":{line}")?;
        }
        write!(f, "{reset}]")
    }
}

/// Current local time formatted as `YYYY-MM-DDTHH:MM:SS+HH:MM`.
struct LocalTimestamp;

impl fmt::Display for LocalTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Querying the local offset can fail once multiple threads are running, so resolve it once.
        static LOCAL_OFFSET: OnceLock<UtcOffset> = OnceLock::new();
        let offset = *LOCAL_OFFSET.get_or_init(|| UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC));
        let now = OffsetDateTime::now_utc().to_offset(offset);
        let format = format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_minute]"
        );
        let formatted = now.format(format).map_err(|_| fmt::Error)?;
        f.write_str(&formatted)
    }
}
