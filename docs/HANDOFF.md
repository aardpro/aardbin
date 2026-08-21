# aardbin 开源发布 v1.0.0 — 实施交接文档

> 面向：接手开发的外部 agent / 开发者。
> 完整依据链：本文档 → [docs/designs/open-source-launch.md](designs/open-source-launch.md)（三级审查锁定的完整计划）→ [README.md](../README.md)（现状）→ [PRD.md](../PRD.md)（将迁入 SPEC）→ [TODOS.md](../TODOS.md)（延期项）。
> 所有决策已经过 CEO + ENG + Design 三级审查与两轮外部对抗评审，**按本文档执行即可，勿重新讨论已锁定决策**。

## 1. 项目一句话

单用户自托管私人 Bin（pastebin）：Rust + Axum + HTMX + SSE + SQLite + AES-256-GCM，约 2400 行 Rust，本地功能已过（smoke 28 项全绿）。产品差异化 = 「个人多设备同步」，不是公开分享。

## 2. 当前仓库状态（先读，重要）

- Remote：`https://github.com/aardpro/aardbin.git`，分支 `master`，本地仅 2 个提交（PRD.md、CLAUDE.md），**其余文件全部未跟踪（untracked）**
- ⚠️ **deploy/ 是用户的在役部署目录**：`deploy/data/`（真实 SQLite 数据库 + 明文附件）、`deploy/.env`（真实 ACCESS_KEY/CRYPTO_KEY）、`deploy/aardbin-1.1.0.tar.gz`
  - **严禁提交、严禁删除、严禁读取/打印 .env 与数据库内容**——只执行 T1 规定的移出操作
- 本地开发循环：`npm install && npm run build:css && cargo run`（需 ACCESS_KEY/CRYPTO_KEY 环境变量）
- 验证命令：`cargo test`、`bash scripts/smoke.sh`（需 curl + python3）、`docker build .`

## 3. 实施顺序（严格执行，勿打乱）

| Step | 内容 | 任务 |
|---|---|---|
| 0 | 仓库可见性 public、GHCR（packages: write）、Dependabot 启用、branch protection | — |
| 1 | 公开前清理 → **白名单 `git add` 首次提交**（禁止 `git add .`） | T1, T2 |
| 2 | P0 文档 + favicon | T3, T4, T10, T16, T18 |
| 3 | CI + E1 安全硬化 | T5 |
| 4 | 代码修复 | T7, T8, T9, T17 |
| 5 | E2 i18n → 之后 E4（EN 截图依赖 i18n） | T11, T13 |
| 6 | E3 JSON API + CLI | T12 |
| 7 | 发布流水线 + 自身实例迁移 → 全量完成后 tag `v1.0.0` | T6, T14 |

## 4. 任务清单

### 4.1 CEO 审查任务（P1 = v1.0.0 发布前置）

