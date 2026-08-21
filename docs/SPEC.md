# aardbin 设计规格文档（SPEC）

> **版本**：v1.0.0
>
> 本文档是 aardbin 的开发面设计规格，从原始 PRD 迁入并保留中文。
> 延期项与未来迭代清单见本文 §42；用户面上手文档见 [README.md](../README.md)。

---

# 1. 产品概述

## 1.1 产品定位

**aardbin** 是一个私有化部署、单用户、网页式的个人文本与小附件共享工具。

用户可以将文本、代码片段、配置、截图以及小文件保存到自己的服务器，并通过 PC、手机、平板等多个浏览器访问，实现个人多设备之间的快速同步与传递。

核心特征：

- 私有化部署
- 单用户 / 单租户
- Docker 一键部署
- 浏览器直接使用，无客户端
- 服务端渲染 + HTMX
- 多客户端实时同步
- 文本标题与正文 AES-256-GCM 加密存储
- 附件原始文件明文存储
- SQLite 作为极简本地数据库
- 所有数据均位于可挂载 `data/` 目录
- 无外部数据库、Redis、消息队列等依赖

产品目标不是成为 Evernote、Notion 或网盘，而是成为一个：

> **“部署后打开网页，就可以在自己的设备之间快速丢东西”的私人 Bin。**

---

# 2. 核心设计原则

## 2.1 极简单体架构

aardbin 采用：

```text
Browser
   │
   │ HTTP / SSE
   ▼
┌────────────────────────────┐
│        aardbin              │
│                            │
│ Axum                       │
│ ├─ Authentication          │
│ ├─ HTML Rendering          │
│ ├─ HTMX Endpoints          │
│ ├─ SSE Event Bus           │
│ ├─ AES-256-GCM             │
│ └─ SQLite / File I/O       │
│                            │
└──────────────┬─────────────┘
               │
       ┌───────┴────────┐
       ▼                ▼
   SQLite             attachments/
   aardbin.db         UUID files
```

运行时不依赖：

- PostgreSQL
- MySQL
- Redis
- MongoDB
- Kafka
- RabbitMQ
- Node.js
- 外部 API
- CDN

---

## 2.2 SQLite 是“嵌入式存储”，不是外部数据库

SQLite 不被视为传统意义上的“数据库服务”。

它只是：

```text
data/aardbin.db
```

一个普通文件。

因此 aardbin 仍然保持：

> **单容器 + 单进程 + 单 SQLite 文件 + 一个附件目录**

的极简架构。

引入 SQLite 的主要目的不是增加复杂度，而是避免纯文件存储在记录数量增加后出现：

- 每次列表都扫描目录
- 文件名排序困难
- 分页需要读取大量文件
- 并发修改容易产生竞态
- 附件引用关系难以维护
- 删除与孤儿附件清理复杂
- 查询和统计越来越麻烦

---

# 3. 技术架构

## 3.1 固定技术栈

| 模块 | 选型 | 说明 |
|---|---|---|
| 后端语言 | Rust | 单二进制、低资源、稳定 |
| Web Framework | Axum | HTTP / SSE / Middleware |
| HTML | Server-Side Rendering | 服务端直接生成 HTML |
| 交互 | HTMX | 局部页面更新 |
| 实时同步 | SSE | 多客户端变更通知 |
| CSS | Tailwind CSS | 构建期生成，不使用 CDN |
| 数据库 | SQLite | 嵌入式单文件数据库 |
| 加密 | AES-256-GCM | 标题、正文加密 |
| 文件存储 | Local Filesystem | 附件明文存储 |
| 部署 | Docker / Docker Compose | 单容器 |
| 构建 | Multi-stage Docker Build | 最终镜像不需要 Node.js |

---

# 4. 前端资源策略

## 4.1 不使用 Tailwind CSS CDN

生产环境禁止依赖 Tailwind CDN。

原因：

1. aardbin 是私有化部署产品，不应该依赖公网 CDN。
2. 用户可能部署在完全离线或受限网络环境。
3. CDN 增加运行时外部依赖。
4. Tailwind 构建后只需要最终使用到的 CSS，体积可以非常小。
5. Docker 构建阶段已经可以完成 CSS 生成。

因此：

```text
Tailwind Source
      │
      ▼
Tailwind CLI
      │
      ▼
dist/assets/app.css
```

最终运行容器：

```text
Rust binary
HTML templates
HTMX
compiled CSS
```

不需要 Node.js 运行时。

