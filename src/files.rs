//! Attachment file storage on the local filesystem (SPEC §9, §32, §33).
//!
//! Upload order per SPEC §32:
//!   1. generate UUID
//!   2. write temp file
//!   3. fsync / close
//!   4. rename to final UUID file
//!   5. SQLite INSERT (handled by caller)
//!
//! Temp files live in `data/tmp/`; orphans there are removed at startup.
//! Orphans in `data/attachments/` (file without DB row) are only logged.

use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone)]
pub struct FileStore {
    attachments_dir: PathBuf,
    tmp_dir: PathBuf,
}

impl FileStore {
    pub fn new(data_dir: &Path) -> std::io::Result<FileStore> {
        let attachments_dir = data_dir.join("attachments");
        let tmp_dir = data_dir.join("tmp");
        std::fs::create_dir_all(&attachments_dir)?;
        std::fs::create_dir_all(&tmp_dir)?;
        Ok(FileStore {
            attachments_dir,
            tmp_dir,
        })
    }

    /// Creates a fresh temp file for one upload. Caller streams into the
    /// returned writer, then calls `finalize`.
    pub async fn begin_upload(&self) -> std::io::Result<(String, PathBuf, tokio::fs::File)> {
        let id = Uuid::new_v4().to_string();
        let path = self.tmp_dir.join(&id);
        let file = tokio::fs::File::create(&path).await?;
        Ok((id, path, file))
    }

    /// fsync + close + rename to `attachments/{uuid}`. Returns bytes written.
    pub async fn finalize_upload(&self, id: &str, file: tokio::fs::File) -> std::io::Result<u64> {
        file.sync_all().await?;
        drop(file);
        let tmp = self.tmp_dir.join(id);
        let size = tokio::fs::metadata(&tmp).await?.len();
        let final_path = self.attachments_dir.join(id);
        tokio::fs::rename(&tmp, &final_path).await?;
        Ok(size)
    }

    pub async fn abort_upload(&self, id: &str) {
        let _ = tokio::fs::remove_file(self.tmp_dir.join(id)).await;
    }

    pub fn attachment_path(&self, id: &str) -> PathBuf {
        // id comes from the DB and was generated as a UUID; never from user
        // input, so no path traversal is possible here. Defensive check anyway:
        debug_assert!(!id.contains('/') && !id.contains(".."));
        self.attachments_dir.join(id)
    }

    pub async fn delete_attachment(&self, id: &str) {
        if let Err(e) = tokio::fs::remove_file(self.attachments_dir.join(id)).await {
            tracing::warn!(attachment_id = %id, error = %e, "failed to delete attachment file");
        }
    }

    /// Startup scan (SPEC §33): files present on disk but unknown to SQLite.
    /// MVP: log a warning, never auto-delete. Also cleans stale temp files.
    pub async fn orphan_scan(&self, known_ids: &[String]) -> std::io::Result<()> {
        // stale temp files are safe to remove (upload never committed)
        let mut tmp_entries = tokio::fs::read_dir(&self.tmp_dir).await?;
        while let Some(entry) = tmp_entries.next_entry().await? {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }

        let mut entries = tokio::fs::read_dir(&self.attachments_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if !known_ids.contains(&name) {
                tracing::warn!(
                    file = %name,
                    "orphan attachment detected (exists on disk, not in database); \
                     left untouched — inspect and remove manually if unwanted"
                );
            }
        }
        Ok(())
    }
}

/// RFC 5987 Content-Disposition with UTF-8 filename support (SPEC §9.1.1).
pub fn content_disposition(kind: &str, original_filename: &str) -> String {
    let mut fallback = String::with_capacity(original_filename.len());
    for ch in original_filename.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | ' ' => fallback.push(ch),
            _ => fallback.push('_'),
        }
    }
    if fallback.is_empty() {
        fallback = "file".into();
    }
    let mut encoded = String::new();
    for byte in original_filename.as_bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_' => {
                encoded.push(*byte as char)
            }
            b' ' => encoded.push_str("%20"),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    format!("{kind}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

/// MIME types allowed to be served inline (SPEC §9.1.1).
pub const INLINE_WHITELIST: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

pub fn guess_mime(filename: &str, declared: Option<&str>) -> String {
    // Prefer a sane declared type; fall back to extension sniffing; never
    // trust the client blindly for types it cannot know (octet-stream).
    match declared {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => mime_guess::from_path(filename)
            .first_or_octet_stream()
            .to_string(),
    }
}

pub fn human_size(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let b = bytes as f64;
    if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn disposition_ascii() {
        let h = content_disposition("attachment", "config.txt");
        assert_eq!(
            h,
            "attachment; filename=\"config.txt\"; filename*=UTF-8''config.txt"
        );
    }

    #[test]
    fn disposition_unicode() {
        let h = content_disposition("attachment", "截图 2024.png");
        assert!(h.contains("filename=\"__ 2024.png\""));
        assert!(h.contains("filename*=UTF-8''"));
        // RFC 5987 percent-encoding of 截 (E6 88 AA)
        assert!(h.contains("%E6%88%AA"));
        assert!(h.contains("%20"));
    }

    #[test]
    fn disposition_strips_dangerous() {
        let h = content_disposition("attachment", "evil\"name\\.txt");
        assert!(!h.contains('"') || h.matches('"').count() == 2); // only the fallback quotes
        assert!(h.contains("filename=\"evil_name_.txt\""));
    }

    #[test]
    fn empty_filename_fallback() {
        let h = content_disposition("attachment", "");
        assert!(h.contains("filename=\"file\""));
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[tokio::test]
    async fn upload_finalize_and_delete() {
        let dir = std::env::temp_dir().join(format!("aardbin-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = FileStore::new(&dir).unwrap();

        let (id, _tmp, mut f) = store.begin_upload().await.unwrap();
        f.write_all(b"hello").await.unwrap();
        let size = store.finalize_upload(&id, f).await.unwrap();
        assert_eq!(size, 5);
        assert_eq!(
            tokio::fs::read(store.attachment_path(&id)).await.unwrap(),
            b"hello"
        );
        store.delete_attachment(&id).await;
        assert!(!store.attachment_path(&id).exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn orphan_scan_keeps_files() {
        let dir = std::env::temp_dir().join(format!("aardbin-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = FileStore::new(&dir).unwrap();
        tokio::fs::write(dir.join("attachments").join("orphan"), b"x")
            .await
            .unwrap();
        store.orphan_scan(&[]).await.unwrap();
        // MVP: never auto-delete
        assert!(dir.join("attachments").join("orphan").exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
