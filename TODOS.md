# TODOS

开源发布（v1.0.0）的延期项唯一权威清单。与 docs/SPEC.md「未来迭代」章节交叉引用，两边改动需同步；README 只放指向本文件的一行链接。

## 延期项

### 1. MCP server（新增）
- **What:** 为 aardbin 提供 MCP server，让 AI 助手直接读写记录
- **Why:** JSON API 落地后（E3），MCP server 是薄封装；平台潜力最大的一项
- **Pros:** 进入 AI 助手生态，差异化强
- **Cons:** 需持续跟进 MCP 协议变化
- **Context:** 依赖 E3 的 JSON API 面；鉴权复用 Bearer ACCESS_KEY + 限流桶
- **Effort:** M（CC: S）
- **Priority:** P3
- **Depends on:** E3 CLI / JSON API

### 2. 深色主题（见 docs/SPEC.md 未来迭代）
- **What:** 深色/浅色主题切换
- **Why:** 深夜使用场景；i18n 落地后主题是同类低垂果实
- **Effort:** S（CC: S） | **Priority:** P3

### 3. PWA（见 docs/SPEC.md 未来迭代）
- **What:** manifest + service worker，可安装到手机桌面
- **Why:** 多设备场景下移动端体验提升
- **Effort:** M（CC: S） | **Priority:** P3

### 4. 粘贴直达建记录（新增）
- **What:** 列表页直接 Ctrl+V 文本即建记录
- **Why:** 产品核心动作是「丢东西」，粘贴是最快入口
- **Effort:** S（CC: S） | **Priority:** P3

### 5. 快捷键（新增）
- **What:** `/` 新建、`c` 复制等键盘操作
- **Effort:** S（CC: S） | **Priority:** P3

### 7. SPEC 英文版（V10）
- **What:** docs/SPEC.md 提供英文版
- **Why:** SPEC 的读者正是贡献者与 API 消费者（英文读者）；首版 README 已双语
- **Pros:** 英文贡献者/API 消费者无障碍
- **Cons:** 约 2000 行 M 级翻译量 + 长期双写维护
- **Context:** 中文为源语言；等首个外部贡献者或 API 消费者信号出现再译
- **Effort:** M（CC: S） | **Priority:** P3

其余未来迭代项见 docs/SPEC.md 未来迭代章节（原 PRD §42 全部清单迁移至此），不在此重复。
