
<p align="center">
  <img src="static/brand.webp" alt="aardbin — drop · sync · keep" width="360">
</p>

# <a id="zh"></a>说明 [en](#en)

aardbin是一个私有化部署、单用户、网页式的个人文本与小附件共享工具 —— 一个 Rust 二进制 + 一个 SQLite 文件 + 一个附件目录 + 一个 Docker 容器。

部署后打开网页，即可在自己的设备（PC / 手机 / 平板）之间快速丢文本、代码、配置、截图与小文件。

> 完整设计见 [`docs/SPEC.md`](docs/SPEC.md)（v1.0）。

## 特性

- **私有化单用户**：无多用户 / 注册 / 权限系统
- **极简架构**：Rust + Axum + HTMX + SSE，无 Node 运行时、无 Redis、无外部数据库、无 CDN
- **加密静态存储**：标题与正文用 AES-256-GCM 加密落盘（`CRYPTO_KEY` 只存在环境变量中）
- **附件明文存储**：小文件（默认 ≤ 2 MiB/个）走 UUID 落盘，元数据入 SQLite
- **多客户端实时同步**：SSE 广播 `data_changed`，HTMX 局部刷新列表
- **无状态签名会话**：HMAC Cookie 重启不掉线，换 `ACCESS_KEY` 全部失效
- **单容器部署**：multi-stage 构建，最终镜像 ~30 MB，仅需 `data/` 目录持久化

## 快速开始（Docker Compose）

1. 生成两个高熵密钥：

   ```bash
   ACCESS_KEY=$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')
   CRYPTO_KEY=$(python3 -c 'import secrets; print(secrets.token_hex(32))')   # 64 位十六进制
   ```

2. 写入 `.env`：

   ```dotenv
   ACCESS_KEY=<上一步的 ACCESS_KEY>
   CRYPTO_KEY=<上一步的 CRYPTO_KEY>
   ```

3. 启动：

   ```bash
   docker compose up -d --build
   ```

   打开 `http://localhost:8080`，输入 ACCESS_KEY 登录。

### 重要警告

- **`CRYPTO_KEY` 一旦丢失，所有加密文本永久不可恢复**。请务必将 `.env` 与 `data/` 一起备份。
- **换 `CRYPTO_KEY` 不会自动重加密**旧记录；旧记录会显示为 `Unable to decrypt`（附件仍可下载、记录可删除）。
- 附件是**明文存储**的，请勿存放敏感文件。

## 环境变量