## 4.2 Tailwind 构建方式

建议采用 Tailwind CSS v4 CLI。

Docker 使用 multi-stage build：

```text
Stage 1: frontend-build
  Node.js
  Tailwind CLI
      ↓
  app.css

Stage 2: rust-build
  Rust
      ↓
  aardbin binary

Stage 3: runtime
  aardbin binary
  templates/
  static/
  data/
```

最终 runtime image 不包含 Node.js。

---

# 5. 数据存储设计

## 5.1 总体目录结构

```text
data/
├── aardbin.db
└── attachments/
    ├── {UUID}
    ├── {UUID}
    └── {UUID}
```

其中：

- `aardbin.db`：记录 metadata、加密文本、附件 metadata
- `attachments/`：附件原始文件

---

# 6. SQLite 数据模型

## 6.1 records

```sql
CREATE TABLE records (
    id TEXT PRIMARY KEY,
    encrypted_content BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_records_updated_at
ON records(updated_at DESC);
```

### encrypted_content

保存：

```text
Nonce + AES-256-GCM ciphertext
```

解密后：

```json
{
  "title": "标题",
  "content": "正文"
}
```

标题与正文始终一起加密。

---

# 6.2 attachments

```sql
CREATE TABLE attachments (
    id TEXT PRIMARY KEY,
    record_id TEXT NOT NULL,
    original_filename TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    mime_type TEXT NOT NULL,
    created_at INTEGER NOT NULL,

    FOREIGN KEY(record_id)
        REFERENCES records(id)
        ON DELETE CASCADE
);

CREATE INDEX idx_attachments_record_id
ON attachments(record_id);
```

附件文件：

```text
data/attachments/{attachment_id}
```

数据库只保存：

- UUID
- 所属记录
- 原始文件名
- 文件大小
- MIME Type
- 创建时间

真正的文件内容存放在 filesystem。

---

# 6.3 为什么附件 metadata 不放进加密 JSON

原设计将 attachments 数组全部放在加密记录中。

这样虽然简单，但会导致：

- 列表查询必须解密记录
- 删除附件需要读取、修改、重新加密整个记录
- SQLite 无法直接维护附件关系
- 后续附件查询不方便

因此优化为：

```text
SQLite
├── record
│   └── encrypted title/content
│
└── attachment metadata
    └── UUID / filename / size / MIME
```

附件本身：

```text
filesystem
└── UUID file
```

这样依然保持极简，但数据关系清晰很多。

---

# 7. 数据安全模型

## 7.1 ACCESS_KEY

ACCESS_KEY 是系统访问凭证。

推荐生成一个高熵随机字符串，而不是依赖用户自己设置简单密码。

例如：

```text
32 bytes random
→ Base64URL
```

约 43 个字符。

服务端：

```text
ACCESS_KEY
```

只存在环境变量或 Secret 中。

---

## 7.2 登录后的 Session

**不再将 ACCESS_KEY 保存到 LocalStorage。**

登录流程：

```text
Browser
   │
   │ POST /login
   │ ACCESS_KEY
   ▼
Server
   │
   │ 校验 ACCESS_KEY
   ▼
HttpOnly Session Cookie
```

Session Cookie：

```text
HttpOnly
Secure
SameSite=Lax
```

后续普通请求与 SSE 请求自动携带 Cookie。

这样：

- JavaScript 无法读取 ACCESS_KEY
- SSE 不需要 query string 携带 ACCESS_KEY
- 不会因为 URL 日志记录泄露 ACCESS_KEY
- 登录态与真正的 ACCESS_KEY 解耦

### 7.2.1 Session 实现机制（无状态签名 Cookie）

aardbin 是单用户系统，**不需要服务端 Session 存储**（不引入 sessions 表，也不使用内存 Map）。

Session Cookie 采用 **HMAC 签名的自包含令牌**：

```text
签名密钥:
  SESSION_KEY = SHA-256("aardbin-session-v1:" + ACCESS_KEY)

Cookie 值:
  v1.{expiry_unix}.{hmac_hex}

  其中 hmac_hex = HMAC-SHA256(SESSION_KEY, "v1.{expiry_unix}")
```

校验规则：

1. 拆分出 `expiry_unix` 与 `hmac_hex`
2. 用常量时间比较校验 HMAC
3. 校验 `expiry_unix > now`

该设计的特性：

- **重启不失效**：无服务端状态，进程重启后已登录客户端不受影响
- **更换 ACCESS_KEY 即全部失效**：签名密钥派生自 ACCESS_KEY
- **无法伪造**：不知道 ACCESS_KEY 就无法构造合法签名
- **零存储成本**

