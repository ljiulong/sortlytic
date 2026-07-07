# 首批用户交付与 TikHub 可行性测试说明

日期：2026-07-07  
适用对象：第一批非开发测试用户、产品验收、外部协作方  
适用版本：macOS 单端 MVP，本地工作区模式

## 1. 交付产物

本次可交付产物位于：

- 应用包：`apps/macos/src-tauri/target/release/bundle/macos/智能数据整理平台.app`
- 安装包：`apps/macos/src-tauri/target/release/bundle/dmg/智能数据整理平台_0.1.0_aarch64.dmg`

建议给非开发用户分发 `.dmg`。测试机打开后，如 macOS 提示来自未认证开发者，可在“系统设置 > 隐私与安全性”中允许打开。

## 2. 首次使用流程

1. 打开 `智能数据整理平台.app`。
2. 应用会自动创建默认本地工作区。
3. 首页应显示“本地研究工作区”，并展示任务、记录、预计请求、证据覆盖等指标。
4. 右侧面板应显示“TikHub 设置 / 免费额度可行性测试”。
5. 在 TikHub 用户中心复制 API Key。
6. 回到应用，选择 API 域名：
   - 国际网络优先选择 `https://api.tikhub.io`
   - 中国大陆网络不稳定时选择 `https://api.tikhub.dev`
7. 粘贴 API Token，点击“保存并测试”。
8. 成功后应显示：
   - 状态：已连通
   - 账号：脱敏邮箱
   - 免费额度：当前剩余额度
   - 邮箱验证：已验证或未验证

## 3. TikHub 账号与免费额度测试结果

本次使用 TikHub 已登录账号和已有 API Key 完成测试，未创建新的持久 API Key。

已验证信息：

- TikHub 用户中心入口：`https://user.tikhub.io`
- API 密钥页：`https://user.tikhub.io/dashboard/api`
- 当前 API Key 状态：活跃
- 当前免费额度：`$0.0500`
- 当前余额：`$0.0000`
- 邮箱状态：已验证
- 应用内 Token 保存位置：macOS Keychain
- 应用不会在 SQLite、日志、导出文件或界面中展示完整 Token

应用内连通性测试调用：

- `GET /api/v1/tikhub/user/get_user_info`
- `GET /api/v1/tikhub/user/get_user_daily_usage`

免费 demo 可行性测试调用：

- `GET /api/v1/demo/tiktok/web/fetch_user_profile`
- `GET /api/v1/demo/douyin/web/fetch_one_video`

测试结论：

- TikHub 官方 API 域名 `https://api.tikhub.io` 可连通。
- Token 可通过应用保存到 macOS Keychain，并可从后端读取用于请求 TikHub。
- TikHub 用户信息接口返回成功，免费额度保持 `0.05`。
- TikTok demo 与抖音 demo 端点均返回 HTTP 200。
- 官方 OpenAPI 描述中 demo 端点标注为免费使用，本次 demo 调用后免费额度仍为 `0.05`。

## 4. 已验证的应用主流程

本次以打包后的 `.app` 实机验证，不使用浏览器开发服务器代替最终应用。

已验证路径：

- 默认本地工作区自动创建和打开。
- 表单式采集计划可见，确认前不会触发真实采集费用。
- 任务队列、数据资产、提示词回归区域可见。
- TikHub Token 可保存到 Keychain，并完成官方 API 连通性测试。
- 导出门禁可生成 Excel 工作簿与 PDF 报告。

本次导出验证产物：

- Excel：`~/Library/Application Support/com.steven.smart-data-workbench/default-workspace/exports/excel/88efd279-4da3-4811-887f-5971b57e47e6.xlsx`
- PDF：`~/Library/Application Support/com.steven.smart-data-workbench/default-workspace/exports/pdf/88efd279-4da3-4811-887f-5971b57e47e6.pdf`

文件校验：

- Excel 文件头为 `PK`，符合 `.xlsx` 压缩包格式。
- PDF 文件头为 `%PDF-1.4`，识别为 1 页 PDF。

## 5. 安全与隐私说明

- API Token 只保存到 macOS Keychain。
- SQLite 中只保存密钥引用、脱敏提示和审计记录。
- UI 只展示脱敏 Token，例如 `hcoO...[REDACTED]...YA==`。
- 导出文件不包含完整 Token、Authorization Header 或敏感 Header。
- 本次测试后已清空系统剪贴板，避免 Token 残留。
- 免费 demo 调用仅用于验证连接和返回结构，不代表已完成真实业务采集。

## 6. 已知限制

- 当前 MVP 仅支持 macOS 单端本地工作区。
- 当前未实现用户账号、远端数据库、远端同步或多设备自动同步。
- 真实采集调用仍需在成本上限、端点价格和任务参数确认后执行。
- 小红书暂无本次使用的官方 demo 端点；本次只完成 TikTok 与抖音 demo 验证，以及 TikHub 用户/额度接口验证。

## 7. 首批用户验收清单

交付给非开发用户前，建议逐项确认：

- 能从 `.dmg` 安装并打开应用。
- 首屏能看到默认工作区。
- TikHub Token 能粘贴、保存并测试成功。
- 应用显示脱敏账号、免费额度和邮箱验证状态。
- 点击“生成计划”只生成本地计划，不直接消耗采集费用。
- 点击“执行导出检查”能生成 Excel 与 PDF。
- 导出的 Excel/PDF 能在本机正常打开。
- 用户知道真实采集前必须确认成本上限。

## 8. 参考入口

- TikHub 文档：`https://docs.tikhub.io/`
- TikHub 用户中心：`https://user.tikhub.io`
- TikHub OpenAPI：`https://api.tikhub.io/openapi.json`