| 变量 | 默认值 | 说明 |
|---|---|---|
| `ACCESS_KEY` | **必填** | 登录凭证（≥16 字符，建议高熵随机） |
| `CRYPTO_KEY` | **必填** | 64 位十六进制 = 32 字节 AES-256 密钥 |
| `MAX_ATTACHMENT_BYTES` | `2097152` | 单附件最大字节数（2 MiB） |
| `MAX_REQUEST_BYTES` | `8388608` | 单请求体总字节数（8 MiB） |
| `MAX_CONTENT_BYTES` | `1048576` | 正文最大字节数（1 MiB） |
| `PAGE_SIZE` | `20` | 每页记录数 |
| `SESSION_TTL` | `7d` | 会话有效期（`7d` / `12h` / `30m`） |
| `COOKIE_SECURE` | `true` | 仅 HTTPS 发送会话 Cookie；**纯 HTTP 内网部署请设为 `false`** |
| `LISTEN_ADDR` | `0.0.0.0:8080` | 监听地址 |
| `DATA_DIR` | `./data` | SQLite 与附件根目录 |
| `TEMPLATES_DIR` | `./templates` | 模板目录 |
| `STATIC_DIR` | `./static` | 静态资源目录 |
| `RUST_LOG` | `aardbin=info` | 日志过滤器（见 [`env_logger` 文档](https://docs.rs/env_logger)） |

## 本地开发

```bash
npm install                     # 构建期依赖：Tailwind CLI
npm run build:css               # 生成 static/app.css
cargo run                       # 启动（需设置 ACCESS_KEY / CRYPTO_KEY）
cargo test                      # 单元测试
bash scripts/smoke.sh           # 集成冒烟测试（需 curl + python3）
```

## 数据备份

所有持久化数据都在 `data/` 下：

```text
data/
├── aardbin.db
├── aardbin.db-shm
├── aardbin.db-wal
├── tmp/                        # 上传中的临时文件
└── attachments/
    └── {UUID}
```

> **备份时务必连同 `.env` 一起打包。** 丢失 `CRYPTO_KEY` 意味着**所有加密文本永久丢失**。

最可靠的备份方式（停止容器后整目录打包）：

```bash
docker compose stop
tar -czf backup.tar.gz data/ .env
docker compose start
```

## HTTPS

aardbin 自身不负责 TLS。跨公网部署时，请在前面挂 Caddy / Nginx / Traefik 终止 HTTPS，再反向代理到 `127.0.0.1:8080`，并保持 `COOKIE_SECURE=true`。

## 安全边界

aardbin 不是端到端加密系统：`CRYPTO_KEY` 保护的是**磁盘静态数据**，但拥有服务器运行权限的人仍可读取明文。登录限流、SameSite=Lax + Origin 校验、附件 `nosniff` + 强制下载等已内置。

### 反代限流限制

登录限流按来源 IP 计数。部署在反向代理后面时，所有客户端共享代理 IP（aardbin 不读取 `X-Forwarded-For`），一个用户的爆破会锁死同一代理后的所有用户。单用户部署场景（即设计目标）下此问题无实际影响。缓解建议见 [`SECURITY.md`](SECURITY.md)。

## 明确不做

多用户、分享链接、全文搜索、Markdown/富文本编辑器、回收站、版本历史、附件加密、WebSocket、Redis、PostgreSQL 等，详见 [`docs/SPEC.md`](docs/SPEC.md) §38。


<p align="center">
  <img src="static/brand.webp" alt="aardbin — drop · sync · keep" width="360">
</p>

# <a id="en"></a> README [中文](#zh)

Self-hosted, single-user, web-based personal bin for text snippets and small attachments — one Rust binary, one SQLite file, one attachment directory, one Docker container.

After deployment, open the browser to quickly share text, code, configs, screenshots and small files between your own devices (PC / phone / tablet).

> Full design spec: [`docs/SPEC.md`](docs/SPEC.md)

## Features

- **Single-user, self-hosted** — no multi-user / registration / permission system
- **Minimal architecture** — Rust + Axum + HTMX + SSE, no Node runtime, no Redis, no external DB, no CDN
- **Encrypted at rest** — title & content encrypted with AES-256-GCM (`CRYPTO_KEY` lives in env only)
- **Plaintext attachments** — small files (default ≤ 2 MiB each) stored as UUID on disk, metadata in SQLite
- **Multi-device real-time sync** — SSE broadcasts `data_changed`, HTMX partial-refreshes the list
- **Stateless signed sessions** — HMAC Cookie survives restarts; changing `ACCESS_KEY` invalidates all sessions
- **Single-container deployment** — multi-stage build, final image ~30 MB, only `data/` needs persistence

## Quick Start (Docker Compose)

1. Generate two high-entropy keys:

   ```bash
   ACCESS_KEY=$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')
   CRYPTO_KEY=$(python3 -c 'import secrets; print(secrets.token_hex(32))')   # 64 hex chars
   ```

2. Write `.env`:

   ```dotenv
   ACCESS_KEY=<ACCESS_KEY from above>
   CRYPTO_KEY=<CRYPTO_KEY from above>
   ```

3. Start:

   ```bash
   docker compose up -d --build
   ```

   Open `http://localhost:8080` and enter your ACCESS_KEY to log in.

### Important Warnings

- **If you lose `CRYPTO_KEY`, all encrypted text is permanently unrecoverable.** Always back up `.env` together with `data/`.
- **Changing `CRYPTO_KEY` does not re-encrypt** old records; they will show as `Unable to decrypt` (attachments still downloadable, records deletable).
- Attachments are stored in **plaintext** — do not upload sensitive files.

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `ACCESS_KEY` | **required** | Login credential (≥16 chars, high-entropy random recommended) |
| `CRYPTO_KEY` | **required** | 64 hex chars = 32-byte AES-256 key |
| `MAX_ATTACHMENT_BYTES` | `2097152` | Max bytes per attachment (2 MiB) |
| `MAX_REQUEST_BYTES` | `8388608` | Max total request body (8 MiB) |
| `MAX_CONTENT_BYTES` | `1048576` | Max content bytes (1 MiB) |
| `PAGE_SIZE` | `20` | Records per page |
| `SESSION_TTL` | `7d` | Session lifetime (`7d` / `12h` / `30m`) |
| `COOKIE_SECURE` | `true` | Send session Cookie over HTTPS only; **set `false` for plain HTTP LAN** |
| `LISTEN_ADDR` | `0.0.0.0:8080` | Listen address |
| `DATA_DIR` | `./data` | SQLite & attachments root directory |
| `TEMPLATES_DIR` | `./templates` | Template directory |
| `STATIC_DIR` | `./static` | Static assets directory |
| `RUST_LOG` | `aardbin=info` | Log filter (see [`env_logger` docs](https://docs.rs/env_logger)) |

## Local Development

```bash
npm install                     # Build-time dependency: Tailwind CLI
npm run build:css               # Generate static/app.css
cargo run                       # Start (requires ACCESS_KEY / CRYPTO_KEY env vars)
cargo test                      # Unit tests
bash scripts/smoke.sh           # Integration smoke test (requires curl + python3)
```

## Data Backup

All persisted data lives in `data/`:

```text
data/
├── aardbin.db
├── aardbin.db-shm
├── aardbin.db-wal
├── tmp/                        # In-progress upload temp files
└── attachments/
    └── {UUID}
```

> **Back up `.env` together with `data/`.** Losing `CRYPTO_KEY` means **all encrypted text is permanently lost**.

Recommended backup (stop container, archive directory):

```bash
docker compose stop
tar -czf backup.tar.gz data/ .env
docker compose start
```

## HTTPS

aardbin does not handle TLS itself. For public-facing deployments, put Caddy / Nginx / Traefik in front to terminate HTTPS, then reverse-proxy to `127.0.0.1:8080`, keeping `COOKIE_SECURE=true`.

## Security Boundary

aardbin is **not** end-to-end encrypted: `CRYPTO_KEY` protects **data at rest on disk**, but anyone with access to the running server process can read plaintext. Login rate limiting, SameSite=Lax + Origin validation, attachment `nosniff` + forced download are all built-in.

### Rate Limiting under Shared Reverse Proxies

The login rate limiter tracks failures by source IP. Behind a reverse proxy, all clients share the proxy IP (aardbin does not read `X-Forwarded-For`). This means one user's brute-force attempt locks out all users behind the same proxy. For the intended single-user deployment, this is not a practical concern. See [`SECURITY.md`](SECURITY.md) for mitigation suggestions.

## Not Planned

Multi-user, share links, full-text search, Markdown/rich-text editors, trash, version history, attachment encryption, WebSocket, Redis, PostgreSQL, etc. See [`docs/SPEC.md`](docs/SPEC.md) §38 for the full list.

---