### 7.2.2 Secure 属性与纯 HTTP 部署

Cookie 默认带 `Secure`（仅 HTTPS 发送）。但 aardbin 允许部署在可信内网纯 HTTP 环境（见 §37），此时 `Secure` 会导致浏览器拒绝携带 Cookie、登录永远失效。

因此增加环境变量：

```text
COOKIE_SECURE=true   # 默认，公网 / HTTPS 反代场景
COOKIE_SECURE=false  # 仅可信内网纯 HTTP 场景
```

`HttpOnly`、`SameSite=Lax` 始终开启，不可配置。

---

## 7.3 Session 失效

Session 应设置有限有效期。

建议：

```text
SESSION_TTL=7d
```

退出登录时：

```text
Set-Cookie: session=; Max-Age=0
```

同时服务端拒绝无效 Session。

---

## 7.4 登录限流

`POST /login` 必须限流，防止在线爆破 ACCESS_KEY。

规则（进程内存实现，无需持久化）：

```text
同一来源 IP：
  5 分钟内累计失败 5 次
    ↓
  返回 429 Too Many Requests
  锁定期 = 剩余窗口时间
```

- 登录成功立即清零该 IP 计数
- 仅统计**失败**尝试
- 滑动窗口，窗口长度 5 分钟
- 进程重启后计数清零（可接受）

限流响应页面应明确提示：

```text
Too many attempts. Try again in N minutes.
```

---

## 7.5 CSRF / Origin 校验

`SameSite=Lax` 已能阻止绝大多数跨站 POST 携带 Cookie。作为纵深防御，服务端对所有 **POST 请求**追加校验：

1. 若请求带 `Origin` 头：其 host 必须与 `Host` 头一致，否则 403
2. 若请求带 `Sec-Fetch-Site` 头：值必须属于 `{same-origin, same-site, none}`，否则 403

不引入 CSRF Token，保持表单与 HTMX 实现简单。

---

# 8. CRYPTO_KEY

## 8.1 规格

```text
64-character hexadecimal
```

对应：

```text
32 bytes
```

即 AES-256。

---

## 8.2 加密算法

使用：

```text
AES-256-GCM
```

每次保存记录：

1. 生成新的随机 96-bit nonce
2. 加密 title + content
3. 保存：

```text
nonce + ciphertext + authentication tag
```

GCM 提供：

- 机密性
- 完整性校验
- 防止密文被无感篡改

---

## 8.3 CRYPTO_KEY 不落盘

CRYPTO_KEY：

```text
Environment / Docker Secret
        ↓
Rust process memory
```

不保存到：

- SQLite
- data/
- 浏览器
- Cookie
- HTML

如果 CRYPTO_KEY 丢失：

> 所有历史文本记录无法恢复。

---

## 8.4 解密失败的降级行为

启动时**不校验** CRYPTO_KEY 是否能解密现有数据（避免全量解密扫描），但运行时解密失败必须优雅降级：

- 列表页中无法解密的记录显示为占位条目：

  ```text
  ⚠ Unable to decrypt
  （该记录的附件仍可下载、记录可删除）
  ```

- 不允许对解密失败的记录执行 Edit / Copy（按钮隐藏）
- 不允许覆盖保存解密失败的记录，防止误操作导致数据被重加密覆盖

典型触发场景：CRYPTO_KEY 配置错误、密文被外部篡改。

---

# 9. 附件安全模型

附件明确采用：

> **明文存储。**

例如：

```text
data/attachments/550e8400-e29b-41d4-a716-446655440000
```

磁盘文件没有扩展名。

用户原始文件名仅作为 metadata 保存。

---

## 9.1 附件访问

禁止：

```text
/static/attachments/foo.png
```

直接映射文件系统。

统一通过：

```text
GET /attachments/{uuid}
```

后端：

1. 校验 Session
2. 查询 attachment UUID
3. 确认 attachment 存在
4. 打开对应文件
5. 根据数据库 metadata 设置 Content-Type
6. 根据 original_filename 设置 Content-Disposition
7. 流式返回

用户提供的 filename 永远不能直接参与 filesystem path 拼接。

### 9.1.1 响应头策略

**Content-Disposition**：默认强制下载，防止 HTML/SVG 等可执行内容附件被浏览器内联渲染造成 XSS：

```text
Content-Disposition: attachment
```

