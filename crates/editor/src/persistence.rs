use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use db::query;
use db::sqlez::bindable::Bind;
use db::sqlez::bindable::Column;
use db::sqlez::bindable::StaticColumnCount;
use db::sqlez::domain::Domain;
use db::sqlez::statement::Statement;
use db::sqlez_macros::sql;
use fs::MTime;
use itertools::Itertools as _;
use workspace::ItemId;
use workspace::WorkspaceDb;
use workspace::WorkspaceId;

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct SerializedEditor {
    pub(crate) abs_path: Option<PathBuf>,
    pub(crate) contents: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) mtime: Option<MTime>,
}

impl StaticColumnCount for SerializedEditor {
    fn column_count() -> usize {
        6
    }
}

impl Bind for SerializedEditor {
    fn bind(&self, statement: &Statement, start_index: i32) -> Result<i32> {
        let start_index = statement.bind(&self.abs_path, start_index)?;
        let start_index =
            statement.bind(&self.abs_path.as_ref().map(|p| p.to_string_lossy().into_owned()), start_index)?;
        let start_index = statement.bind(&self.contents, start_index)?;
        let start_index = statement.bind(&self.language, start_index)?;

        let start_index = match self.mtime.and_then(|mtime| mtime.to_seconds_and_nanos_for_persistence()) {
            Some((seconds, nanos)) => {
                let start_index = statement.bind(&seconds.cast_signed(), start_index)?;
                statement.bind(&nanos.cast_signed(), start_index)?
            }
            None => {
                let start_index = statement.bind::<Option<i64>>(&None, start_index)?;
                statement.bind::<Option<i32>>(&None, start_index)?
            }
        };
        Ok(start_index)
    }
}

impl Column for SerializedEditor {
    fn column(statement: &mut Statement, start_index: i32) -> Result<(Self, i32)> {
        let (abs_path, start_index): (Option<PathBuf>, i32) = Column::column(statement, start_index)?;
        let (_abs_path, start_index): (Option<PathBuf>, i32) = Column::column(statement, start_index)?;
        let (contents, start_index): (Option<String>, i32) = Column::column(statement, start_index)?;
        let (language, start_index): (Option<String>, i32) = Column::column(statement, start_index)?;
        let (mtime_seconds, start_index): (Option<i64>, i32) = Column::column(statement, start_index)?;
        let (mtime_nanos, start_index): (Option<i32>, i32) = Column::column(statement, start_index)?;

        let mtime = mtime_seconds
            .zip(mtime_nanos)
            .map(|(seconds, nanos)| MTime::from_seconds_and_nanos(seconds as u64, nanos as u32));

        let editor = Self { abs_path, contents, language, mtime };
        Ok((editor, start_index))
    }
}

pub struct EditorDb(db::sqlez::thread_safe_connection::ThreadSafeConnection);

impl Domain for EditorDb {
    const NAME: &str = stringify!(EditorDb);

    const SCHEMA: &str = sql!(
        CREATE TABLE editors(
            item_id INTEGER NOT NULL,
            workspace_id INTEGER NOT NULL,
            path BLOB,
            scroll_top_row INTEGER NOT NULL DEFAULT 0,
            scroll_horizontal_offset REAL NOT NULL DEFAULT 0,
            scroll_vertical_offset REAL NOT NULL DEFAULT 0,
            contents TEXT,
            language TEXT,
            mtime_seconds INTEGER DEFAULT NULL,
            mtime_nanos INTEGER DEFAULT NULL,
            buffer_path TEXT,
            PRIMARY KEY(item_id, workspace_id),
            FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
            ON DELETE CASCADE
            ON UPDATE CASCADE
        ) STRICT;

        CREATE TABLE editor_selections (
            item_id INTEGER NOT NULL,
            editor_id INTEGER NOT NULL,
            workspace_id INTEGER NOT NULL,
            start INTEGER NOT NULL,
            end INTEGER NOT NULL,
            PRIMARY KEY(item_id),
            FOREIGN KEY(editor_id, workspace_id) REFERENCES editors(item_id, workspace_id)
            ON DELETE CASCADE
        ) STRICT;

        CREATE INDEX editor_selections_editor_id_workspace_id
        ON editor_selections (editor_id, workspace_id);

        CREATE TABLE editor_folds (
            item_id INTEGER NOT NULL,
            editor_id INTEGER NOT NULL,
            workspace_id INTEGER NOT NULL,
            start INTEGER NOT NULL,
            end INTEGER NOT NULL,
            start_fingerprint TEXT,
            end_fingerprint TEXT,
            PRIMARY KEY(item_id),
            FOREIGN KEY(editor_id, workspace_id) REFERENCES editors(item_id, workspace_id)
            ON DELETE CASCADE
        ) STRICT;

        // Folds keyed by file path rather than editor, so they survive tab close and
        // workspace cleanup.
        CREATE TABLE file_folds (
            workspace_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            start INTEGER NOT NULL,
            end INTEGER NOT NULL,
            start_fingerprint TEXT,
            end_fingerprint TEXT,
            FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                ON DELETE CASCADE
                ON UPDATE CASCADE,
            PRIMARY KEY(workspace_id, path, start)
        );
    );
}

db::static_connection!(EditorDb, [WorkspaceDb]);

// https://www.sqlite.org/limits.html
// > <..> the maximum value of a host parameter number is SQLITE_MAX_VARIABLE_NUMBER,
// > which defaults to <..> 32766 for SQLite versions after 3.32.0.
const MAX_QUERY_PLACEHOLDERS: usize = 32000;

