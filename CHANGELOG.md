# Changelog

All notable changes to aardbin are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-08-21

Initial public release.

### Added
- Single-user self-hosted personal bin for text snippets and small attachments
- AES-256-GCM encryption for text at rest (title + content)
- Plaintext attachment storage with UUID filenames
- SQLite database with WAL mode
- Server-side rendering with minijinja + HTMX for interactivity
- SSE real-time sync across multiple browser clients
- Stateless HMAC-SHA256 signed session cookies (survives restart)
- Login rate limiting (5 failures / 5 min window per IP)
- Origin / Sec-Fetch-Site CSRF guard on POST endpoints
- Docker single-container deployment (~30 MB image)
- Responsive UI (desktop / tablet / mobile)
- Attachment drag-and-drop upload with size validation
- Copy-to-clipboard endpoint (decrypted content served as text/plain)
- Graceful degradation for undecryptable records
- Health check endpoint (`/healthz`)
- SSE heartbeat (25s ping)
- Orphan attachment detection on startup (warn-only)