仅当请求显式带 `?inline=1` **且** MIME 属于安全图片白名单时允许内联：

```text
image/png, image/jpeg, image/gif, image/webp
```

**文件名编码**：`original_filename` 可能包含中文等非 ASCII 字符，必须使用 RFC 5987 编码：

```text
Content-Disposition: attachment; filename*=UTF-8''<percent-encoded>
```

同时提供 sanitized 的 ASCII `filename="..."` 作为旧客户端 fallback（非 ASCII 字符替换为 `_`，`\` `"` 控制字符剔除）。

**其他必带头**：

```text
X-Content-Type-Options: nosniff
Content-Length: <size_bytes>
```

---

# 10. 记录列表设计

这是 aardbin 使用频率最高的界面，因此列表应该优先考虑：

> **信息密度 + 快速扫描 + 手机可用**

而不是传统的大卡片网格。

---

## 10.1 推荐列表结构

每条记录：

```text
┌──────────────────────────────────────────┐
│ Docker 部署命令                           │
│ 2 minutes ago                            │
│ docker compose up -d --build             │
│                                          │
│ 📎 2 attachments                         │
│                                          │
│             Copy   Edit   Delete         │
└──────────────────────────────────────────┘
```

---

## 10.2 有标题的记录

显示：

```text
标题
更新时间
正文摘要
附件信息
操作
```

例如：

```text
Cloudflare SMTP 配置
今天 17:22
smtpdm.aliyun.com / port 465 ...
📎 2
```

---

## 10.3 没有标题的记录

如果 title 为空：

> 自动使用正文第一行作为标题候选。

例如正文：

```text
docker compose restart aardbin
```

列表显示：

```text
docker compose restart aardbin
今天 17:20
docker compose restart aardbin
```

如果正文也是空：

```text
Untitled
今天 17:18
📎 3
```

建议 `Untitled` 使用弱化视觉样式。

---

# 11. 附件在列表中的表达

附件不要只显示一个难以理解的数字。

建议：

```text
📎 2
```

有空间时可以进一步显示：

```text
📎 screenshot.png +1
```

例如：

```text
Cloudflare 配置
今天 17:20

DNS 配置完成，需要继续配置 SMTP。

📎 screenshot.png +1
```

移动端可以缩短为：

```text
📎 2
```

---

# 12. 时间显示

数据库统一保存 Unix timestamp 或 UTC timestamp。

前端显示使用相对时间：

```text
刚刚
2 分钟前
1 小时前
昨天 21:32
8 月 18 日
```

当用户需要精确时间时，可以通过 `title` 属性或详情页显示：

```text
2026-08-20 17:22:31
```

排序始终依据：

```text
updated_at DESC
```

而不是字符串排序。

---

# 13. 大量记录时的分页

SQLite 负责分页。

默认：

```text
PAGE_SIZE=20
```

例如：

```sql
SELECT ...
FROM records
ORDER BY updated_at DESC
LIMIT 20 OFFSET 40;
```

对于 MVP，传统分页足够。

未来如果记录数量达到非常大，可以切换为 cursor pagination：

```text
updated_at + id
```

但 MVP 不需要提前复杂化。

---

# 14. 记录管理

## 14.1 新建

支持：

- 标题
- 正文
- 多附件

全部可选。

因此允许：

```text
空标题
空正文
有附件
```

也允许：

```text
有标题
空正文
无附件
```

甚至允许完全空记录。

---

## 14.2 编辑

支持：

- 修改标题
- 修改正文
- 新增附件
- 删除附件

保存时：

```text
UPDATE records
UPDATE attachments
```

最终发送一次：

```text
data_changed
```

事件。

**updated_at 规则**：新增附件、删除附件都会 bump 所属记录的 `updated_at`（列表排序与 SSE 刷新以记录为粒度，附件变化本质上是记录变化）。

---

# 15. 复制文本

点击：

```text
Copy
```

浏览器调用（见 §30 路由表）：

```text
GET /records/:id/copy
```

服务端读取并解密正文，以 `text/plain; charset=utf-8` 返回。

浏览器：

```javascript
navigator.clipboard.writeText(content)
```

只有用户主动点击时才调用 Clipboard API。

成功：

```text
Copied
```

失败：

```text
Copy failed
```

解密失败的记录不渲染 Copy 按钮（见 §8.4）。

---

# 16. 删除记录

删除前：

```text
Delete this record?
```

确认后：

