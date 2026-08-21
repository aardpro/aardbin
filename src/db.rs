//! SQLite storage (SPEC §5, §6, §31).
//!
//! Single embedded file in WAL mode. Single-user workload → one
//! `tokio::sync::Mutex<Connection>` is sufficient and keeps everything
//! trivially correct (WAL still helps crash durability & reader concurrency
//! with external tools).

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

const SCHEMA_V1: &str = r#"
CREATE TABLE records (
    id TEXT PRIMARY KEY,
    encrypted_content BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX idx_records_updated_at ON records(updated_at DESC);

CREATE TABLE attachments (
    id TEXT PRIMARY KEY,
    record_id TEXT NOT NULL,
    original_filename TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    mime_type TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(record_id) REFERENCES records(id) ON DELETE CASCADE
);
CREATE INDEX idx_attachments_record_id ON attachments(record_id);
"#;

#[derive(Debug, Clone, Serialize)]
pub struct RecordRow {
    pub id: String,
    #[serde(skip_serializing)]
    pub blob: Vec<u8>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentRow {
    pub id: String,
    pub record_id: String,
    pub original_filename: String,
    pub size_bytes: i64,
    pub mime_type: String,
    pub created_at: i64,
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Db> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000u32)?;
        Self::migrate(&conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Schema migration driven by `PRAGMA user_version` (SPEC §31.1).
    fn migrate(conn: &Connection) -> anyhow::Result<()> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
            conn.pragma_update(None, "user_version", 1i64)?;
        }
        Ok(())
    }

    pub async fn create_record(&self, id: &str, blob: &[u8], now: i64) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO records (id, encrypted_content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)",
            params![id, blob, now],
        )?;
        Ok(())
    }

    pub async fn update_record(&self, id: &str, blob: &[u8], now: i64) -> anyhow::Result<bool> {
        let conn = self.conn.lock().await;
        let n = conn.execute(
            "UPDATE records SET encrypted_content = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, blob, now],
        )?;
        Ok(n > 0)
    }

    pub async fn touch_record(&self, id: &str, now: i64) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE records SET updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(())
    }

    /// Deletes the record; attachment rows cascade. Returns attachment ids
    /// so the caller can remove files from disk.
    pub async fn delete_record(&self, id: &str) -> anyhow::Result<Option<Vec<String>>> {
        let conn = self.conn.lock().await;
        let attachment_ids = {
            let mut stmt = conn.prepare("SELECT id FROM attachments WHERE record_id = ?1")?;
            let ids: Vec<String> = stmt
                .query_map(params![id], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            ids
        };
        let n = conn.execute("DELETE FROM records WHERE id = ?1", params![id])?;
        Ok((n > 0).then_some(attachment_ids))
    }

    pub async fn get_record(&self, id: &str) -> anyhow::Result<Option<RecordRow>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, encrypted_content, created_at, updated_at FROM records WHERE id = ?1",
        )?;
        let row = stmt
            .query_row(params![id], |r| {
                Ok(RecordRow {
                    id: r.get(0)?,
                    blob: r.get(1)?,
                    created_at: r.get(2)?,
                    updated_at: r.get(3)?,
                })
            })
            .optional()?;
        Ok(row)
    }

    /// Page is 1-based. Returns (rows, total_count).
    pub async fn list_records(
        &self,
        page: i64,
        page_size: i64,
    ) -> anyhow::Result<(Vec<RecordRow>, i64)> {
        let conn = self.conn.lock().await;
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM records", [], |r| r.get(0))?;
        let offset = (page.max(1) - 1) * page_size;
        let mut stmt = conn.prepare(
            "SELECT id, encrypted_content, created_at, updated_at
             FROM records ORDER BY updated_at DESC, id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt
            .query_map(params![page_size, offset], |r| {
                Ok(RecordRow {
                    id: r.get(0)?,
                    blob: r.get(1)?,
                    created_at: r.get(2)?,
                    updated_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok((rows, total))
    }

    pub async fn insert_attachment(&self, a: &AttachmentRow) -> anyhow::Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO attachments (id, record_id, original_filename, size_bytes, mime_type, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                a.id,
                a.record_id,
                a.original_filename,
                a.size_bytes,
                a.mime_type,
                a.created_at
            ],
        )?;
        Ok(())
    }

    pub async fn get_attachment(&self, id: &str) -> anyhow::Result<Option<AttachmentRow>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, record_id, original_filename, size_bytes, mime_type, created_at
             FROM attachments WHERE id = ?1",
        )?;
        let row = stmt.query_row(params![id], map_attachment).optional()?;
        Ok(row)
    }

    pub async fn list_attachments(&self, record_id: &str) -> anyhow::Result<Vec<AttachmentRow>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, record_id, original_filename, size_bytes, mime_type, created_at
             FROM attachments WHERE record_id = ?1 ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt
            .query_map(params![record_id], map_attachment)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub async fn attachments_for_records(
        &self,
        record_ids: &[String],
    ) -> anyhow::Result<Vec<AttachmentRow>> {
        if record_ids.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.conn.lock().await;
        let placeholders = record_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, record_id, original_filename, size_bytes, mime_type, created_at
             FROM attachments WHERE record_id IN ({placeholders}) ORDER BY created_at ASC, id ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(record_ids.iter()),
                map_attachment,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Deletes one attachment belonging to the given record. Returns the row
    /// (for file removal) when it existed.
    pub async fn delete_attachment(
        &self,
        record_id: &str,
        attachment_id: &str,
    ) -> anyhow::Result<Option<AttachmentRow>> {
        let conn = self.conn.lock().await;
        let row = {
            let mut stmt = conn.prepare(
                "SELECT id, record_id, original_filename, size_bytes, mime_type, created_at
                 FROM attachments WHERE id = ?1 AND record_id = ?2",
            )?;
            stmt.query_row(params![attachment_id, record_id], map_attachment)
                .optional()?
        };
        if row.is_some() {
            conn.execute(
                "DELETE FROM attachments WHERE id = ?1 AND record_id = ?2",
                params![attachment_id, record_id],
            )?;
        }
        Ok(row)
    }

    /// All attachment ids — used by the startup orphan scan (SPEC §33).
    pub async fn all_attachment_ids(&self) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT id FROM attachments")?;
        let ids = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(ids)
    }
}