impl EditorDb {
    query! {
        pub fn get_serialized_editor(item_id: ItemId, workspace_id: WorkspaceId) -> Result<Option<SerializedEditor>> {
            SELECT path, buffer_path, contents, language, mtime_seconds, mtime_nanos FROM editors
            WHERE item_id = ? AND workspace_id = ?
        }
    }

    query! {
        pub async fn save_serialized_editor(item_id: ItemId, workspace_id: WorkspaceId, serialized_editor: SerializedEditor) -> Result<()> {
            INSERT INTO editors
                (item_id, workspace_id, path, buffer_path, contents, language, mtime_seconds, mtime_nanos)
            VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT DO UPDATE SET
                item_id = ?1,
                workspace_id = ?2,
                path = ?3,
                buffer_path = ?4,
                contents = ?5,
                language = ?6,
                mtime_seconds = ?7,
                mtime_nanos = ?8
        }
    }

    // Returns the scroll top row, and offset
    query! {
        pub fn get_scroll_position(item_id: ItemId, workspace_id: WorkspaceId) -> Result<Option<(u32, f64, f64)>> {
            SELECT scroll_top_row, scroll_horizontal_offset, scroll_vertical_offset
            FROM editors
            WHERE item_id = ? AND workspace_id = ?
        }
    }

    query! {
        pub async fn save_scroll_position(
            item_id: ItemId,
            workspace_id: WorkspaceId,
            top_row: u32,
            vertical_offset: f64,
            horizontal_offset: f64
        ) -> Result<()> {
            UPDATE OR IGNORE editors
            SET
                scroll_top_row = ?3,
                scroll_horizontal_offset = ?4,
                scroll_vertical_offset = ?5
            WHERE item_id = ?1 AND workspace_id = ?2
        }
    }

    query! {
        pub fn get_editor_selections(
            editor_id: ItemId,
            workspace_id: WorkspaceId
        ) -> Result<Vec<(usize, usize)>> {
            SELECT start, end
            FROM editor_selections
            WHERE editor_id = ?1 AND workspace_id = ?2
            ORDER BY item_id
        }
    }

    query! {
        pub fn get_editor_folds(
            editor_id: ItemId,
            workspace_id: WorkspaceId
        ) -> Result<Vec<(usize, usize, Option<String>, Option<String>)>> {
            SELECT start, end, start_fingerprint, end_fingerprint
            FROM editor_folds
            WHERE editor_id = ?1 AND workspace_id = ?2
        }
    }

    query! {
        pub fn get_file_folds(
            workspace_id: WorkspaceId,
            path: &Path
        ) -> Result<Vec<(usize, usize, Option<String>, Option<String>)>> {
            SELECT start, end, start_fingerprint, end_fingerprint
            FROM file_folds
            WHERE workspace_id = ?1 AND path = ?2
            ORDER BY start
        }
    }

    pub async fn save_editor_selections(
        &self,
        editor_id: ItemId,
        workspace_id: WorkspaceId,
        selections: Vec<(usize, usize)>,
    ) -> Result<()> {
        log::debug!("Saving selections for editor {editor_id} in workspace {workspace_id:?}");
        let mut first_selection;
        let mut last_selection = 0_usize;
        for (count, placeholders) in std::iter::once("(?1, ?2, ?, ?)")
            .cycle()
            .take(selections.len())
            .chunks(MAX_QUERY_PLACEHOLDERS / 4)
            .into_iter()
            .map(|chunk| {
                let mut count = 0;
                let placeholders = chunk
                    .inspect(|_| {
                        count += 1;
                    })
                    .join(", ");
                (count, placeholders)
            })
            .collect::<Vec<_>>()
        {
            first_selection = last_selection;
            last_selection += count;
            let query = format!(
                r#"
DELETE FROM editor_selections WHERE editor_id = ?1 AND workspace_id = ?2;

INSERT OR IGNORE INTO editor_selections (editor_id, workspace_id, start, end)
VALUES {placeholders};
"#
            );

            let selections = selections[first_selection..last_selection].to_vec();
            self.write(move |conn| {
                let mut statement = Statement::prepare(conn, query)?;
                statement.bind(&editor_id, 1)?;
                let mut next_index = statement.bind(&workspace_id, 2)?;
                for (start, end) in selections {
                    next_index = statement.bind(&start, next_index)?;
                    next_index = statement.bind(&end, next_index)?;
                }
                statement.exec()
            })
            .await?;
        }
        Ok(())
    }

    pub async fn save_file_folds(
        &self,
        workspace_id: WorkspaceId,
        path: Arc<Path>,
        folds: Vec<(usize, usize, String, String)>,
    ) -> Result<()> {
        log::debug!("Saving folds for file {path:?} in workspace {workspace_id:?}");
        self.write(move |conn| {
            // Clear existing folds for this file
            conn.exec_bound(sql!(
                DELETE FROM file_folds WHERE workspace_id = ?1 AND path = ?2;
            ))?((workspace_id, path.as_ref()))?;

            // Insert each fold
            for (start, end, start_fp, end_fp) in folds {
                conn.exec_bound(sql!(
                    INSERT INTO file_folds (workspace_id, path, start, end, start_fingerprint, end_fingerprint)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6);
                ))?((workspace_id, path.as_ref(), start, end, start_fp, end_fp))?;
            }
            Ok(())
        })
        .await
    }

    pub async fn delete_file_folds(&self, workspace_id: WorkspaceId, path: Arc<Path>) -> Result<()> {
        self.write(move |conn| {
            conn.exec_bound(sql!(
                DELETE FROM file_folds WHERE workspace_id = ?1 AND path = ?2;
            ))?((workspace_id, path.as_ref()))
        })
        .await
    }
}