1. 删除数据库 record
2. ON DELETE CASCADE 删除 attachment metadata
3. 删除对应 attachment files
4. 广播 `data_changed`

删除操作为：

> **物理删除，无回收站。**

---

# 17. 附件限制

默认：

```text
MAX_ATTACHMENT_BYTES=2097152
```

即 2 MiB。

前端：

- 提前检查
- 给用户即时提示

后端：

- 必须再次检查
- 永远不能信任前端限制

---

## 17.1 单记录附件数量

不设置固定数量限制。

限制来源：

```text
filesystem capacity
```

但建议后续可以增加：

```text
MAX_ATTACHMENTS_PER_RECORD
```

作为可选环境变量。

MVP 不需要。

---

## 17.2 其他体积限制

除单附件大小外，还需要以下限制：

| 限制 | 环境变量 | 默认值 | 说明 |
|---|---|---:|---|
| 单附件 | `MAX_ATTACHMENT_BYTES` | 2097152（2 MiB） | 每个文件 |
| 请求体总量 | `MAX_REQUEST_BYTES` | 8388608（8 MiB） | 覆盖整个 multipart 表单（含所有附件 + 文本字段），同时作为 HTTP body limit |
| 正文 | `MAX_CONTENT_BYTES` | 1048576（1 MiB） | content UTF-8 字节数 |
| 标题 | 固定 | 200 字符 | title 字符数，超出截断并提示 |

- 请求到达时先检查 `Content-Length`（若存在）快速拒绝超限请求
- 流式处理 multipart 时仍须累计计数，不信任客户端声明的长度
- 任一限制触发：返回明确错误提示，本次写入全部回滚（记录与已落盘的临时文件）

---

# 18. 实时同步

## 18.1 SSE

每个登录浏览器建立：

```text
GET /events
```

连接。

Session 通过 HttpOnly Cookie 自动发送。

服务器维护：

```text
broadcast channel
```

所有 SSE client 订阅该 channel。

---

## 18.2 事件

MVP 只定义一种：

```text
event: data_changed
data: {}
```

或者：

```json
{
  "type": "data_changed"
}
```

不发送业务数据。

---

## 18.3 SSE 心跳

反向代理（Nginx / Caddy / Traefik）通常会断开长时间空闲的 SSE 连接，导致实时同步悄悄失效。

服务端每 **25 秒**向每个 SSE 连接发送注释心跳：

```text
: ping

```

心跳不触发客户端任何业务行为，仅用于保活。浏览器 EventSource 自动处理断线重连。

# 19. 实时同步行为

例如：

```text
PC A
  │
  │ 新建 Record
  ▼
aardbin
  │
  ├── SQLite INSERT
  ├── attachment write
  └── broadcast(data_changed)
       │
       ├── PC A
       ├── PC B
       └── Phone
```

其他客户端收到事件后：

```text
HTMX reload list
```

只刷新列表区域。

---

# 20. SSE 断线重连

浏览器断网：

```text
SSE disconnected
```

网络恢复后：

```text
EventSource reconnect
```

客户端重新连接。

由于事件本身只表示：

```text
数据发生变化
```

而不是业务数据，因此丢失某一个 SSE event 也没有关系。

重新连接后客户端可以重新加载当前列表。

---

# 21. 并发修改规则

aardbin 是单用户系统，但可能存在：

```text
PC
Phone
Tablet
```

同时修改。

MVP 不实现复杂的冲突合并。

规则：

> **最后一次成功保存的数据为最终状态。**

例如：

```text
PC A 打开记录
PC B 修改记录并保存
PC A 使用旧内容继续编辑并保存
```

最终：

```text
PC A 的最后一次保存覆盖 PC B
```

其他客户端收到 `data_changed` 后重新加载列表。

正在编辑的表单不自动覆盖用户当前输入。

---

# 22. UI / UX

## 22.1 整体风格

关键词：

- 极简
- 紧凑
- 快速
- 无复杂动画
- 无 dashboard
- 无侧边栏
- 无复杂导航

aardbin 应该更接近：

```text
Pastebin + 手机备忘录 + 私有文件投递箱
```

而不是：

```text
Notion / Linear / Slack
```

---

# 23. 登录页面

桌面：

```text
             aardbin

       ┌──────────────────┐
       │ Access Key       │
       └──────────────────┘
             Login
```

手机：

全宽卡片。

只包含：

- Access Key
- Login
- 错误提示

---

# 24. 主列表页面

顶部：

```text
aardbin                         + New    Logout
```

下面：

