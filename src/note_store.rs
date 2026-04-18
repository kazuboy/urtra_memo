use crate::model::{derive_title, Note, NoteSummary, SearchResult, SortOrder};
use crate::paths::AppPaths;
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

const SNIPPET_MAX_CHARS: usize = 120;

/// SQLite-backed note repository with markdown file bodies.
pub struct NoteStore {
    paths: AppPaths,
    conn: Connection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExportBundle {
    version: u32,
    exported_at: String,
    notes: Vec<ExportNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExportNote {
    id: String,
    file_name: String,
    title: String,
    body: String,
    #[serde(default)]
    tags: Vec<String>,
    created_at: i64,
    updated_at: i64,
    deleted: bool,
    is_daily: bool,
}

#[derive(Debug, Clone)]
/// Summary of import operation results.
pub struct ImportSummary {
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
}

impl NoteStore {
    /// Opens store at `paths`, creates schema/directories as needed.
    pub fn open(paths: AppPaths) -> Result<Self> {
        paths.ensure_dirs()?;
        let conn = Connection::open(&paths.db_path).with_context(|| {
            format!(
                "failed to open sqlite database: {}",
                paths.db_path.display()
            )
        })?;
        let mut store = Self { paths, conn };
        store.migrate()?;
        Ok(store)
    }

    /// Returns resolved storage paths used by this store.
    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn create_note(&mut self, body: &str) -> Result<Note> {
        let id = Uuid::new_v4().to_string();
        let file_name = format!("{id}.md");
        self.create_note_with_file_name(id, file_name, body, false)
    }

    pub fn create_or_open_daily_note(&mut self, date: NaiveDate) -> Result<Note> {
        let file_name = format!("{}.md", date.format("%Y-%m-%d"));
        let existing: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT id, deleted FROM notes WHERE file_name = ?1 LIMIT 1",
                params![file_name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        if let Some((id, deleted)) = existing {
            if deleted != 0 {
                self.restore_note(&id)?;
            }
            return self.load_note(&id);
        }

        let body = format!("# {}\n", date.format("%Y-%m-%d"));
        let id = Uuid::new_v4().to_string();
        self.create_note_with_file_name(id, file_name, &body, true)
    }

    pub fn load_note(&mut self, id: &str) -> Result<Note> {
        let meta = self.fetch_meta(id)?;
        let body = self.read_note_body(&meta.file_name)?;
        self.touch_recent(&meta.id)?;
        Ok(Note {
            id: meta.id,
            title: meta.title,
            body,
            tags: decode_tags(&meta.tags),
            created_at: millis_to_utc(meta.created_at)?,
            updated_at: millis_to_utc(meta.updated_at)?,
            deleted: meta.deleted != 0,
        })
    }

    pub fn update_note(&mut self, id: &str, body: &str) -> Result<Note> {
        let meta = self.fetch_meta(id)?;
        if meta.deleted != 0 {
            bail!("note is deleted and cannot be edited: {id}");
        }
        let note_path = self.paths.note_path(&meta.file_name);
        // Atomic write with backup/restore to minimize loss on crash or IO failure.
        let temp_path = note_path.with_extension("tmp");
        let backup_path = note_path.with_extension("bak");
        fs::write(&temp_path, body)
            .with_context(|| format!("failed to write temp note body: {}", temp_path.display()))?;
        if note_path.exists() {
            let _ = fs::remove_file(&backup_path);
            fs::rename(&note_path, &backup_path).with_context(|| {
                format!(
                    "failed to move note to backup before replace: {}",
                    note_path.display()
                )
            })?;
        }
        match fs::rename(&temp_path, &note_path) {
            Ok(_) => {
                let _ = fs::remove_file(&backup_path);
            }
            Err(err) => {
                if backup_path.exists() {
                    let _ = fs::rename(&backup_path, &note_path);
                }
                let _ = fs::remove_file(&temp_path);
                return Err(err).with_context(|| {
                    format!("failed to replace note body atomically: {}", note_path.display())
                });
            }
        }

        let now = now_millis();
        let title = derive_title(body);
        let snippet = normalize_snippet(body, SNIPPET_MAX_CHARS);
        self.conn.execute(
            "UPDATE notes SET title = ?1, snippet = ?2, updated_at = ?3 WHERE id = ?4 AND deleted = 0",
            params![title, snippet, now, id],
        )?;
        self.upsert_fts(id, &title, body, now, false)?;
        self.touch_recent(id)?;
        Ok(Note {
            id: id.to_string(),
            title,
            body: body.to_string(),
            tags: decode_tags(&meta.tags),
            created_at: millis_to_utc(meta.created_at)?,
            updated_at: millis_to_utc(now)?,
            deleted: false,
        })
    }

    pub fn update_note_tags(&mut self, id: &str, raw_tags: &str) -> Result<()> {
        let meta = self.fetch_meta(id)?;
        if meta.deleted != 0 {
            bail!("note is deleted and cannot be tagged: {id}");
        }
        let tags = normalize_tags(raw_tags);
        let encoded = encode_tags(&tags);
        let now = now_millis();
        self.conn.execute(
            "UPDATE notes SET tags = ?1, updated_at = ?2 WHERE id = ?3 AND deleted = 0",
            params![encoded, now, id],
        )?;
        let body = self.read_note_body(&meta.file_name)?;
        self.upsert_fts(id, &meta.title, &body, now, false)?;
        self.touch_recent(id)?;
        Ok(())
    }

    pub fn list_notes(
        &self,
        sort_order: SortOrder,
        limit: usize,
        include_deleted: bool,
    ) -> Result<Vec<NoteSummary>> {
        let sql = match (include_deleted, sort_order) {
            (false, SortOrder::UpdatedDesc) => {
                "SELECT id, title, updated_at, snippet, tags
                 FROM notes
                 WHERE deleted = 0
                 ORDER BY updated_at DESC
                 LIMIT ?1"
            }
            (false, SortOrder::CreatedDesc) => {
                "SELECT id, title, updated_at, snippet, tags
                 FROM notes
                 WHERE deleted = 0
                 ORDER BY created_at DESC
                 LIMIT ?1"
            }
            (true, SortOrder::UpdatedDesc) => {
                "SELECT id, title, updated_at, snippet, tags
                 FROM notes
                 ORDER BY updated_at DESC
                 LIMIT ?1"
            }
            (true, SortOrder::CreatedDesc) => {
                "SELECT id, title, updated_at, snippet, tags
                 FROM notes
                 ORDER BY created_at DESC
                 LIMIT ?1"
            }
        };
        let mut stmt = self.conn.prepare_cached(sql)?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut notes = Vec::new();
        for row in rows {
            let (id, title, updated_at, snippet, tags) = row?;
            notes.push(NoteSummary {
                id,
                title,
                updated_at: millis_to_utc(updated_at)?,
                snippet,
                tags: decode_tags(&tags),
            });
        }
        Ok(notes)
    }

    pub fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let tag_terms = parse_tag_query_terms(query);
        if !tag_terms.is_empty() {
            return self.search_notes_by_tags(&tag_terms, limit);
        }
        let fts_query = build_fts_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        match self.search_notes_with_fts(&fts_query, limit) {
            Ok(results) => Ok(results),
            Err(err) => {
                if is_fts_fallback_error(&err) {
                    self.search_notes_with_like(query, limit)
                } else {
                    Err(err)
                }
            }
        }
    }

    pub fn soft_delete_note(&mut self, id: &str) -> Result<()> {
        let now = now_millis();
        let affected = self.conn.execute(
            "UPDATE notes SET deleted = 1, updated_at = ?1 WHERE id = ?2 AND deleted = 0",
            params![now, id],
        )?;
        if affected == 0 {
            bail!("note not found or already deleted: {id}");
        }
        self.sync_fts_deleted_flag(id, now, true)?;
        self.conn
            .execute("DELETE FROM recent_notes WHERE note_id = ?1", params![id])?;
        Ok(())
    }

    pub fn restore_note(&mut self, id: &str) -> Result<()> {
        let now = now_millis();
        let affected = self.conn.execute(
            "UPDATE notes SET deleted = 0, updated_at = ?1 WHERE id = ?2 AND deleted = 1",
            params![now, id],
        )?;
        if affected == 0 {
            bail!("note is not in deleted state: {id}");
        }
        let meta = self.fetch_meta(id)?;
        let body = self.read_note_body(&meta.file_name)?;
        self.upsert_fts(id, &meta.title, &body, now, false)?;
        Ok(())
    }

    pub fn purge_note(&mut self, id: &str) -> Result<()> {
        let meta = self.fetch_meta(id)?;
        if meta.deleted == 0 {
            bail!("note must be in deleted state to purge: {id}");
        }

        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM note_fts WHERE id = ?1", params![id])?;
        tx.execute("DELETE FROM recent_notes WHERE note_id = ?1", params![id])?;
        tx.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
        tx.commit()?;

        let note_path = self.paths.note_path(&meta.file_name);
        if note_path.exists() {
            fs::remove_file(&note_path).with_context(|| {
                format!("failed to remove purged note file: {}", note_path.display())
            })?;
        }
        Ok(())
    }

    pub fn purge_deleted_notes(&mut self) -> Result<usize> {
        let mut deleted = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT id, file_name FROM notes WHERE deleted = 1")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                deleted.push(row?);
            }
        }
        if deleted.is_empty() {
            return Ok(0);
        }

        let tx = self.conn.transaction()?;
        {
            let mut delete_fts = tx.prepare("DELETE FROM note_fts WHERE id = ?1")?;
            let mut delete_recent = tx.prepare("DELETE FROM recent_notes WHERE note_id = ?1")?;
            for (id, _) in &deleted {
                delete_fts.execute(params![id])?;
                delete_recent.execute(params![id])?;
            }
        }
        tx.execute("DELETE FROM notes WHERE deleted = 1", [])?;
        tx.commit()?;

        for (_, file_name) in &deleted {
            let note_path = self.paths.note_path(file_name);
            if note_path.exists() {
                fs::remove_file(&note_path).with_context(|| {
                    format!(
                        "failed to remove purged note file during bulk purge: {}",
                        note_path.display()
                    )
                })?;
            }
        }
        Ok(deleted.len())
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<NoteSummary>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT n.id, n.title, n.updated_at, n.snippet, n.tags
             FROM recent_notes r
             JOIN notes n ON n.id = r.note_id
             WHERE n.deleted = 0
             ORDER BY r.opened_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut recent = Vec::new();
        for row in rows {
            let (id, title, updated_at, snippet, tags) = row?;
            recent.push(NoteSummary {
                id,
                title,
                updated_at: millis_to_utc(updated_at)?,
                snippet,
                tags: decode_tags(&tags),
            });
        }
        Ok(recent)
    }

