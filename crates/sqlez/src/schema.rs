// Schemas are not migrated. The combined schema text of all domains is hashed into
// `PRAGMA user_version`; when it no longer matches, every table is dropped and the
// schema is created from scratch.

use std::ffi::CString;

use anyhow::Context as _;
use anyhow::Result;
use libsqlite3_sys::sqlite3_exec;

use crate::connection::Connection;

impl Connection {
    fn eager_exec(&self, sql: &str) -> anyhow::Result<()> {
        let sql_str = CString::new(sql).context("Error creating cstr")?;
        unsafe {
            sqlite3_exec(self.sqlite3, sql_str.as_c_str().as_ptr(), None, std::ptr::null_mut(), std::ptr::null_mut());
        }
        self.last_error().with_context(|| format!("Prepare call failed for query:\n{}", sql))?;

        Ok(())
    }

    /// Create the given schema, wiping the database first if it was created from a
    /// different schema.
    /// Note: Unlike everything else in SQLez, schema statements are run eagerly, without first
    /// preparing them. This makes it possible to create several tables in a single string.
    pub fn ensure_schema(&self, schema: &[&str]) -> Result<()> {
        let version = schema_version(schema);
        let stored_version = self.select_row::<i32>("PRAGMA user_version")?()?.unwrap_or(0);
        if stored_version == version {
            return Ok(());
        }

        self.with_savepoint("schema", || {
            if stored_version != 0 {
                log::info!("Database schema changed, discarding existing data");
            }
            self.drop_all()?;
            for statements in schema {
                self.eager_exec(statements)?;
            }
            self.exec(&format!("PRAGMA user_version = {version}"))?()
        })
    }

    fn drop_all(&self) -> Result<()> {
        let objects: Vec<(String, String)> = self.select(
            "SELECT type, name FROM sqlite_schema
             WHERE type IN ('view', 'trigger', 'table') AND name NOT LIKE 'sqlite_%'
             ORDER BY type = 'table'",
        )?()?;
        for (kind, name) in objects {
            self.exec(&format!("DROP {kind} IF EXISTS \"{name}\""))?()?;
        }
        Ok(())
    }
}

/// Stable, non-zero hash of the schema text (`0` is the version of an empty database).
fn schema_version(schema: &[&str]) -> i32 {
    let mut hash: u32 = 0x811c9dc5;
    for statements in schema {
        for byte in statements.bytes().chain([0]) {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(0x01000193);
        }
    }
    match hash.cast_signed() {
        0 => 1,
        version => version,
    }
}