```text
Search（未来）
```

MVP 可以暂时不提供搜索。

记录：

```text
┌────────────────────────────────────┐
│ Docker deployment                  │
│ 3 minutes ago                      │
│ docker compose up -d               │
│                                    │
│ 📎 1                               │
│                                    │
│ Copy     Edit     Delete           │
└────────────────────────────────────┘
```

---

# 25. 响应式布局

## Desktop

宽屏使用较高的信息密度。

可以采用：

```text
2-column card grid
```

但不要强制卡片高度。

正文摘要根据空间截断。

---

## Tablet

```text
1~2 columns
```

根据 viewport 自动决定。

---

## Mobile

严格：

```text
1 column
```

操作按钮保证：

- 足够点击面积
- 不需要精确点击小图标
- 不出现横向滚动

---

# 26. 新建 / 编辑页面

结构：

```text
Title
┌───────────────────────────┐
│                           │
└───────────────────────────┘

Content
┌───────────────────────────┐
│                           │
│                           │
│                           │
└───────────────────────────┘

Attachments
┌───────────────────────────┐
│ Drop files here           │
│ or click to select        │
└───────────────────────────┘

📎 screenshot.png   234 KB   Download  ×
📎 config.txt        12 KB   Download  ×

             Cancel   Save
```

---

# 27. HTMX 使用原则

HTMX 负责：

- 创建记录
- 编辑记录
- 删除记录
- 分页
- 表单提交
- 局部刷新
- Toast
- Modal
- 附件列表更新

尽可能避免编写大型 SPA。

---

# 28. JavaScript 使用原则

不追求：

> “零 JavaScript”。

而是：

> **JavaScript 只负责浏览器必须负责的事情。**

例如：

- Clipboard API
- 文件拖拽
- SSE/EventSource
- 少量 UI 行为

业务状态尽量由服务器控制。

---

# 29. 鉴权流程

```text
GET /
   │
   ├── valid session
   │       ↓
   │     records
   │
   └── no session
           ↓
         /login
```

登录：

```text
POST /login
   │
   ├── invalid
   │     ↓
   │   303 → /login?error=1
   │
   └── valid
         ↓
   Set-Cookie: session=...
         ↓
   303 → /
```

登录成功/失败均使用 **303 See Other** 重定向（PRG 模式），避免浏览器刷新重复提交表单。

---

# 30. API / Route 设计

建议保持简单。

```text
GET    /login
POST   /login
POST   /logout

GET    /
GET    /records                          ?page=N（HTMX 分页局部刷新，默认 page=1）
GET    /records/new
POST   /records                          multipart 表单（title / content / files）
GET    /records/:id/edit
POST   /records/:id                      multipart 表单（同上）
POST   /records/:id/delete

GET    /records/:id/copy                 返回 text/plain 解密正文（§15）
POST   /records/:id/attachments/:aid/delete   删除单个附件（§14.2）

GET    /attachments/:id                  下载（§9.1），?inline=1 图片白名单内联

GET    /events                           SSE（§18）

GET    /healthz                          无需登录，返回 200 "ok"（健康检查）

GET    /static/*                         css / js / htmx，无需登录（登录页需要样式）
```

除 `/login`、`/healthz`、`/static/*` 外，所有路由必须携带有效 Session。

不需要为了 REST API 而设计大量 JSON API。

aardbin 是：

> Server-rendered web application

不是：

> SPA + REST backend。

---

# 31. 数据一致性

SQLite：

```text
WAL mode
```

推荐：

```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
```

这样可以改善：

- 读写并发
- SSE 长连接情况下的数据库访问
- 多请求同时访问

## 31.1 Schema Migration

启动时执行内置迁移，使用 SQLite `PRAGMA user_version` 记录版本：

```text
user_version = 0  →  执行 v1 schema（§6.1 / §6.2），置 user_version = 1
user_version = 1  →  当前版本，跳过
```

迁移在进程启动、接受任何请求之前完成。新增 schema 变更时递增版本号，迁移只增不删。

---

# 32. 附件与 SQLite 的一致性

附件文件与数据库无法天然做到真正的 ACID 原子事务，因此上传采用：

```text
1. 生成 UUID
2. 写入临时文件
3. fsync / close
4. rename 为最终 UUID 文件
5. SQLite INSERT attachment
6. Commit
7. broadcast
```

如果中途失败：

- 数据库没有 attachment metadata
- 文件可能成为 orphan

因此服务启动时可以进行 orphan scan。

---

