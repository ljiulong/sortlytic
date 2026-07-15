<!-- BEAUTIFIED -->

<div align="right">

<a href="README.md">English</a> · 中文

</div>

<p align="center">
  <img src="apps/macos/src-tauri/icons/icon.png" width="128" alt="Sortlytic logo" />
</p>

<h1 align="center">Sortlytic</h1>

<p align="center">
  <strong>用于采集、整理、校验和导出公开社交平台研究数据的本地优先 macOS 工作区。</strong>
  <br />
  <em>TikTok · 抖音 · 小红书 · 结构化工作流 · XLSX 与 PDF 导出</em>
</p>

<p align="center">
  <a href="#快速开始"><img src="https://img.shields.io/badge/快速开始-111827?style=for-the-badge" alt="Quick Start" /></a>
  <a href="https://github.com/ljiulong/sortlytic/releases/latest"><img src="https://img.shields.io/badge/最新版本-0891B2?style=for-the-badge" alt="Latest Release" /></a>
</p>

<p align="center">
  <a href="https://github.com/ljiulong/sortlytic/actions/workflows/ci.yml"><img src="https://github.com/ljiulong/sortlytic/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/ljiulong/sortlytic/releases"><img src="https://img.shields.io/github/v/release/ljiulong/sortlytic?display_name=tag&amp;style=flat" alt="Release" /></a>
  <img src="https://img.shields.io/badge/macOS-Desktop-000000?style=flat&amp;logo=apple&amp;logoColor=white" alt="macOS" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/TypeScript-007ACC?style=flat&amp;logo=typescript&amp;logoColor=white" alt="TypeScript" />
  <img src="https://img.shields.io/badge/React-20232A?style=flat&amp;logo=react&amp;logoColor=61DAFB" alt="React" />
  <img src="https://img.shields.io/badge/Tauri-FFC131?style=flat&amp;logo=tauri&amp;logoColor=black" alt="Tauri" />
  <img src="https://img.shields.io/badge/Rust-000000?style=flat&amp;logo=rust&amp;logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/SQLite-003B57?style=flat&amp;logo=sqlite&amp;logoColor=white" alt="SQLite" />
</p>

## 功能特性

| 能力 | 作用 |
|---|---|
| 多平台采集 | 将 TikTok、抖音和小红书的关键词搜索、评论、账号资料与内容详情映射到对应的 TikHub 端点。 |
| 受控任务执行 | 执行前要求用户确认计划，并由本地任务执行器校验请求数、记录数和预算上限。 |
| 自然语言计划 | 通过当前本地规则解析器把中文研究意图转换成经过校验的采集计划，同时保存运行快照。 |
| 提示词治理 | 保存提示词模板与版本，绑定输出 Schema，并在内置回归样例失败时阻止版本激活。 |
| 本地优先安全 | 工作区数据保存在本地 SQLite，API 凭据通过作用域隔离的密钥引用写入 macOS Keychain。 |
| 可审计交付 | 生成报告快照、执行导出完整性校验，并输出带哈希和任务历史的 Excel 工作簿与 PDF 报告。 |

## 快速开始

### 下载 macOS 应用