    pub fn list_deleted_notes(&self, limit: usize) -> Result<Vec<NoteSummary>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, title, updated_at, snippet, tags
             FROM notes
             WHERE deleted = 1
             ORDER BY updated_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, title, updated_at, snippet, tags) = row?;
            out.push(NoteSummary {
                id,
                title,
                updated_at: millis_to_utc(updated_at)?,
                snippet,
                tags: decode_tags(&tags),
            });
        }
        Ok(out)
    }

    pub fn list_deleted_note_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM notes WHERE deleted = 1")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn rebuild_index(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM note_fts", [])?;

        let mut stmt = tx.prepare("SELECT id, title, file_name, updated_at, deleted FROM notes")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        drop(stmt);

        for (id, title, file_name, updated_at, deleted) in records {
            let body = fs::read_to_string(self.paths.note_path(&file_name)).unwrap_or_default();
            let snippet = normalize_snippet(&body, SNIPPET_MAX_CHARS);
            tx.execute(
                "INSERT INTO note_fts (id, title, body, preview, updated_at, deleted) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, title, body, snippet, updated_at, deleted],
            )?;
            tx.execute(
                "UPDATE notes SET snippet = ?1 WHERE id = ?2",
                params![snippet, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn export_to_json(&self, path: &Path) -> Result<usize> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_name, title, tags, created_at, updated_at, deleted, is_daily
             FROM notes
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;

        let mut notes = Vec::new();
        for row in rows {
            let (id, file_name, title, tags, created_at, updated_at, deleted, is_daily) = row?;
            let body = fs::read_to_string(self.paths.note_path(&file_name)).unwrap_or_default();
            notes.push(ExportNote {
                id,
                file_name,
                title,
                body,
                tags: decode_tags(&tags),
                created_at,
                updated_at,
                deleted: deleted != 0,
                is_daily: is_daily != 0,
            });
        }

        let bundle = ExportBundle {
            version: 1,
            exported_at: Utc::now().to_rfc3339(),
            notes,
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&bundle)?;
        let tmp_path = path.with_extension("tmp");
        fs::write(&tmp_path, json)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(tmp_path, path)?;
        Ok(bundle.notes.len())
    }

    pub fn export_to_path(&self, path: &Path, note_id: Option<&str>) -> Result<usize> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if ext == "json" {
            return self.export_to_json(path);
        }
        if is_supported_memo_text_extension(&ext) {
            let id = note_id.ok_or_else(|| {
                anyhow!(
                    "text export requires a target note (open/select a memo first, or export as json)"
                )
            })?;
            self.export_single_note_to_text(path, id)?;
            return Ok(1);
        }
        bail!(
            "unsupported export extension: .{} (supported: json, md, markdown, txt, text, log, rst, adoc, org)",
            ext
        );
    }

    pub fn import_from_path(&mut self, path: &Path) -> Result<ImportSummary> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if ext == "json" {
            return self.import_from_json(path);
        }
        if is_supported_memo_text_extension(&ext) {
            return self.import_from_single_file(path);
        }
        bail!(
            "unsupported import extension: .{} (supported: json, md, markdown, txt, text, log, rst, adoc, org)",
            ext
        );
    }

    pub fn import_from_json(&mut self, path: &Path) -> Result<ImportSummary> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read import file: {}", path.display()))?;
        let bundle: ExportBundle = serde_json::from_str(&text)
            .with_context(|| format!("failed to parse import json: {}", path.display()))?;
        if bundle.version != 1 {
            bail!("unsupported import version: {}", bundle.version);
        }

        let tx = self.conn.transaction()?;
        let mut created = 0usize;
        let mut updated = 0usize;
        let mut skipped = 0usize;

        for note in bundle.notes {
            let file_name_conflict: Option<String> = tx
                .query_row(
                    "SELECT id FROM notes WHERE file_name = ?1 AND id <> ?2 LIMIT 1",
                    params![note.file_name, note.id],
                    |row| row.get(0),
                )
                .optional()?;
            if file_name_conflict.is_some() {
                skipped += 1;
                continue;
            }

            let existed: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM notes WHERE id = ?1 LIMIT 1",
                    params![note.id],
                    |row| row.get(0),
                )
                .optional()?;
            if existed.is_some() {
                updated += 1;
            } else {
                created += 1;
            }

            tx.execute(
                "INSERT INTO notes (
                    id, file_name, title, snippet, tags, created_at, updated_at, deleted, is_daily
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                    file_name = excluded.file_name,
                    title = excluded.title,
                    snippet = excluded.snippet,
                    tags = excluded.tags,
                    created_at = excluded.created_at,
                    updated_at = excluded.updated_at,
                    deleted = excluded.deleted,
                    is_daily = excluded.is_daily",
                params![
                    note.id,
                    note.file_name,
                    note.title,
                    normalize_snippet(&note.body, SNIPPET_MAX_CHARS),
                    encode_tags(&note.tags),
                    note.created_at,
                    note.updated_at,
                    bool_to_int(note.deleted),
                    bool_to_int(note.is_daily),
                ],
            )?;
            tx.execute("DELETE FROM note_fts WHERE id = ?1", params![note.id])?;
            tx.execute(
                "INSERT INTO note_fts (id, title, body, preview, updated_at, deleted)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    note.id,
                    note.title,
                    note.body,
                    normalize_snippet(&note.body, SNIPPET_MAX_CHARS),
                    note.updated_at,
                    bool_to_int(note.deleted),
                ],
            )?;

            let note_path = self.paths.note_path(&note.file_name);
            fs::write(&note_path, note.body).with_context(|| {
                format!("failed to write imported note: {}", note_path.display())
            })?;
        }

        tx.commit()?;
        Ok(ImportSummary {
            created,
            updated,
            skipped,
        })
    }

    fn import_from_single_file(&mut self, path: &Path) -> Result<ImportSummary> {
        let body = fs::read_to_string(path)
            .with_context(|| format!("failed to read import file: {}", path.display()))?;
        let _ = self.create_note(&body)?;
        Ok(ImportSummary {
            created: 1,
            updated: 0,
            skipped: 0,
        })
    }

    fn export_single_note_to_text(&self, path: &Path, note_id: &str) -> Result<()> {
        let meta = self.fetch_meta(note_id)?;
        let body = self.read_note_body(&meta.file_name)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp_path = path.with_extension("tmp");
        fs::write(&tmp_path, body)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(tmp_path, path)?;
        Ok(())
    }

    fn create_note_with_file_name(
        &mut self,
        id: String,
        file_name: String,
        body: &str,
        is_daily: bool,
    ) -> Result<Note> {
        let note_path = self.paths.note_path(&file_name);
        fs::write(&note_path, body)
            .with_context(|| format!("failed to write note file: {}", note_path.display()))?;

        let now = now_millis();
        let title = derive_title(body);
        let snippet = normalize_snippet(body, SNIPPET_MAX_CHARS);
        self.conn.execute(
            "INSERT INTO notes (
                id, file_name, title, snippet, tags, created_at, updated_at, deleted, is_daily
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)",
            params![
                id,
                file_name,
                title,
                snippet,
                "",
                now,
                now,
                bool_to_int(is_daily)
            ],
        )?;
        self.upsert_fts(&id, &title, body, now, false)?;
        self.touch_recent(&id)?;
        Ok(Note {
            id,
            title,
            body: body.to_string(),
            tags: Vec::new(),
            created_at: millis_to_utc(now)?,
            updated_at: millis_to_utc(now)?,
            deleted: false,
        })
    }

    fn search_notes_with_fts(&self, fts_query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT f.id, f.title, f.updated_at, f.preview, n.tags
             FROM note_fts f
             JOIN notes n ON n.id = f.id
             WHERE note_fts MATCH ?1
               AND f.deleted = 0
             ORDER BY f.updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, title, updated_at, snippet, tags) = row?;
            out.push(SearchResult {
                id,
                title,
                updated_at: millis_to_utc(updated_at)?,
                snippet,
                tags: decode_tags(&tags),
            });
        }
        Ok(out)
    }

    fn search_notes_by_tags(&self, tags: &[String], limit: usize) -> Result<Vec<SearchResult>> {
        if tags.is_empty() {
            return Ok(Vec::new());
        }
        let mut sql = String::from(
            "SELECT id, title, updated_at, snippet, tags
             FROM notes
             WHERE deleted = 0",
        );
        let mut params_owned: Vec<rusqlite::types::Value> = Vec::with_capacity(tags.len() + 1);
        for tag in tags {
            sql.push_str(" AND lower(tags) LIKE ?");
            params_owned.push(rusqlite::types::Value::Text(format!(
                "%|{}|%",
                tag.to_lowercase()
            )));
        }
        sql.push_str(" ORDER BY updated_at DESC LIMIT ?");
        params_owned.push(rusqlite::types::Value::Integer(limit as i64));

        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params_owned), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, title, updated_at, snippet, encoded_tags) = row?;
            out.push(SearchResult {
                id,
                title,
                updated_at: millis_to_utc(updated_at)?,
                snippet,
                tags: decode_tags(&encoded_tags),
            });
        }
        Ok(out)
    }

    fn search_notes_with_like(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let like = format!("%{}%", escape_like_pattern(query));
        let terms = query_terms(query);
        let mut stmt = self.conn.prepare_cached(
            "SELECT f.id, f.title, f.updated_at, f.body, n.tags
             FROM note_fts f
             JOIN notes n ON n.id = f.id
             WHERE (f.title LIKE ?1 ESCAPE '^' OR f.body LIKE ?1 ESCAPE '^')
               AND f.deleted = 0
             ORDER BY f.updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![like, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, title, updated_at, body, tags) = row?;
            out.push(SearchResult {
                id,
                title,
                updated_at: millis_to_utc(updated_at)?,
                snippet: build_search_snippet(&body, &terms, 120),
                tags: decode_tags(&tags),
            });
        }
        Ok(out)
    }

    fn upsert_fts(
        &self,
        id: &str,
        title: &str,
        body: &str,
        updated_at: i64,
        deleted: bool,
    ) -> Result<()> {
        let preview = normalize_snippet(body, SNIPPET_MAX_CHARS);
        self.conn
            .execute("DELETE FROM note_fts WHERE id = ?1", params![id])?;
        self.conn.execute(
            "INSERT INTO note_fts (id, title, body, preview, updated_at, deleted) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, title, body, preview, updated_at, bool_to_int(deleted)],
        )?;
        Ok(())
    }

    fn sync_fts_deleted_flag(&self, id: &str, updated_at: i64, deleted: bool) -> Result<()> {
        let affected = self.conn.execute(
            "UPDATE note_fts SET updated_at = ?1, deleted = ?2 WHERE id = ?3",
            params![updated_at, bool_to_int(deleted), id],
        )?;
        if affected > 0 {
            return Ok(());
        }
        let meta = self.fetch_meta(id)?;
        let body = self.read_note_body(&meta.file_name)?;
        self.upsert_fts(id, &meta.title, &body, updated_at, deleted)?;
        Ok(())
    }

    fn touch_recent(&self, note_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO recent_notes (note_id, opened_at)
             VALUES (?1, ?2)
             ON CONFLICT(note_id) DO UPDATE SET opened_at = excluded.opened_at",
            params![note_id, now_millis()],
        )?;
        Ok(())
    }

    fn fetch_meta(&self, id: &str) -> Result<MetaRow> {
        let row = self
            .conn
            .prepare_cached(
                "SELECT id, file_name, title, tags, created_at, updated_at, deleted
                 FROM notes
                 WHERE id = ?1",
            )?
            .query_row(params![id], |row| {
                Ok(MetaRow {
                    id: row.get(0)?,
                    file_name: row.get(1)?,
                    title: row.get(2)?,
                    tags: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    deleted: row.get(6)?,
                })
            })
            .optional()?;
        row.ok_or_else(|| anyhow!("note not found: {id}"))
    }

    fn read_note_body(&self, file_name: &str) -> Result<String> {
        let note_path = self.paths.note_path(file_name);
        let body = fs::read_to_string(&note_path)
            .with_context(|| format!("failed to read note body: {}", note_path.display()))?;
        Ok(body)
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA temp_store=MEMORY;
            PRAGMA cache_size=-20000;
            CREATE TABLE IF NOT EXISTS notes (
                id TEXT PRIMARY KEY,
                file_name TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                snippet TEXT NOT NULL DEFAULT '',
                tags TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                deleted INTEGER NOT NULL DEFAULT 0,
                is_daily INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_notes_updated_at ON notes(updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_notes_created_at ON notes(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_notes_deleted ON notes(deleted);

            CREATE VIRTUAL TABLE IF NOT EXISTS note_fts USING fts5(
                id UNINDEXED,
                title,
                body,
                preview UNINDEXED,
                updated_at UNINDEXED,
                deleted UNINDEXED
            );

            CREATE TABLE IF NOT EXISTS recent_notes (
                note_id TEXT PRIMARY KEY,
                opened_at INTEGER NOT NULL,
                FOREIGN KEY(note_id) REFERENCES notes(id)
            );
            CREATE INDEX IF NOT EXISTS idx_recent_opened_at ON recent_notes(opened_at DESC);
            ",
        )?;

        let snippet_column_added = match self.conn.execute(
            "ALTER TABLE notes ADD COLUMN snippet TEXT NOT NULL DEFAULT ''",
            [],
        ) {
            Ok(_) => true,
            Err(err) => {
                let msg = err.to_string().to_lowercase();
                if msg.contains("duplicate column name") {
                    false
                } else {
                    return Err(err.into());
                }
            }
        };
        if snippet_column_added {
            self.backfill_missing_snippets()?;
        }
        if let Err(err) = self.conn.execute(
            "ALTER TABLE notes ADD COLUMN tags TEXT NOT NULL DEFAULT ''",
            [],
        ) {
            let msg = err.to_string().to_lowercase();
            if !msg.contains("duplicate column name") {
                return Err(err.into());
            }
        }
        self.ensure_note_fts_schema()?;
        Ok(())
    }

    fn ensure_note_fts_schema(&mut self) -> Result<()> {
        let has_extended_columns = self
            .conn
            .prepare("SELECT preview, updated_at, deleted FROM note_fts LIMIT 0")
            .is_ok();
        if has_extended_columns {
            return Ok(());
        }
        self.conn.execute("DROP TABLE IF EXISTS note_fts", [])?;
        self.conn.execute_batch(
            "CREATE VIRTUAL TABLE note_fts USING fts5(
                id UNINDEXED,
                title,
                body,
                preview UNINDEXED,
                updated_at UNINDEXED,
                deleted UNINDEXED
            );",
        )?;
        self.rebuild_index()?;
        Ok(())
    }

    fn backfill_missing_snippets(&self) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_name
             FROM notes
             WHERE COALESCE(snippet, '') = ''",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut to_update = Vec::new();
        for row in rows {
            to_update.push(row?);
        }

        for (id, file_name) in to_update {
            let body = fs::read_to_string(self.paths.note_path(&file_name)).unwrap_or_default();
            let snippet = normalize_snippet(&body, SNIPPET_MAX_CHARS);
            self.conn.execute(
                "UPDATE notes SET snippet = ?1 WHERE id = ?2",
                params![snippet, id],
            )?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct MetaRow {
    id: String,
    file_name: String,
    title: String,
    tags: String,
    created_at: i64,
    updated_at: i64,
    deleted: i64,
}

fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}

fn millis_to_utc(value: i64) -> Result<DateTime<Utc>> {
    Utc.timestamp_millis_opt(value)
        .single()
        .ok_or_else(|| anyhow!("invalid timestamp millis: {value}"))
}

fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn parse_tag_query_terms(query: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for token in query.split_whitespace() {
        if let Some(raw) = token.strip_prefix('#') {
            let normalized = normalize_single_tag(raw);
            if !normalized.is_empty() {
                tags.push(normalized);
            }
        }
    }
    tags
}

fn normalize_tags(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in raw
        .replace(',', " ")
        .split_whitespace()
        .map(|t| t.trim_start_matches('#'))
    {
        let normalized = normalize_single_tag(token);
        if !normalized.is_empty() && !out.iter().any(|v| v == &normalized) {
            out.push(normalized);
        }
    }
    out
}

fn normalize_single_tag(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c: char| c == '#' || c == '|' || c == ',' || c == ';')
        .to_lowercase()
}

