# Contributing to aardbin / 贡献指南

Thank you for your interest in contributing to aardbin!

## How to Contribute

1. **Fork** the repository and create a feature branch from `master`.
2. **Make changes** — keep commits focused and atomic.
3. **Test** — run `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`, and `bash scripts/smoke.sh` locally.
4. **Open a Pull Request** — fill in the PR template and describe what changed and why.

## Code Style

- Rust code follows standard `rustfmt` formatting.
- Clippy warnings are treated as errors (`-D warnings`).
- Keep the dependency tree minimal — no new dependencies without justification.
- Comments and commit messages in English preferred.

## Reporting Bugs

Use the [bug report issue template](.github/ISSUE_TEMPLATE/bug_report.md). Include:
- Steps to reproduce
- Expected vs actual behavior
- Environment (OS, Docker version, aardbin version)

## Feature Requests

Use the [feature request issue template](.github/ISSUE_TEMPLATE/feature_request.md).

## Security Issues

See [SECURITY.md](SECURITY.md) for the responsible disclosure process.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).

---

## 中文

感谢你对 aardbin 的兴趣！

1. **Fork** 仓库，从 `master` 分支创建功能分支。
2. **修改代码** — 保持提交聚焦且原子化。
3. **测试** — 本地运行 `cargo fmt`、`cargo clippy -- -D warnings`、`cargo test` 和 `bash scripts/smoke.sh`。
4. **提交 Pull Request** — 填写 PR 模板，说明修改内容和原因。

- Rust 代码使用标准 `rustfmt` 格式。
- Clippy 警告视为错误（`-D warnings`）。
- 尽量不引入新依赖——需要时请说明理由。
- 注释和提交信息建议使用英文。
- 提交即表示同意以 [MIT 许可证](LICENSE) 发布贡献。
