//! aardbin-cli — command-line client for the aardbin JSON API.
//!
//! Key resolution: AARDBIN_ACCESS_KEY env > ACCESS_KEY env > config.toml > prompt.
//! Server URL:     AARDBIN_URL env > config.toml > http://127.0.0.1:8080.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "aardbin-cli", version = "1.0.0", about = "CLI client for aardbin")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    /// Server base URL (overrides AARDBIN_URL / config).
    #[arg(long, global = true)]
    url: Option<String>,

    /// Access key (overrides env / config).
    #[arg(long, global = true)]
    key: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new record.
    Paste {
        /// Record title.
        #[arg(short, long)]
        title: Option<String>,
        /// Content body (reads stdin if omitted).
        #[arg(short, long)]
        content: Option<String>,
        /// Files to attach.
        #[arg(short, long)]
        file: Vec<PathBuf>,
    },
    /// List records (paginated).
    List {
        /// Page number (1-based).
        #[arg(short, long, default_value = "1")]
        page: i64,
        /// Records per page.
        #[arg(long)]
        page_size: Option<i64>,
    },
    /// Show a single record.
    Get {
        /// Record ID.
        id: String,
    },
    /// Delete a record.
    Delete {
        /// Record ID.
        id: String,
    },
    /// Upload an attachment to an existing record.
    Upload {
        /// Record ID.
        record_id: String,
        /// File to upload.
        file: PathBuf,
    },
    /// Download an attachment.
    Download {
        /// Attachment ID.
        id: String,
        /// Output path (prints to stdout if omitted).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

// ---------------------------------------------------------------------------
// Config resolution
// ---------------------------------------------------------------------------

#[derive(Default, Deserialize)]
struct ConfigFile {
    server: Option<ServerConfig>,
}

#[derive(Default, Deserialize)]
struct ServerConfig {
    url: Option<String>,
    access_key: Option<String>,
}

fn resolve_url(cli_url: Option<&str>) -> String {
    if let Some(u) = cli_url {
        return u.to_string();
    }
    if let Ok(u) = std::env::var("AARDBIN_URL") {
        return u;
    }
    if let Some(cfg) = load_config_file() {
        if let Some(u) = cfg.server.as_ref().and_then(|s| s.url.as_deref()) {
            return u.to_string();
        }
    }
    "http://127.0.0.1:8080".to_string()
}

fn resolve_key(cli_key: Option<&str>) -> Result<String> {
    if let Some(k) = cli_key {
        return Ok(k.to_string());
    }
    if let Ok(k) = std::env::var("AARDBIN_ACCESS_KEY") {
        return Ok(k);
    }
    if let Ok(k) = std::env::var("ACCESS_KEY") {
        return Ok(k);
    }
    if let Some(cfg) = load_config_file() {
        if let Some(k) = cfg.server.as_ref().and_then(|s| s.access_key.as_deref()) {
            return Ok(k.to_string());
        }
    }
    // Interactive prompt (skip if piped)
    if atty::is(atty::Stream::Stdin) {
        eprint!("Access Key: ");
        let mut key = String::new();
        std::io::stdin().read_line(&mut key)?;
        let key = key.trim().to_string();
        if !key.is_empty() {
            return Ok(key);
        }
    }
    bail!("No access key found. Set AARDBIN_ACCESS_KEY or use --key.")
}

fn load_config_file() -> Option<ConfigFile> {
    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(home).join(".config/aardbin/config.toml");
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

// ---------------------------------------------------------------------------
// HTTP client helpers
// ---------------------------------------------------------------------------

struct Client {
    base_url: String,
    http: reqwest::Client,
}

impl Client {
    fn new(base_url: &str, access_key: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {access_key}"))
                .context("invalid access key characters")?,
        );
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;
        Ok(Client {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        })
    }

    /// Send a request with automatic retry on 429 (linear backoff).
    /// If the request cannot be cloned (e.g. multipart), falls back to a single attempt.
    async fn send_with_retry(&self, req: reqwest::Request) -> Result<reqwest::Response> {
        let mut attempts = 0;
        loop {
            let r = match req.try_clone() {
                Some(r) => r,
                None => {
                    // Non-cloneable (multipart): send once without retry
                    return Ok(self.http.execute(req).await?);
                }
            };
            let resp = self.http.execute(r).await?;
            if resp.status() != reqwest::StatusCode::TOO_MANY_REQUESTS || attempts >= 3 {
                return Ok(resp);
            }
            let retry_after = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1);
            let wait = retry_after.min(30) * (attempts + 1);
            eprintln!("Rate limited. Retrying in {wait}s...");
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            attempts += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// JSON response types (matching the API)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ApiListResponse {
    records: Vec<ApiRecordSummary>,
    page: i64,
    total_pages: i64,
    total: i64,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ApiRecordSummary {
    id: String,
    title: String,
    untitled: bool,
    snippet: String,
    updated_at: i64,
    attachment_count: usize,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ApiRecord {
    id: String,
    title: String,
    content: String,
    untitled: bool,
    updated_at: i64,
    attachments: Vec<ApiAttachmentMeta>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ApiAttachmentMeta {
    id: String,
    original_filename: String,
    size_bytes: i64,
}

#[derive(Deserialize)]
struct ApiCreated {
    id: String,
}

#[derive(Deserialize)]
struct ApiError {
    error: String,
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

async fn cmd_paste(c: &Client, title: Option<String>, content: Option<String>, files: &[PathBuf]) -> Result<()> {
    let body_content = match content {
        Some(c) => c,
        None => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    let mut form = reqwest::multipart::Form::new()
        .text("title", title.unwrap_or_default())
        .text("content", body_content);

    for f in files {
        let filename = f.file_name().unwrap_or_default().to_string_lossy().to_string();
        let data = tokio::fs::read(f).await?;
        let part = reqwest::multipart::Part::bytes(data)
            .file_name(filename);
        form = form.part("files", part);
    }

    let req = c.http.post(format!("{}/api/records", c.base_url))
        .multipart(form)
        .build()?;
    let resp = c.send_with_retry(req).await?;
    let status = resp.status();
    if status == reqwest::StatusCode::CREATED {
        let created: ApiCreated = resp.json().await?;
        println!("{}", created.id);
    } else {
        let err: ApiError = resp.json().await.unwrap_or(ApiError { error: format!("HTTP {status}") });
        bail!("{}", err.error);
    }
    Ok(())
}

async fn cmd_list(c: &Client, page: i64, page_size: Option<i64>) -> Result<()> {
    let mut url = format!("{}/api/records?page={page}", c.base_url);
    if let Some(ps) = page_size {
        url.push_str(&format!("&page_size={ps}"));
    }
    let resp = c.http.get(&url).send().await?;
    if !resp.status().is_success() {
        let err: ApiError = resp.json().await.unwrap_or(ApiError { error: "request failed".into() });
        bail!("{}", err.error);
    }
    let list: ApiListResponse = resp.json().await?;
    for r in &list.records {
        let title = if r.untitled { "(untitled)" } else { &r.title };
        let att = if r.attachment_count > 0 { format!(" 📎×{}", r.attachment_count) } else { String::new() };
        println!("{}  {}{}  {}", r.id, title, att, r.snippet);
    }
    eprintln!("Page {}/{} · {} records", list.page, list.total_pages, list.total);
    Ok(())
}

async fn cmd_get(c: &Client, id: &str) -> Result<()> {
    let resp = c.http.get(format!("{}/api/records/{id}", c.base_url)).send().await?;
    if !resp.status().is_success() {
        let err: ApiError = resp.json().await.unwrap_or(ApiError { error: "request failed".into() });
        bail!("{}", err.error);
    }
    let record: ApiRecord = resp.json().await?;
    if record.untitled {
        println!("(untitled)");
    } else {
        println!("Title: {}", record.title);
    }
    println!("Updated: {}", record.updated_at);
    if !record.attachments.is_empty() {
        println!("Attachments:");
        for a in &record.attachments {
            println!("  {} ({} bytes) — {}", a.id, a.size_bytes, a.original_filename);
        }
    }
    println!("---");
    println!("{}", record.content);
    Ok(())
}

async fn cmd_delete(c: &Client, id: &str) -> Result<()> {
    let resp = c.http.delete(format!("{}/api/records/{id}", c.base_url)).send().await?;
    if resp.status().is_success() {
        println!("Deleted {id}");
    } else {
        let err: ApiError = resp.json().await.unwrap_or(ApiError { error: "request failed".into() });
        bail!("{}", err.error);
    }
    Ok(())
}

async fn cmd_upload(c: &Client, record_id: &str, file: &PathBuf) -> Result<()> {
    // Get existing record, re-create with additional attachment
    let resp = c.http.get(format!("{}/api/records/{record_id}", c.base_url)).send().await?;
    if !resp.status().is_success() {
        let err: ApiError = resp.json().await.unwrap_or(ApiError { error: "record not found".into() });
        bail!("{}", err.error);
    }
    let record: ApiRecord = resp.json().await?;

    let filename = file.file_name().unwrap_or_default().to_string_lossy().to_string();
    let data = tokio::fs::read(file).await?;

    let form = reqwest::multipart::Form::new()
        .text("title", record.title)
        .text("content", record.content)
        .part("files", reqwest::multipart::Part::bytes(data).file_name(filename));

    let req = c.http.post(format!("{}/api/records", c.base_url))
        .multipart(form)
        .build()?;
    let resp = c.send_with_retry(req).await?;
    let status = resp.status();
    if status == reqwest::StatusCode::CREATED {
        let created: ApiCreated = resp.json().await?;
        println!("Created new record {} with attachment (API does not support adding to existing record)", created.id);
    } else {
        let err: ApiError = resp.json().await.unwrap_or(ApiError { error: format!("HTTP {status}") });
        bail!("{}", err.error);
    }
    Ok(())
}

async fn cmd_download(c: &Client, id: &str, output: Option<PathBuf>) -> Result<()> {
    let resp = c.http.get(format!("{}/api/attachments/{id}", c.base_url)).send().await?;
    if !resp.status().is_success() {
        let err: ApiError = resp.json().await.unwrap_or(ApiError { error: "attachment not found".into() });
        bail!("{}", err.error);
    }
    let bytes = resp.bytes().await?;
    match output {
        Some(path) => {
            tokio::fs::write(&path, &bytes).await?;
            println!("Saved {} bytes to {}", bytes.len(), path.display());
        }
        None => {
            std::io::Write::write_all(&mut std::io::stdout(), &bytes)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Minimal atty check (avoid adding atty crate)
// ---------------------------------------------------------------------------
mod atty {
    pub enum Stream { Stdin }
    pub fn is(_stream: Stream) -> bool {
        // Simple heuristic: if stdin is a terminal
        unsafe { libc_isatty(0) != 0 }
    }
    extern "C" {
        #[link_name = "isatty"]
        fn libc_isatty(fd: i32) -> i32;
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let base_url = resolve_url(cli.url.as_deref());
    let access_key = resolve_key(cli.key.as_deref())?;
    let client = Client::new(&base_url, &access_key)?;

    match cli.cmd {
        Cmd::Paste { title, content, file } => cmd_paste(&client, title, content, &file).await,
        Cmd::List { page, page_size } => cmd_list(&client, page, page_size).await,
        Cmd::Get { id } => cmd_get(&client, &id).await,
        Cmd::Delete { id } => cmd_delete(&client, &id).await,
        Cmd::Upload { record_id, file } => cmd_upload(&client, &record_id, &file).await,
        Cmd::Download { id, output } => cmd_download(&client, &id, output).await,
    }
}
