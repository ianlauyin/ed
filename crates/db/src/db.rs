pub mod kvp;
pub mod query;

// Re-export
pub use anyhow;
use anyhow::Context as _;
pub use gpui;
use gpui::{App, AppContext, Global};
pub use indoc::indoc;
pub use inventory;
pub use paths::database_dir;
pub use sqlez;
pub use sqlez_macros;
pub use uuid;

use sqlez::domain::Schema;
use sqlez::thread_safe_connection::ThreadSafeConnection;
use sqlez_macros::sql;
use std::fs::create_dir_all;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{LazyLock, atomic::Ordering};
use util::{ResultExt, maybe};

/// A domain schema registered via `static_connection!` and collected at link time.
pub struct DomainSchema {
    pub name: &'static str,
    pub schema: &'static str,
    pub dependencies: &'static [&'static str],
}

inventory::collect!(DomainSchema);

/// The shared database connection backing all domain-specific DB wrappers.
/// Set as a GPUI global per-App. Falls back to a shared LazyLock if not set.
pub struct AppDatabase(pub ThreadSafeConnection);

impl Global for AppDatabase {}

/// Schema made of all inventory-registered domain schemas.
pub struct AppSchema;

impl Schema for AppSchema {
    fn statements() -> Vec<&'static str> {
        // Sort by name first so the combined schema (and its hash) doesn't depend on link order.
        let mut registrations: Vec<&DomainSchema> = inventory::iter::<DomainSchema>().collect();
        registrations.sort_by_key(|reg| reg.name);
        topological_sort(&registrations)
            .into_iter()
            .map(|reg| reg.schema)
            .collect()
    }
}

impl AppDatabase {
    /// Opens the production database, creating all inventory-registered
    /// domain schemas in dependency order.
    pub fn new() -> Self {
        let db_dir = database_dir();
        let connection = gpui::block_on(open_db::<AppSchema>(db_dir, AppDbScope));
        Self(connection)
    }

    /// Returns the per-App connection if set, otherwise falls back to
    /// the shared LazyLock.
    pub fn global(cx: &App) -> &ThreadSafeConnection {
        #[allow(unreachable_code)]
        if let Some(db) = cx.try_global::<Self>() {
            return &db.0;
        } else {
            panic!("database not initialized")
        }
    }
}

fn topological_sort<'a>(registrations: &[&'a DomainSchema]) -> Vec<&'a DomainSchema> {
    let mut sorted: Vec<&DomainSchema> = Vec::new();
    let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();

    fn visit<'a>(
        name: &str,
        registrations: &[&'a DomainSchema],
        sorted: &mut Vec<&'a DomainSchema>,
        visited: &mut std::collections::HashSet<&'a str>,
    ) {
        if visited.contains(name) {
            return;
        }
        if let Some(reg) = registrations.iter().find(|r| r.name == name) {
            for dep in reg.dependencies {
                visit(dep, registrations, sorted, visited);
            }
            visited.insert(reg.name);
            sorted.push(reg);
        }
    }

    for reg in registrations {
        visit(reg.name, registrations, &mut sorted, &mut visited);
    }
    sorted
}

const CONNECTION_INITIALIZE_QUERY: &str = sql!(
    PRAGMA foreign_keys=TRUE;
);

const DB_INITIALIZE_QUERY: &str = sql!(
    PRAGMA journal_mode=WAL;
    PRAGMA busy_timeout=500;
    PRAGMA case_sensitive_like=TRUE;
    PRAGMA synchronous=NORMAL;
);

const FALLBACK_DB_NAME: &str = "FALLBACK_MEMORY_DB";

const DB_FILE_NAME: &str = "db.sqlite";

pub static ALL_FILE_DB_FAILED: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));

/// A type that can be used as a database scope for path construction.
pub trait DbScope {
    fn scope_name(&self) -> &str;
}

/// Application database scope, preserving this fork's existing data directory.
pub struct AppDbScope;