# 33. 孤儿附件管理

启动时可以执行：

```text
scan attachments/
```

查询：

```text
SELECT id FROM attachments
```

找出：

```text
filesystem file
    ↓
不存在于 SQLite
```

则认为是 orphan。

MVP：

> 不自动删除。

仅记录 warning。

未来提供：

```text
aardbin doctor
```

或者：

```text
/admin/storage
```

进行清理。

这样可以避免因为异常退出导致的数据误删。

---

# 34. 数据备份

所有持久化数据：

```text
data/
├── aardbin.db
└── attachments/
```

均需要备份。

但是由于 SQLite 运行时可能处于 WAL 状态：

> 不建议简单地在数据库写入过程中复制单个 `.db` 文件作为正式备份。

推荐：

```text
SQLite backup API
+
attachments/
```

或者在停止容器后：

```text
backup data/
```

MVP 可以提供：

```text
docker compose stop
tar data/
docker compose start
```

作为最简单的可靠备份方式。

---

# 35. Docker 部署

## 35.1 Runtime

最终容器只需要：

```text
aardbin
templates/
static/
data/
```

不需要：

- Node.js
- npm
- Cargo
- Rust compiler

---

## 35.2 Volume

```yaml
volumes:
  - ./data:/app/data
```

---

# 36. 环境变量

## 必填

```text
ACCESS_KEY
CRYPTO_KEY
```

---

## 可选

| 变量 | 默认值 | 说明 |
|---|---:|---|
| `MAX_ATTACHMENT_BYTES` | `2097152` | 单附件最大字节数 |
| `MAX_REQUEST_BYTES` | `8388608` | 单请求体总字节数（含全部附件与文本字段） |
| `MAX_CONTENT_BYTES` | `1048576` | 正文最大 UTF-8 字节数 |
| `PAGE_SIZE` | `20` | 每页记录数量 |
| `SESSION_TTL` | `7d` | 登录 Session 有效期，如 `7d` / `12h` / `30m` |
| `COOKIE_SECURE` | `true` | Session Cookie 是否带 `Secure`；纯 HTTP 内网部署设为 `false`（§7.2.2） |
| `LISTEN_ADDR` | `0.0.0.0:8080` | HTTP 监听地址 |
| `DATA_DIR` | `./data` | SQLite 与附件根目录 |

---

# 37. HTTPS

aardbin 本身 MVP 不负责 TLS。

推荐：

```text
Internet
   ↓
Caddy / Nginx / Traefik
   ↓ HTTPS
aardbin
   ↓ HTTP
localhost:8080
```

如果仅在可信内网：

```text
Browser
   ↓
HTTP
   ↓
aardbin
```

也可以。

但如果跨公网：

> **必须使用 HTTPS。**

特别是登录时 ACCESS_KEY 会通过 HTTPS 发送到服务器。

---

# 38. 明确不做

MVP 不实现：

- 多用户
- 多租户
- 用户注册
- 权限系统
- 公网匿名分享
- 分享链接
- 全局剪贴板监听
- 大文件存储
- 附件加密
- 回收站
- 版本历史
- 文本全文搜索
- AI 功能
- Markdown 编辑器
- 富文本编辑器
- 在线图片编辑
- 多服务器集群
- Redis
- PostgreSQL
- WebSocket

---

# 39. 为什么不使用 WebSocket

aardbin 的实时需求非常简单：

```text
数据发生变化
        ↓
通知客户端刷新
```

并不需要：

- 双向实时通信
- 客户端发送业务事件
- presence
- typing indicator
- collaborative editing

因此：

> SSE 比 WebSocket 更简单。

---

# 40. 为什么不做客户端加密

当前安全模型：

```text
Client
   ↓ plaintext
HTTPS
   ↓
Server memory
   ↓ AES-256-GCM
SQLite
```

aardbin **不是端到端加密系统**。

服务器运行过程中必须能够：

- 解密文本
- 渲染页面
- 返回复制内容

因此：

> CRYPTO_KEY 能保护“磁盘静态数据”，但不能防止拥有服务器运行权限的人读取明文。

这是产品明确的安全边界。

---

# 41. MVP 验收标准

## AC-01 登录

- 未登录访问 `/` 自动进入登录页
- 错误 ACCESS_KEY 无法登录
- 正确 ACCESS_KEY 登录成功
- 登录后获得 HttpOnly Session Cookie
- ACCESS_KEY 不写入 LocalStorage
- Logout 后 Session 失效

---