fn encode_tags(tags: &[String]) -> String {
    if tags.is_empty() {
        return String::new();
    }
    let mut out = String::from("|");
    for tag in tags {
        if !tag.is_empty() {
            out.push_str(tag);
            out.push('|');
        }
    }
    out
}

fn decode_tags(encoded: &str) -> Vec<String> {
    encoded
        .split('|')
        .filter_map(|v| {
            let t = v.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
        .collect()
}

fn build_fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .filter(|token| !token.trim().is_empty())
        .map(|token| format!("\"{}\"*", token.replace('\"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn is_fts_fallback_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("fts5")
        || msg.contains("malformed")
        || msg.contains("syntax error")
        || msg.contains("parse error")
}

fn is_supported_memo_text_extension(ext: &str) -> bool {
    matches!(
        ext,
        "md" | "markdown" | "txt" | "text" | "log" | "rst" | "adoc" | "org"
    )
}

fn normalize_snippet(body: &str, max_chars: usize) -> String {
    let compact = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    if compact.is_empty() {
        return "(empty)".to_string();
    }
    let mut chars = compact.chars();
    let mut snippet: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        snippet.push_str("...");
    }
    snippet
}

fn escape_like_pattern(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len());
    for ch in query.chars() {
        match ch {
            '^' => escaped.push_str("^^"),
            '%' => escaped.push_str("^%"),
            '_' => escaped.push_str("^_"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn build_search_snippet(body: &str, terms: &[String], max_chars: usize) -> String {
    let compact = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    if compact.is_empty() {
        return "(empty)".to_string();
    }

    let mut match_byte = None;
    let mut match_term = None;
    for term in terms {
        if let Some(idx) = compact.find(term) {
            if match_byte.is_none_or(|cur| idx < cur) {
                match_byte = Some(idx);
                match_term = Some(term.as_str());
            }
        }
    }

    let mut snippet = if let Some(byte_idx) = match_byte {
        centered_snippet_by_byte(&compact, byte_idx, max_chars)
    } else {
        normalize_snippet(&compact, max_chars)
    };

    if let Some(term) = match_term {
        if !term.is_empty() {
            if let Some(pos) = snippet.find(term) {
                let mut highlighted = String::new();
                highlighted.push_str(&snippet[..pos]);
                highlighted.push('[');
                highlighted.push_str(term);
                highlighted.push(']');
                highlighted.push_str(&snippet[pos + term.len()..]);
                snippet = highlighted;
            }
        }
    }

    snippet
}

fn centered_snippet_by_byte(text: &str, center_byte: usize, max_chars: usize) -> String {
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text.to_string();
    }

    let center_char = text[..center_byte].chars().count();
    let start_char = center_char.saturating_sub(max_chars / 3);
    let end_char = (start_char + max_chars).min(total_chars);
    let start_byte = byte_index_from_char(text, start_char);
    let end_byte = byte_index_from_char(text, end_char);

    let mut out = String::new();
    if start_char > 0 {
        out.push_str("...");
    }
    out.push_str(text[start_byte..end_byte].trim());
    if end_char < total_chars {
        out.push_str("...");
    }
    out
}

fn byte_index_from_char(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    for (count, (idx, _)) in text.char_indices().enumerate() {
        if count == char_index {
            return idx;
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_update_and_search_note() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");

        let note = store.create_note("hello world").expect("create");
        let updated = store
            .update_note(&note.id, "rust search target")
            .expect("update");
        assert_eq!(updated.title, "rust search target");

        let hits = store.search_notes("rust", 10).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, note.id);
    }

    #[test]
    fn soft_delete_and_restore() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");
        let note = store.create_note("temporary").expect("create");

        store.soft_delete_note(&note.id).expect("delete");
        let listed = store
            .list_notes(SortOrder::UpdatedDesc, 20, false)
            .expect("list");
        assert!(listed.is_empty());

        store.restore_note(&note.id).expect("restore");
        let listed_after = store
            .list_notes(SortOrder::UpdatedDesc, 20, false)
            .expect("list after restore");
        assert_eq!(listed_after.len(), 1);
    }

    #[test]
    fn list_deleted_notes_only_returns_deleted() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");
        let keep = store.create_note("keep me").expect("create keep");
        let trash = store.create_note("remove me").expect("create trash");
        store.soft_delete_note(&trash.id).expect("delete");

        let deleted = store.list_deleted_notes(20).expect("deleted list");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].id, trash.id);

        let active = store
            .list_notes(SortOrder::UpdatedDesc, 20, false)
            .expect("active list");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, keep.id);
    }

    #[test]
    fn create_or_open_daily_note_reuses_same_file() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");
        let date = NaiveDate::from_ymd_opt(2026, 3, 22).expect("date");

        let first = store.create_or_open_daily_note(date).expect("first");
        let second = store.create_or_open_daily_note(date).expect("second");
        assert_eq!(first.id, second.id);

        let expected_path = store.paths().notes_dir.join("2026-03-22.md");
        assert!(expected_path.exists());
    }

    #[test]
    fn purge_note_removes_metadata_and_file() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");
        let note = store.create_note("purge target").expect("create");
        let note_path = store.paths().note_path(&format!("{}.md", note.id));
        assert!(note_path.exists());

        store.soft_delete_note(&note.id).expect("soft delete");
        store.purge_note(&note.id).expect("purge");

        assert!(!note_path.exists());
        assert!(store.load_note(&note.id).is_err());
    }

    #[test]
    fn purge_deleted_notes_clears_trash_and_keeps_active() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");

        let keep = store.create_note("keep").expect("create keep");
        let trash_a = store.create_note("trash a").expect("create trash a");
        let trash_b = store.create_note("trash b").expect("create trash b");

        store.soft_delete_note(&trash_a.id).expect("delete a");
        store.soft_delete_note(&trash_b.id).expect("delete b");

        let purged = store.purge_deleted_notes().expect("purge deleted");
        assert_eq!(purged, 2);

        let deleted_after = store.list_deleted_notes(20).expect("deleted after");
        assert!(deleted_after.is_empty());

        let active = store
            .list_notes(SortOrder::UpdatedDesc, 20, false)
            .expect("active list");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, keep.id);

        assert!(store.load_note(&trash_a.id).is_err());
        assert!(store.load_note(&trash_b.id).is_err());
        assert!(store.load_note(&keep.id).is_ok());
    }

    #[test]
    fn build_search_snippet_highlights_first_term() {
        let body = "abc def ghi rust language memo";
        let terms = vec!["rust".to_string()];
        let snippet = build_search_snippet(body, &terms, 24);
        assert!(snippet.contains("[rust]"));
    }

    #[test]
    fn export_and_import_roundtrip_restores_notes() {
        let temp = tempdir().expect("tempdir");
        let source_paths = AppPaths::from_root(temp.path().join("source-data"));
        let mut source = NoteStore::open(source_paths).expect("open source");
        let note_a = source.create_note("alpha body").expect("create alpha");
        let note_b = source.create_note("beta body").expect("create beta");
        source.soft_delete_note(&note_b.id).expect("delete beta");

        let export_path = temp.path().join("export").join("memo-export.json");
        let count = source.export_to_json(&export_path).expect("export");
        assert_eq!(count, 2);

        let target_paths = AppPaths::from_root(temp.path().join("target-data"));
        let mut target = NoteStore::open(target_paths).expect("open target");
        let summary = target.import_from_json(&export_path).expect("import");
        assert_eq!(summary.created, 2);
        assert_eq!(summary.updated, 0);
        assert_eq!(summary.skipped, 0);

        let loaded_a = target.load_note(&note_a.id).expect("load alpha");
        assert_eq!(loaded_a.body, "alpha body");

        let deleted = target.list_deleted_notes(10).expect("deleted list");
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].id, note_b.id);
    }

    #[test]
    fn import_skips_file_name_conflict_for_different_id() {
        let temp = tempdir().expect("tempdir");
        let source_paths = AppPaths::from_root(temp.path().join("source-data"));
        let mut source = NoteStore::open(source_paths).expect("open source");
        let note = source.create_note("from export").expect("create source");
        let export_path = temp.path().join("export").join("memo-export.json");
        source.export_to_json(&export_path).expect("export");

        let target_paths = AppPaths::from_root(temp.path().join("target-data"));
        let mut target = NoteStore::open(target_paths).expect("open target");
        target
            .create_note_with_file_name(
                "conflict-id".to_string(),
                format!("{}.md", note.id),
                "existing file-name owner",
                false,
            )
            .expect("create conflict note");

        let summary = target.import_from_json(&export_path).expect("import");
        assert_eq!(summary.created, 0);
        assert_eq!(summary.updated, 0);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn import_single_text_file_creates_one_note() {
        let temp = tempdir().expect("tempdir");
        let source_file = temp.path().join("single-note.md");
        fs::write(&source_file, "# Imported\nhello from file").expect("write source");

        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");
        let summary = store.import_from_path(&source_file).expect("import");
        assert_eq!(summary.created, 1);
        assert_eq!(summary.updated, 0);
        assert_eq!(summary.skipped, 0);

        let list = store
            .list_notes(SortOrder::UpdatedDesc, 10, false)
            .expect("list");
        assert_eq!(list.len(), 1);
        let loaded = store.load_note(&list[0].id).expect("load");
        assert!(loaded.body.contains("hello from file"));
    }

    #[test]
    fn import_rejects_unsupported_extension() {
        let temp = tempdir().expect("tempdir");
        let source_file = temp.path().join("binary.bin");
        fs::write(&source_file, b"not text").expect("write source");

        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");
        let err = store.import_from_path(&source_file).expect_err("must fail");
        assert!(err.to_string().contains("unsupported import extension"));
    }

    #[test]
    fn export_single_text_file_writes_selected_note_body() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");
        let note = store.create_note("hello export text").expect("create");

        let out_path = temp.path().join("out").join("memo.txt");
        let count = store
            .export_to_path(&out_path, Some(&note.id))
            .expect("export text");
        assert_eq!(count, 1);
        let content = fs::read_to_string(out_path).expect("read out");
        assert_eq!(content, "hello export text");
    }

    #[test]
    fn export_text_requires_selected_note() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let store = NoteStore::open(paths).expect("open store");
        let out_path = temp.path().join("out").join("memo.md");
        let err = store
            .export_to_path(&out_path, None)
            .expect_err("must fail");
        assert!(err
            .to_string()
            .contains("text export requires a target note"));
    }

    #[test]
    fn update_tags_and_search_by_tag() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");
        let note = store.create_note("tag body").expect("create");

        store
            .update_note_tags(&note.id, "work idea")
            .expect("update tags");

        let loaded = store.load_note(&note.id).expect("load");
        assert_eq!(loaded.tags, vec!["work".to_string(), "idea".to_string()]);

        let hits = store.search_notes("#work", 10).expect("tag search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, note.id);
    }

    #[test]
    fn escape_like_pattern_escapes_wildcards_and_escape_char() {
        assert_eq!(escape_like_pattern("100%_ok^x"), "100^%^_ok^^x");
    }

    #[test]
    fn like_fallback_treats_wildcards_as_literals() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");
        let note_hit = store.create_note("100% match").expect("create hit");
        store.create_note("plain text").expect("create miss");

        let hits = store.search_notes_with_like("%", 10).expect("like search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, note_hit.id);
    }
}
