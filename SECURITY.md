# Security Policy / 安全策略

## Supported Versions

Only the latest release receives security updates.

## Reporting a Vulnerability

**Do not open a public issue for security vulnerabilities.**

Please email **aardpro@proton.me** (or use GitHub's [private vulnerability reporting](https://github.com/aardpro/aardbin/security/advisories/new)) with:

- A description of the vulnerability
- Steps to reproduce
- Potential impact

We will acknowledge receipt within 48 hours and provide a timeline for a fix.

## Known Limitations

### Rate Limiting under Shared Reverse Proxies

aardbin's login rate limiter tracks failures by source IP. When deployed behind a reverse proxy (Nginx, Caddy, Traefik, etc.) that does not forward the real client IP via `X-Forwarded-For` (which aardbin does **not** read for security reasons), all users share the proxy's IP. This means:

- A brute-force attempt from any user locks out **all** users behind the same proxy.
- There is no per-user isolation at the application layer.

**Mitigation:**
- Deploy aardbin behind a reverse proxy that terminates HTTPS and forwards the real client IP.
- Apply rate limiting at the reverse proxy level (e.g., Nginx `limit_req`, Caddy `rate_limit`) per client IP.
- For single-user deployments (the intended use case), this is not a practical concern.

## Security Model

- Text (title + content) is encrypted at rest with AES-256-GCM. The `CRYPTO_KEY` exists only in environment variables / process memory.
- Attachments are stored **unencrypted** on disk.
- Session cookies are HttpOnly, signed with HMAC-SHA256 derived from `ACCESS_KEY`, and survive server restarts (stateless).
- Changing `ACCESS_KEY` invalidates all existing sessions immediately.
- Losing `CRYPTO_KEY` means **all encrypted text is permanently unrecoverable**.

---

## 中文

### 反代限流限制

aardbin 的登录限流按来源 IP 计数。当部署在反向代理（Nginx、Caddy、Traefik 等）后面时，如果代理不转发真实客户端 IP（aardbin 出于安全考虑**不读取** `X-Forwarded-For`），则所有用户共享代理 IP：

- 任一用户的爆破尝试会锁死同一代理后的**所有**用户。
- 应用层无单用户隔离。

**缓解建议：**
- 在反代层面按客户端 IP 做限流（如 Nginx `limit_req`、Caddy `rate_limit`）。
- 单用户部署场景（即设计目标）下此问题无实际影响。

### 安全模型

- 文本（标题 + 正文）使用 AES-256-GCM 静态加密，`CRYPTO_KEY` 仅存于环境变量 / 进程内存。
- 附件**明文**存储。
- 会话 Cookie 为 HttpOnly、HMAC-SHA256 签名、无状态（重启不失效）。
- 更换 `ACCESS_KEY` 立即使所有现有会话失效。
- 丢失 `CRYPTO_KEY` 意味着**所有加密文本永久不可恢复**。