| T# | 任务 | 文件 | 验证 |
|---|---|---|---|
| T1 | 修复 deploy/ 数据泄露：`.gitignore` 加 `deploy/data/`、修正第 8 行误导性注释（「data already covered above」是错的——`/data` 只匹配根目录）、把 deploy/ 下真实数据与 .env **移出仓库目录**（移到仓库外备份位置）、30MB tarball 移出、白名单提交 | .gitignore, deploy/ | `git check-ignore deploy/data/aardbin.db` 命中；`git add .` 干跑不含任何数据文件 |
| T2 | 公开前敏感信息审计：`git log --all -p` + 工作区全文扫描 ACCESS_KEY/CRYPTO_KEY 残留（仓库从未公开→移除即可，无需改写历史） | 全仓库 | 扫描零匹配 |
| T3 | LICENSE（MIT，与 Cargo.toml 一致）+ CONTRIBUTING.md / SECURITY.md（EN 为源语言、双语）/ CODE_OF_CONDUCT / issue+PR 模板 / CHANGELOG + TODOS.md 已在库中 | 仓库根、.github/ | 文件齐备 |
| T4 | **P0 文档**：README 双语（吸收用户上手要点）；PRD.md 迁入 **docs/SPEC.md**（中文保留）；**零「PRD §」悬空引用**（源码注释、smoke.sh 文本、deploy 文件全部改为 README/SPEC 锚点；清点范围含 TODOS.md）；准确性修正包：data/ 目录树补 tmp/、smoke.sh 前置依赖（curl+python3）、RUST_LOG 入环境变量表、版本号统一 v1.0.0、反代限流限制记录（3B） | README.md, PRD.md, scripts/smoke.sh, src/, deploy/ | 源码逐条对照完成；无悬空引用 |
| T5 | CI：GitHub Actions（fmt / clippy / test / smoke.sh / docker build）+ E1（仅 cargo-deny 不用 cargo-audit + Trivy + Dependabot cargo+npm 双生态）；交叉编译用 cargo-zigbuild | .github/ | PR 全绿 |
| T6 | 发布流水线：musl 静态二进制（x86_64/aarch64-unknown-linux-musl）+ buildx 双架构 GHCR `ghcr.io/aardpro/aardbin:1.0.0` + latest；**release tarball 打包 templates/ + static/** | .github/, Cargo.toml | 双架构产物存在；arm64 qemu 冒烟通过 |
| T7 | Save 按钮 `hx-disabled-elt="this"` 防双击 | templates/form.html | 双击不产生重复记录 |
| T8 | SSE `es.onopen` 断线重连后刷新列表 + 文档对齐（原 PRD §20 描述已实现化） | static/app.js | 断网恢复后列表自动刷新 |
| T9 | `update_record` 附件插入失败时回滚记录到旧 blob（handler 层补偿，非事务） | src/routes.rs | 见 E5 drop-table 测试 |
| T10 | README/SECURITY.md 记录反代限流限制（反代 IP 共享致全量锁死，含缓解建议） | README.md, SECURITY.md | 文档含该限制 |
| T11 | E2 i18n（见 §5 决策） | templates/, src/render.rs, static/app.js | 双语言冒烟通过 |
| T12 | E3 JSON API + CLI（见 §5 决策） | src/api.rs, src/cli/, src/main.rs, README.md | 六条路径测试通过 |
| T13 | E4：awesome-selfhosted **先开 issue**（发布时），+4 个月后投收录 PR；assets/ 放 EN 截图/录屏 | assets/ | issue 已开；素材就绪 |
| T14 | 版本号统一 v1.0.0（Cargo.toml、deploy compose 镜像 tag、tarball 命名——1.1.0 从未公开，rebase 后旧号废弃，不视为回退）+ 自身在役实例迁移到 GHCR 镜像（dogfooding） | Cargo.toml, deploy/docker-compose.yml | 实例跑 GHCR 镜像且数据完好 |
| T15 | `static/app.css` 构建产物入库（styles/ 源保留，README 注明重建步骤）——新克隆 `cargo run` 直接可用 | .gitignore, static/app.css, README.md | 干净克隆 cargo run 有样式 |
| T16 | 备份文档补「.env 与 data/ 一起备份；丢 CRYPTO_KEY 即丢全部文本」 | README.md, deploy/docker-compose.yml | 备份章节含密钥警告 |
| T17 | routes 关键路径单测 + smoke SSE 计时稳健化（27s 心跳等待改短/显式超时） | src/routes.rs, scripts/smoke.sh | 见 E5 底座 |
| T18 | favicon | templates/, static/ | 页面有图标 |

### 4.2 ENG 审查增补任务

| E# | 任务 | 文件 |
|---|---|---|
| E1 | 新建 **src/api.rs**：ApiAuth 提取器（FromRequestParts + 常量时间比较 + 限流同桶）+ 5 端点 + json_error 统一 `{"error":"..."}`；routes.rs 保持纯 web | src/api.rs, src/main.rs |
| E2 | 语言解析实现：aardbin_lang Cookie > Accept-Language > 默认 en；t() minijinja 函数 + 服务端翻译表；key parity 单测 | src/render.rs, src/routes.rs, templates/ |
| E3 | CLI 密钥通道：AARDBIN_ACCESS_KEY env > ~/.config/aardbin/config.toml（0600）> 交互提示（管道不卡） | src/cli/ |
| E4 | 交叉编译定为 cargo-zigbuild + goto-bus-stop/setup-zig（固定 zig 版本）；cross 仅 glibc 备选 | .github/, Dockerfile |
| E5 | **tests/api.rs 内存集成**（tower::ServiceExt oneshot）：drop-table 注入测 T9 回滚、429+Retry-After、分页、401；smoke.sh 仅扩 CLI 端到端（置于限流块之前或重启后）；arm64 qemu 最小冒烟（healthz+登录+建记录） | tests/api.rs, scripts/smoke.sh |
| E6 | 限流桶语义修正：API 仅错 key 时 record_failure、成功不清空、check 保持验证前；**登录 429 补 Retry-After 头** | src/api.rs, src/routes.rs, scripts/smoke.sh |
| E7 | release tarball 打包 templates/ + static/（并入 T6） | .github/ |
| E8 | E4 收录节奏（并入 T13） | — |
| E9 | Untitled 回服务端 t()；JS 轻量 t() 表覆盖相对时间/toast；smoke 双语断言更新 | src/render.rs, static/app.js, scripts/smoke.sh |
| E10 | API locale 无关契约：raw updated_at + title/content + untitled 布尔；解密失败 `{"error":"undecryptable"}` + 422；分页参数 `?page=N&page_size=M`（默认复用 PAGE_SIZE） | src/api.rs |
| E11 | CLI 双名 env 兼容（AARDBIN_ACCESS_KEY + ACCESS_KEY）+ AARDBIN_URL（默认 http://127.0.0.1:8080）+ config.toml [server] 段 | src/cli/, README.md |
| E12 | arm64 qemu 最小冒烟（并入 T6 流水线） | .github/ |

### 4.3 Design 审查增补任务

| D# | 任务 | 文件 |
|---|---|---|
| D1 | 语言选择器：登录页卡片右上 + 认证后 header 右侧（Logout 旁）；文本式「EN \| 中文」无图标无下拉壳；点击设 lang Cookie + 服务端重渲染当前页 | templates/login.html, templates/base.html, src/routes.rs |
| D2 | 登录页 Access Key 加可见 `<label>`（消除 placeholder-only 违规；label 随 t() 双语） | templates/login.html |
| D3 | docs/SPEC.md 增「设计令牌」章节：neutral 色板、字号 12/14/15、圆角 scale、shadow-sm、间距约定；字号取舍（<16px 但对比度 ≥4.5:1）显式标注 | docs/SPEC.md |
| D4 | toast 容器加 `aria-live="polite"`；移动端操作按钮触控目标 ≥44px | templates/base.html, templates/partials/records.html, styles/app.css |
| D5 | t() 运行时缺 key → fallback 英文 + 日志告警 | src/render.rs |

## 5. 已锁定决策（勿重新讨论，全部经用户拍板）

1. **版本**：首个公开发行 v1.0.0；tag `v1.0.0`；镜像 `ghcr.io/aardpro/aardbin:1.0.0` + latest
2. **文档结构**：README 双语（用户面）+ docs/SPEC.md 中文（开发面，PRD 迁入）+ TODOS.md 延期项唯一权威；「未来迭代」由 SPEC 承接，README 只放链接
3. **API**：src/api.rs 新模块；Bearer ACCESS_KEY；与登录**同一限流桶**（仅错 key 计数、成功不清空、429+Retry-After、CLI 线性退避；同 IP 连带锁死为已知可接受行为）；Origin 校验仅浏览器端点；locale 无关契约
4. **i18n**：模板双目录（en/zh）+ t() 翻译表 + Untitled 服务端 + JS 轻量表（相对时间/toast）+ lang 选择器；切换=设 cookie 重渲染当前页（不做 JS 即时切换）
5. **发布**：cargo-zigbuild musl 双架构 + buildx GHCR + tarball 打包资源 + arm64 qemu 冒烟
6. **测试**：tests/api.rs 内存集成（oneshot + drop-table 失败注入）+ parity 单测 + smoke 仅扩 CLI 端到端
7. **设计**：选择器双位置、登录 label、SPEC 设计令牌、aria-live、44px 触控、字号保持 12/14/15
8. **明确不做**：多用户、分享链接、全文搜索、附件加密、E2E 客户端加密、X-Forwarded-For 支持（文档记录限制即可）、密钥轮换、deploy 目录迁移（用户拍板维持原计划）、demo 实例（仅截图）、MCP server（延期 TODOS）
9. **E4**：awesome-selfhosted 先 issue 后 PR（+4 个月成熟期）

## 6. 防御性红线（违反 = 重做）

1. 任何提交前跑 `git check-ignore deploy/data/aardbin.db` 必须命中；首次提交用白名单 `git add <files...>`，**永不 `git add .`**
2. 永不读取/打印 deploy/.env、数据库内容、附件内容
3. 删除数据文件、force-push、改写历史等破坏性操作需用户明确书面确认
4. 每个任务完成标准 = 对应验证列通过（含 CI 全绿 + smoke 全过）
5. 改文档（README/SPEC）必须逐条对照源码，不得编造行为

## 7. 验收（v1.0.0 发布定义）

- CI 全绿：fmt / clippy / test / smoke.sh / docker build / cargo-deny / Trivy 无 CRITICAL/HIGH
- `git add .` 干跑不含任何数据文件；公开历史零密钥残留
- 双语言下模板 + 服务端错误消息 + toast 全部可切换；lang 选择器可用
- CLI 六条路径（paste/list/get/delete + 附件上传/下载）测试通过；429+Retry-After 路径通过
- 双架构镜像 + 双架构二进制发布；arm64 qemu 冒烟通过
- README 双语、SPEC 就位、零悬空引用；awesome-selfhosted issue 已开
- 自身在役实例运行 GHCR 镜像（dogfooding）且数据完好
