# Sortlytic

Sortlytic 是一个本地优先的 macOS 智能数据整理应用。它面向内容运营、市场研究、产品分析和数据整理场景，帮助用户把 TikTok、抖音、小红书等平台的公开内容采集任务、AI 结构化整理、证据追溯和可交付导出放在一个本机工作区里完成。

应用默认不依赖账号体系、不接远端数据库、不做远端同步。用户在本机配置 TikHub 与模型供应商密钥，任务数据、运行快照、导出文件和配置状态都优先保存在本地。

## 核心能力

- 多平台采集规划：围绕 TikTok、抖音、小红书创建关键词、评论、账号公开信息和内容详情等采集任务。
- 自然语言任务生成：把业务意图转成可确认的采集计划，并在执行前保留人工确认门槛。
- AI 数据整理：支持摘要、分类、情绪、实体和洞察生成，并保留提示词版本、运行快照和证据引用。
- 本地工作区：使用 SQLite 保存任务、连接器、模型配置、运行记录和导出记录。
- 密钥安全：密钥写入系统安全存储，数据库只保存密钥引用和脱敏提示。
- 可交付导出：优先输出 Excel 工作簿，报告类交付保留 PDF 路径。
- 自动化轻集成：通过 Webhook 和文件导出与 n8n 等工具衔接，但不把自动化平台作为核心依赖。

## 适用场景

- 内容运营团队批量整理评论、账号和话题数据。
- 市场研究人员快速形成可复核的公开内容观察。
- 产品团队追踪用户反馈、情绪变化和竞品内容信号。
- 个人研究者在本机保留数据、密钥和报告，不依赖云端工作区。

## 技术栈

- 前端：React、TypeScript、Vite、TanStack Query、TanStack Table、Radix Tabs、lucide-react。
- 桌面端：Tauri 2。
- 后端核心：Rust、SQLite、rusqlite、rust_xlsxwriter。
- 测试与质量：Vitest、Oxlint、Cargo test、Cargo clippy。
- 发布：GitHub Actions、Tauri bundler、Tauri updater。

## 目录结构

```text
.
├── apps/macos                 # Sortlytic macOS 客户端
│   ├── src                    # React 前端
│   ├── src-tauri              # Tauri/Rust 后端与打包配置
│   ├── package.json           # 前端与 Tauri CLI 脚本
│   └── pnpm-lock.yaml
├── plan                       # PRD、技术方案、测试说明和交付文档
├── .github/workflows          # CI 与 macOS 自动发版工作流
└── AGENTS.md                  # 本仓库协作规则
```

## 本地开发

进入 macOS 应用目录：

```bash
cd apps/macos
corepack enable
corepack install
pnpm install --frozen-lockfile
```

启动前端开发服务器：

```bash
pnpm dev
```

启动 Tauri 桌面应用开发模式：

```bash
pnpm tauri dev
```

如果只需要构建前端静态产物：

```bash
pnpm build
```

## 测试与检查

前端检查：

```bash
cd apps/macos
pnpm lint
pnpm test
pnpm build
```

Rust 检查：

```bash
cd apps/macos/src-tauri
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo test --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
```

CI 会在 GitHub Actions 中运行前端 lint/test/build、Rust fmt/check/test/clippy，并执行依赖安全审计。

## 打包与发布

本地打包 macOS 应用：

```bash
cd apps/macos
pnpm build:mac
```

发布流程由 `.github/workflows/release-macos.yml` 管理。手动触发 `release-macos` workflow 后，会自动递增版本号、同步 Tauri 与 Cargo 版本、创建 `app-v版本号` 标签，并产出 macOS `.app` 与 `.dmg` 包。

自动更新配置位于 `apps/macos/src-tauri/tauri.conf.json`，更新元数据从 GitHub Release 下载：

```text
https://github.com/ljiulong/sortlytic/releases/latest/download/latest.json
```

## 数据与安全

- 应用标识：`com.steven.sortlytic`。
- 默认工作区：`default-workspace`。
- macOS 数据目录：`~/Library/Application Support/com.steven.sortlytic/default-workspace`。
- 本地数据库：工作区内 SQLite 文件。
- 导出文件：工作区下的 `exports` 与 `reports` 目录。
- 密钥存储：系统 Keychain，服务名为 `com.steven.sortlytic`。
- 数据策略：MVP 不做账号、远端数据库、远端同步、多设备自动同步或云端密钥托管。

Sortlytic 会尽量把模型输出、运行快照和证据来源绑定在一起，降低“只有结论、没有来源”的数据风险。AI 生成内容仍需要人工复核，尤其是报告、商业判断和外部交付材料。

## 路线图

- 完成 macOS 单端 MVP 的采集、整理、导出和本地设置闭环。
- 强化提示词版本管理、Schema 约束、证据引用和回归样例。
- 扩展更多模型供应商和 OpenAI-compatible endpoint。
- 增强 Excel 工作簿导出和 PDF 报告排版。
- 后续再评估多端工作区、账号体系、远端同步和协作能力。

## 协作约定

- 生产源码文件默认不超过项目约定的行数上限。
- 涉及桌面 UI 的最终验证必须以打包后的应用产物为准。
- 每个独立修改包完成验证后单独提交，只提交该包实际修改的文件。
- Sortlytic 是本应用正式英文名，不是候选序号或临时代号。