fn map_attachment(r: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentRow> {
    Ok(AttachmentRow {
        id: r.get(0)?,
        record_id: r.get(1)?,
        original_filename: r.get(2)?,
        size_bytes: r.get(3)?,
        mime_type: r.get(4)?,
        created_at: r.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    async fn test_db() -> (Db, PathBuf) {
        let dir = std::env::temp_dir().join(format!("aardbin-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("test.db")).unwrap();
        (db, dir)
    }

    fn attachment(id: &str, record: &str, name: &str) -> AttachmentRow {
        AttachmentRow {
            id: id.into(),
            record_id: record.into(),
            original_filename: name.into(),
            size_bytes: 42,
            mime_type: "text/plain".into(),
            created_at: 1000,
        }
    }

    #[tokio::test]
    async fn record_crud() {
        let (db, dir) = test_db().await;
        db.create_record("r1", b"blob1", 100).await.unwrap();
        let row = db.get_record("r1").await.unwrap().unwrap();
        assert_eq!(row.blob, b"blob1");
        assert_eq!(row.created_at, 100);
        assert_eq!(row.updated_at, 100);

        db.update_record("r1", b"blob2", 200).await.unwrap();
        let row = db.get_record("r1").await.unwrap().unwrap();
        assert_eq!(row.blob, b"blob2");
        assert_eq!(row.updated_at, 200);
        assert_eq!(row.created_at, 100);

        assert!(!db.update_record("nope", b"x", 1).await.unwrap());
        assert_eq!(db.delete_record("r1").await.unwrap(), Some(vec![]));
        assert!(db.get_record("r1").await.unwrap().is_none());
        assert_eq!(db.delete_record("r1").await.unwrap(), None);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn pagination_and_ordering() {
        let (db, dir) = test_db().await;
        for i in 0..7 {
            db.create_record(&format!("r{i}"), b"b", 1000 + i)
                .await
                .unwrap();
        }
        let (page1, total) = db.list_records(1, 3).await.unwrap();
        assert_eq!(total, 7);
        assert_eq!(page1.len(), 3);
        assert_eq!(page1[0].id, "r6"); // newest updated_at first
        let (page3, _) = db.list_records(3, 3).await.unwrap();
        assert_eq!(page3.len(), 1);
        assert_eq!(page3[0].id, "r0");
        let (beyond, _) = db.list_records(4, 3).await.unwrap();
        assert!(beyond.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn cascade_delete_returns_attachment_ids() {
        let (db, dir) = test_db().await;
        db.create_record("r1", b"b", 1).await.unwrap();
        db.insert_attachment(&attachment("a1", "r1", "f1.txt"))
            .await
            .unwrap();
        db.insert_attachment(&attachment("a2", "r1", "f2.txt"))
            .await
            .unwrap();

        let mut ids = db.delete_record("r1").await.unwrap().unwrap();
        ids.sort();
        assert_eq!(ids, vec!["a1", "a2"]);
        assert!(db.list_attachments("r1").await.unwrap().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn attachment_scoped_delete() {
        let (db, dir) = test_db().await;
        db.create_record("r1", b"b", 1).await.unwrap();
        db.create_record("r2", b"b", 2).await.unwrap();
        db.insert_attachment(&attachment("a1", "r1", "f.txt"))
            .await
            .unwrap();
        // wrong record scope → nothing deleted
        assert!(db.delete_attachment("r2", "a1").await.unwrap().is_none());
        assert!(db.get_attachment("a1").await.unwrap().is_some());
        // right scope
        assert!(db.delete_attachment("r1", "a1").await.unwrap().is_some());
        assert!(db.get_attachment("a1").await.unwrap().is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn batch_attachment_fetch() {
        let (db, dir) = test_db().await;
        db.create_record("r1", b"b", 1).await.unwrap();
        db.create_record("r2", b"b", 2).await.unwrap();
        db.insert_attachment(&attachment("a1", "r1", "x.png"))
            .await
            .unwrap();
        db.insert_attachment(&attachment("a2", "r2", "y.txt"))
            .await
            .unwrap();
        let all = db
            .attachments_for_records(&["r1".into(), "r2".into()])
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        assert!(db.attachments_for_records(&[]).await.unwrap().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn migration_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("aardbin-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.db");
        let db1 = Db::open(&path).unwrap();
        db1.create_record("r", b"b", 1).await.unwrap();
        drop(db1);
        // reopen → migrate again, no error, data preserved
        let db2 = Db::open(&path).unwrap();
        assert!(db2.get_record("r").await.unwrap().is_some());
        std::fs::remove_dir_all(dir).ok();
    }
}