1. 打开 [GitHub 最新版本页面](https://github.com/ljiulong/sortlytic/releases/latest)。
2. 通过 **苹果菜单 → 关于本机** 查看芯片架构，也可以在终端运行 `uname -m`。
3. Apple 芯片（`arm64`）下载文件名以 `_aarch64.dmg` 结尾的安装包；Intel 芯片（`x86_64`）下载以 `_x64.dmg` 结尾的安装包。`.app.tar.gz` 和 `.sig` 是应用内更新产物，不是常规安装包。
4. 打开 DMG，把 Sortlytic 拖入**应用程序**，推出磁盘映像，再从“应用程序”目录启动 Sortlytic。

当前发版工作流尚未接入 Apple Developer ID 签名与公证。覆盖任何 macOS 安全警告前，请先阅读[首次打开与“已损坏”提示](#首次打开与已损坏提示)。

### 从源码启动

源码开发需要 macOS、Node.js 24、通过 Corepack 使用的 pnpm 11.5.2，以及 Rust 1.77.2 或更高版本。

```bash
git clone https://github.com/ljiulong/sortlytic.git
cd sortlytic/apps/macos
corepack enable
corepack install
pnpm install --frozen-lockfile
pnpm tauri dev
```

如果只需预览界面，不连接原生后端，可运行：

```bash
pnpm dev
```

浏览器预览使用演示数据，不能访问 macOS Keychain、执行原生采集任务、创建本地导出文件或安装应用更新。

## 使用方法

### 首次打开与“已损坏”提示

当前 `v0.1.5` 版本包含 Tauri updater 更新包签名，但 GitHub Actions 工作流还没有配置 macOS Gatekeeper 所需的 Apple Developer ID 证书和公证凭据。Tauri 文档说明，从浏览器下载的 macOS 应用需要代码签名，才能避免“应用已损坏，无法打开”警告。updater 签名用于校验 Sortlytic 应用内下载的更新产物，不能替代 Apple 代码签名和公证。

如果 macOS 出现截图中的提示：

1. 删除被拒绝的副本，从 [Sortlytic 官方 Releases 页面](https://github.com/ljiulong/sortlytic/releases)重新下载正确的 DMG，不要使用镜像站或聊天转发文件。
2. 确认 Apple 芯片对应 `_aarch64.dmg`，Intel 芯片对应 `_x64.dmg`。
3. 尝试打开一次 Sortlytic，然后进入**系统设置 → 隐私与安全性**。如果出现**仍要打开**，仅在确认下载来源后使用。Apple 说明，这个例外入口通常会在尝试启动后保留约一小时。
4. 如果系统仍报告应用损坏或没有显示**仍要打开**，并且已经确认应用来自官方 Release，可以只移除 Sortlytic 的隔离属性，再重新启动：

   ```bash
   xattr -dr com.apple.quarantine "/Applications/Sortlytic.app"
   open "/Applications/Sortlytic.app"
   ```

   这两条命令不会全局关闭 Gatekeeper，只会移除当前应用包的下载隔离属性。通常不需要 `sudo`；如果终端提示 `Permission denied`，只对 `xattr` 命令加一次 `sudo` 后重试。
5. 如果定向移除隔离属性后仍无法打开，请删除应用并重新核对芯片架构和下载完整性。应改用源码启动或等待完成 Developer ID 签名与公证的版本，不要执行 `sudo spctl --master-disable` 等全局绕过命令。

安全机制与发版要求可参考 [Apple Gatekeeper 指南](https://support.apple.com/zh-cn/102445)和 [Tauri macOS 签名指南](https://v2.tauri.app/zh-cn/distribute/sign/macos/)。

### 界面导航

| 区域 | 用途 |
|---|---|
| 工作台 | 创建计划、确认采集、跟踪任务状态、检查数据与证据，以及导出交付文件。 |
| 设置 | 查看本地工作区，配置 TikHub 和模型供应商，重新测试连接并安装更新。 |
| 使用指南按钮 | 点击右上角书本图标，查看 TikHub 注册、Token、域名、成本和安全说明。 |
| 主题按钮 | 切换明暗主题，偏好会保存在本机。 |

### 配置 TikHub

真实采集依赖 TikHub。创建任务前应先注册并验证账号：

1. [注册 TikHub 账号](https://user.tikhub.io/register)，完成邮箱验证，再[登录用户中心](https://user.tikhub.io/login)。
2. 在用户中心创建 API Token，并在显示时立即复制；使用付费端点前先查看 [TikHub 价格说明](https://tikhub.io/pricing)。
3. 在 Sortlytic 中打开**设置 → TikHub 设置 → 配置 TikHub API**。
4. 选择域名，粘贴 Token，然后点击**保存并测试**。

| 网络环境 | API 域名 |
|---|---|
| 国际网络 | `https://api.tikhub.io` |
| 中国大陆网络 | `https://api.tikhub.dev` |

测试成功后，界面会显示脱敏账号邮箱、可用免费额度和邮箱验证状态。Token 写入 macOS Keychain，SQLite 工作区只保存带作用域的密钥引用。编辑已有配置时，可以把 Token 输入框留空以复用已保存 Token。

首次采集建议只设置 10～50 条记录，先检查平台、数据类型、国家或地区、关键词和端点成本，再逐步扩大任务。

### 配置模型供应商（可选）

打开**设置 → 模型 API 设置**，可以保存 OpenAI、Anthropic、Gemini 或自定义 OpenAI 兼容配置。选择 API 格式，按需填写 Base URL，再填写默认模型 ID 和 API Key，最后点击**保存并校验**；已保存的本地供应商配置也可以使用 Ollama API 格式。密钥位于 macOS Keychain，后续可以留空复用。

当前 MVP 不要求配置模型供应商。自然语言计划仍使用 `local-rule-engine/rule-parser-v1`；基于供应商模型的计划生成和真实模型推理尚未接入。

### 创建并确认采集计划

打开**工作台 → 采集创建**，选择一种入口：

| 方式 | 适用场景 | 需要重点检查 |
|---|---|---|
| 表单式 | 已明确平台、数据类型、地区、关键词、时间范围、记录数和预算。 | 生成计划前确认每个字段。 |
| 自然语言 | 希望用中文描述调研目标，再由本地解析器整理成结构化计划。 | 检查推断的平台、数据类型、缺失条件、记录上限和预算。 |

表单支持 TikTok、抖音和小红书，数据类型包括关键词搜索、账号公开信息、评论采集和内容详情。单任务记录数范围为 10～5,000，成本上限可填写 1～500。

点击**生成计划**或**解析为计划**后：

1. 检查计划预览，重点核对平台、数据类型、国家或地区、时间范围、最大记录数、请求估算和金额上限。
2. 处理**缺失条件**中列出的内容。只有后端校验状态为有效时才能确认计划。
3. 点击**确认运行**。生成计划本身不会开始正式付费采集；确认后任务才会进入本地队列。
4. 在**任务队列**中跟踪状态。可能出现已排队、运行中、等待确认、部分成功、成功和失败等状态。

### 检查结果与证据

在**数据资产**中选择一条记录，右侧面板会展示来源链接、证据摘要、校验状态、置信度、模型运行和转换理由。标记为**待人工确认**或**证据不足**的记录，应在进入报告前人工复核。

当前 MVP 边界：原生任务执行与原始记录存储已经实现，但工作台的真实记录查询尚未接通。因此，在打包应用的后端会话中，任务运行后**数据资产**仍可能为空；浏览器预览中的记录属于演示数据，不是真实采集结果。

### 导出 Excel 与 PDF

1. 至少创建一个采集任务。
2. 在右侧**导出中心**点击**执行导出检查**。
3. Sortlytic 会生成报告快照、校验导出请求，并分别创建 XLSX 与 PDF 任务。
4. 两项均显示**已通过**后，根据**Excel 工作簿**和**PDF 报告**下方显示的路径定位文件。

文件保存在当前工作区中：

```text
default-workspace/
├── app.sqlite
├── raw/tikhub/
├── reports/
├── exports/excel/
└── exports/pdf/
```

XLSX 用于承载结构化报告数据。当前 PDF 写入器只生成简要摘要，并提示读者到工作簿查看完整结构化数据。界面中的 **Webhook 摘要**尚未启用，不会发送数据。

### 更新应用

打包版本可以通过**设置 → 自动更新**更新：

1. 点击**检查更新**。
2. 查看版本号与更新说明。
3. 点击**下载并重启**。Sortlytic 会校验 Tauri updater 更新产物签名，安装更新并重新启动。

浏览器预览没有更新权限。Apple Developer ID 签名与公证和 updater 签名校验是两套机制，仍需接入发版工作流。

### 常见问题

| 现象 | 检查方法 |
|---|---|
| “Sortlytic 已损坏，无法打开” | 从官方 Release 重新下载匹配架构的 DMG，再按[首次打开与“已损坏”提示](#首次打开与已损坏提示)处理。 |
| TikHub 测试失败 | 检查 Token 是否完整、邮箱是否已验证、域名是否适合当前网络，以及账号额度是否足够。 |
| **保存并测试**不可点击 | 新 TikHub Token 至少需要 8 个字符；已有 Token 可以留空复用。 |
| 计划无法确认 | 先生成计划，清空**缺失条件**，并确认后端校验状态为有效。 |
| 导出失败 | 先创建任务，再查看工作区标题下方的后端错误；任务数据可用后重新执行导出。 |
| 页面有逼真的记录，但原生功能不可用 | 当前通过 `pnpm dev` 运行浏览器演示模式。请使用 `pnpm tauri dev` 或打包应用。 |
| 找不到**仍要打开** | 再尝试启动一次，并在一小时内检查“隐私与安全性”。对已确认来源的官方版本使用上面的定向 `xattr` 命令，不要全局关闭 Gatekeeper。 |

### 数据与安全边界

- Sortlytic 当前只创建一个本地 `default-workspace`，不提供用户账号、远端数据库、远端同步或多设备同步。
- 工作区数据、原始响应、提示词快照、日志、报告和导出文件都保存在 macOS 应用数据目录下。
- TikHub 与模型 API 密钥保存在 macOS Keychain，不会写入报告或导出文件。
- 删除应用不一定会删除工作区或 Keychain 条目。手动清理应用数据前，应先备份需要保留的 XLSX、PDF 和原始文件。
- 只采集有权访问的公开数据，并遵守平台条款、隐私要求与适用法律。

## 架构

```mermaid
%%{init: {'theme': 'base', 'themeVariables': {'fontSize': '14px', 'lineColor': '#64748B'}}}%%
graph LR
    classDef client fill:#3B82F6,stroke:#2563EB,color:#fff,stroke-width:2px
    classDef service fill:#10B981,stroke:#059669,color:#fff,stroke-width:2px
    classDef data fill:#8B5CF6,stroke:#7C3AED,color:#fff,stroke-width:2px
    classDef auth fill:#F97316,stroke:#EA580C,color:#fff,stroke-width:2px
    classDef external fill:#F43F5E,stroke:#E11D48,color:#fff,stroke-width:2px

    A[React Workbench<br/>TypeScript] --> B[Tauri Command Layer<br/>Rust]
    B --> C[Collection Task Worker]
    B --> D[Prompt and Plan Runtime]
    B --> E[Report and Export Engine]
    B --> F[(Local Workspace<br/>SQLite and files)]
    B --> G[macOS Keychain]
    B --> H[TikHub and Model APIs]
    C --> F
    C --> H
    D --> F
    E --> F

    class A client
    class B,C,D,E service
    class F data
    class G auth
    class H external
```

## 配置

### 应用身份

| 配置项 | 值 | 来源 |
|---|---|---|
| 产品名称 | `Sortlytic` | `apps/macos/src-tauri/tauri.conf.json` |
| 应用标识 | `com.steven.sortlytic` | `apps/macos/src-tauri/tauri.conf.json` |
| 默认工作区 | `default-workspace` | 创建在 macOS 应用数据目录中 |
| 本地持久化 | SQLite、原始记录、报告与导出文件 | 保存在当前活动工作区中 |
| 更新端点 | `https://github.com/ljiulong/sortlytic/releases/latest/download/latest.json` | Tauri updater 配置 |

### 应用内设置

| 配置项 | 用途 | 存储位置 |
|---|---|---|
| TikHub API 域名 | 根据当前网络选择 `api.tikhub.io` 或 `api.tikhub.dev` | 工作区数据库 |
| TikHub Token | 用于采集请求和账户状态检查 | macOS Keychain |
| 模型供应商 | 保存 API 格式、端点、地区、策略和健康状态 | 工作区数据库 |
| 模型 API Key | 用于模型供应商连通测试 | macOS Keychain |
| 默认模型配置 | 记录模型能力和当前模型选择 | 工作区数据库 |

### 发布密钥

| GitHub Actions Secret | 用途 |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | 为发版工作流生成的 updater 更新产物签名。 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 在签名私钥带密码时用于解锁私钥。 |

不要把签名私钥、API Token 或导出的凭据提交到代码仓库。

## 项目结构

```text
.
├── .github/workflows/          # CI 与 macOS 发版自动化
│   ├── ci.yml                  # 前端、Rust 和依赖检查
│   └── release-macos.yml       # 版本递增、签名、打包与发布
├── apps/macos/                 # Sortlytic 桌面应用
│   ├── src/                    # React 工作台与设置界面
│   ├── src-tauri/              # Rust 命令、存储、任务执行器与打包配置
│   └── package.json            # pnpm 脚本与前端依赖
├── excel/                      # 项目使用的表格模板
├── plan/                       # 产品、架构、测试与交付文档
├── AGENTS.md                   # 仓库协作规则
├── README.md                   # 英文文档
└── README-zh.md                # 简体中文文档
```

## 技术栈

### 界面层

| 技术 | 用途 |
|---|---|
| React 19 | 桌面工作台与设置界面 |
| TypeScript 6 | 前端类型与 Tauri 命令契约 |
| Vite 8 | 前端开发和生产构建 |
| TanStack Query 与 Table | 服务状态协调与表格数据展示 |
| React Hook Form 与 Zod | 表单状态和输入校验 |
| Radix Tabs 与 Lucide | 无障碍导航基础组件与界面图标 |

### 桌面端与数据层

| 技术 | 用途 |
|---|---|
| Tauri 2 | macOS 原生应用外壳与命令桥接 |
| Rust | 工作区、采集、任务、提示词、安全和导出逻辑 |
| SQLite 与 rusqlite | 本地事务型工作区存储 |
| macOS Keychain | 通过作用域密钥引用保存 API 凭据 |
| reqwest | TikHub 与模型供应商连通请求 |
| rust_xlsxwriter | 原生 XLSX 报告生成 |

### 质量与交付

| 技术 | 用途 |
|---|---|
| Vitest | 前端单元测试 |
| Oxlint | 前端静态检查 |
| Cargo fmt、test 与 Clippy | Rust 格式、测试和 lint 检查 |
| GitHub Actions | CI、版本管理、双架构 macOS 构建与发版 |
| Tauri updater | 已签名更新元数据和可下载应用产物 |

## 部署与发布

### 本地验证

```bash
cd apps/macos
pnpm lint
pnpm test
pnpm build
```

```bash
cd apps/macos/src-tauri
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo test --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
```

### 构建 macOS 产物

```bash
cd apps/macos
pnpm build:mac
```

本地构建 updater 更新产物时，需要配置 `TAURI_SIGNING_PRIVATE_KEY`；私钥带密码时还需要 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。

### 发布新版本

手动运行 [`release-macos`](.github/workflows/release-macos.yml) 工作流，并选择 patch、minor 或 major 版本递增。工作流会同步 `package.json`、`tauri.conf.json` 和 `Cargo.toml`，创建 `app-vX.Y.Z` 标签，然后分别构建 Apple Silicon 与 Intel 的 `.app` 和 `.dmg` 产物并发布到 GitHub Release。

## 贡献

1. Fork 本仓库。
2. 创建边界清晰的分支：`git checkout -b feature/short-description`。
3. 完成修改并运行相关前端与 Rust 检查。
4. 只提交本次范围内的文件。
5. 推送分支并创建 Pull Request。

当前仓库没有 LICENSE 文件。正式分发或按明确条款接收外部贡献前，应先添加许可证。
