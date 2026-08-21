# aardbin

私有化部署、单用户、网页式的个人文本与小附件共享工具 —— 一个 Rust 二进制 + 一个 SQLite 文件 + 一个附件目录 + 一个 Docker 容器。

部署后打开网页，即可在自己的设备（PC / 手机 / 平板）之间快速丢文本、代码、配置、截图与小文件。

> 完整设计见 [`PRD.md`](PRD.md)（v1.1）。

## 特性

- **私有化单用户**：无多用户 / 注册 / 权限系统
- **极简架构**：Rust + Axum + HTMX + SSE，无 Node 运行时、无 Redis、无外部数据库、无 CDN
- **加密静态存储**：标题与正文用 AES-256-GCM 加密落盘（`CRYPTO_KEY` 只存在环境变量中）
- **附件明文存储**：小文件（默认 ≤ 2 MiB/个）走 UUID 落盘，元数据入 SQLite
- **多客户端实时同步**：SSE 广播 `data_changed`，HTMX 局部刷新列表
- **HttpOnly 签名会话**：无状态 HMAC Cookie，重启不掉线，换 `ACCESS_KEY` 全部失效
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

- **CRYPTO_KEY 一旦丢失，所有历史文本记录无法恢复**。请务必妥善备份 `.env`。
- **换 CRYPTO_KEY 不会自动重加密**旧记录；旧记录会显示为 `Unable to decrypt`（附件仍可下载、记录可删除）。
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

## 本地开发

```bash
npm install                     # 构建期依赖：Tailwind CLI
npm run build:css               # 生成 static/app.css
cargo run                       # 启动（需设置 ACCESS_KEY / CRYPTO_KEY）
cargo test                      # 单元测试
bash scripts/smoke.sh           # 集成冒烟测试（覆盖 PRD AC-01~AC-14）
```

## 数据备份（PRD §34）

所有持久化数据都在 `data/` 下：

```text
data/
├── aardbin.db
└── attachments/
```

最可靠的备份方式（停止容器后整目录打包）：

```bash
docker compose stop
tar -czf backup.tar.gz data/
docker compose start
```

## HTTPS（PRD §37）

aardbin 自身不负责 TLS。跨公网部署时，请在前面挂 Caddy / Nginx / Traefik 终止 HTTPS，再反向代理到 `127.0.0.1:8080`，并保持 `COOKIE_SECURE=true`。

## 安全边界（PRD §40）

aardbin 不是端到端加密系统：`CRYPTO_KEY` 保护的是**磁盘静态数据**，但拥有服务器运行权限的人仍可读取明文。登录限流、SameSite=Lax + Origin 校验、附件 `nosniff` + 强制下载等已内置。

## 明确不做（MVP）

多用户、分享链接、全文搜索、Markdown/富文本编辑器、回收站、版本历史、附件加密、WebSocket、Redis、PostgreSQL 等，详见 PRD §38。