impl DbScope for AppDbScope {
    fn scope_name(&self) -> &str {
        "dev"
    }
}

/// A database scope shared across all release channels.
pub struct GlobalDbScope;

impl DbScope for GlobalDbScope {
    fn scope_name(&self) -> &str {
        "global"
    }
}

/// Returns the path to the `AppDatabase` SQLite file for the given scope
/// under `db_dir`.
pub fn db_path(db_dir: &Path, scope: impl DbScope) -> PathBuf {
    db_dir
        .join(format!("0-{}", scope.scope_name()))
        .join(DB_FILE_NAME)
}

/// Open or create a database at the given directory path.
/// This will retry a couple times if there are failures. If opening fails once, the db directory
/// is moved to a backup folder and a new one is created. If that fails, a shared in memory db is created.
/// In either case, static variables are set so that the user can be notified.
pub async fn open_db<S: Schema>(db_dir: &Path, scope: impl DbScope) -> ThreadSafeConnection {
    let db_path = db_path(db_dir, scope);

    let connection = maybe!(async {
        if let Some(parent) = db_path.parent() {
            create_dir_all(parent)
                .context("Could not create db directory")
                .log_err()?;
        }
        open_main_db::<S>(&db_path).await
    })
    .await;

    if let Some(connection) = connection {
        return connection;
    }

    // Set another static ref so that we can escalate the notification
    ALL_FILE_DB_FAILED.store(true, Ordering::Release);

    // If still failed, create an in memory db with a known name
    open_fallback_db::<S>().await
}

async fn open_main_db<S: Schema>(db_path: &Path) -> Option<ThreadSafeConnection> {
    log::trace!("Opening database {}", db_path.display());
    ThreadSafeConnection::builder::<S>(db_path.to_string_lossy().as_ref(), true)
        .with_db_initialization_query(DB_INITIALIZE_QUERY)
        .with_connection_initialize_query(CONNECTION_INITIALIZE_QUERY)
        .build()
        .await
        .log_err()
}

async fn open_fallback_db<S: Schema>() -> ThreadSafeConnection {
    log::warn!("Opening fallback in-memory database");
    ThreadSafeConnection::builder::<S>(FALLBACK_DB_NAME, false)
        .with_db_initialization_query(DB_INITIALIZE_QUERY)
        .with_connection_initialize_query(CONNECTION_INITIALIZE_QUERY)
        .build()
        .await
        .expect(
            "Fallback in memory database failed. Likely initialization queries or schemas have fundamental errors",
        )
}

/// Implements a basic DB wrapper for a given domain
///
/// Arguments:
/// - type of connection wrapper
/// - dependencies, whose schemas should be created before this domain's schema
#[macro_export]
macro_rules! static_connection {
    ($t:ident, [ $($d:ty),* ]) => {
        impl ::std::ops::Deref for $t {
            type Target = $crate::sqlez::thread_safe_connection::ThreadSafeConnection;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl ::std::clone::Clone for $t {
            fn clone(&self) -> Self {
                $t(self.0.clone())
            }
        }

        impl $t {
            /// Returns an instance backed by the per-App database if set,
            /// or the shared fallback connection otherwise.
            pub fn global(cx: &$crate::gpui::App) -> Self {
                $t($crate::AppDatabase::global(cx).clone())
            }

        }

        $crate::inventory::submit! {
            $crate::DomainSchema {
                name: <$t as $crate::sqlez::domain::Domain>::NAME,
                schema: <$t as $crate::sqlez::domain::Domain>::SCHEMA,
                dependencies: &[$(<$d as $crate::sqlez::domain::Domain>::NAME),*],
            }
        }
    }
}

pub fn write_and_log<F>(cx: &App, db_write: impl FnOnce() -> F + Send + 'static)
where
    F: Future<Output = anyhow::Result<()>> + Send,
{
    cx.background_spawn(async move { db_write().await.log_err() })
        .detach()
}