## AC-02 创建记录

创建：

```text
title
content
attachment
```

保存成功。

验证：

```text
SQLite
  ↓
record exists

data/attachments/
  ↓
UUID file exists
```

---

## AC-03 文本加密

直接查看：

```text
data/aardbin.db
```

无法直接读出：

```text
title
content
```

记录 ciphertext 使用 AES-256-GCM。

---

## AC-04 复制

点击 Copy：

```text
Clipboard
=
record.content
```

内容完全一致。

---

## AC-05 附件下载

下载文件：

```text
downloaded bytes
=
uploaded bytes
```

原始文件名正确。

---

## AC-06 手机端

手机浏览器打开：

- 登录
- 查看列表
- 新建
- 编辑
- 删除
- 上传
- 下载
- 复制

全部可用。

---

## AC-07 多客户端实时同步

打开：

```text
Browser A
Browser B
Phone
```

A：

```text
Create
Edit
Delete
```

B / Phone：

> 无需手动刷新，列表自动更新。

---

## AC-08 分页

创建超过：

```text
PAGE_SIZE
```

数量的记录。

确认：

- 正确分页
- updated_at DESC
- 页面不会扫描全部记录文件

---

## AC-09 附件删除

删除 attachment：

```text
SQLite metadata
+
filesystem file
```

均消失。

---

## AC-10 删除记录

删除 record：

```text
records row
+
attachment rows
+
attachment files
```

全部删除。

---

## AC-11 孤儿文件安全

人为制造：

```text
data/attachments/orphan
```

aardbin 不应把它暴露给用户。

doctor / startup scan 能识别 orphan。

---

## AC-12 安全基线

- 连续 5 次错误 ACCESS_KEY 后，`POST /login` 返回 429
- 带跨站 `Origin` / `Sec-Fetch-Site: cross-site` 的 POST 请求被拒绝（403）
- `COOKIE_SECURE=true` 时 Cookie 带 `Secure`；`false` 时不带，且两种模式下 HTTP 部署均可完成登录
- 上传 `x.html` 附件后下载，响应 `Content-Disposition` 为 `attachment`；`?inline=1` 对非白名单 MIME 无效
- 中文文件名附件下载时 `Content-Disposition` 使用 `filename*=UTF-8''...` 编码

---

## AC-13 解密失败降级

使用错误的 CRYPTO_KEY 启动：

- 服务正常启动，列表可访问
- 历史记录显示 `Unable to decrypt` 占位
- 占位记录无 Edit / Copy 按钮
- 记录仍可删除、附件仍可下载

---

## AC-14 健康检查与心跳

- `GET /healthz` 未登录也返回 200
- SSE 连接 25 秒内收到 `: ping` 心跳

---

# 42. 未来迭代

延期项与未来迭代清单：

1. **MCP server** — 为 aardbin 提供 MCP server，让 AI 助手直接读写记录（P3，依赖 JSON API）
2. **深色主题** — 深色/浅色主题切换（P3）
3. **PWA** — manifest + service worker，可安装到手机桌面（P3）
4. **粘贴直达建记录** — 列表页直接 Ctrl+V 文本即建记录（P3）
5. **快捷键** — `/` 新建、`c` 复制等键盘操作（P3）
6. **SPEC 英文版** — 等首个外部贡献者或 API 消费者信号出现再译（P3）

---

# 43. 最终架构结论

aardbin 最终保持：

```text
                Browser
                   │
             HTTPS / HTTP
                   │
             ┌─────▼─────┐
             │   Axum    │
             │           │
             │ HTMX      │
             │ SSE       │
             │ Auth      │
             │ AES-GCM   │
             └─────┬─────┘
                   │
          ┌────────┴────────┐
          │                 │
     SQLite DB        Attachments
          │                 │
   encrypted text       plaintext
   metadata              files
```

最终产品原则：

> **一个 Rust 二进制、一个 SQLite 文件、一个附件目录、一个 Docker 容器。**

没有外部数据库，没有 Redis，没有 Node runtime，没有 CDN，没有 SPA，没有复杂 API。

同时通过 SQLite 解决纯文件存储在记录数量增长后的分页、排序、关联和一致性问题；通过 Tailwind 构建期编译解决 CDN 依赖；通过 HttpOnly Session Cookie 解决 ACCESS_KEY 暴露与 SSE 鉴权问题；通过 SSE + HTMX 保持多客户端实时同步。

这比原 v1.0 更适合作为真正可以长期运行的 aardbin MVP。