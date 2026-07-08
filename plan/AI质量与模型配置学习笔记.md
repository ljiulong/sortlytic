# AI 质量与模型配置学习笔记

日期：2026-07-04  
适用文档：智能数据整理平台 PRD v0.5  
研究方式：主线程阅读本地克隆仓库，并调用两个子代理并行研究 OpenClaw 与 WeSight  
文档目的：把对 AI slop、反假进度、可追溯数据整理、多供应商模型配置和 Agent 配置的学习经验沉淀为本项目规则

## 1. 结论摘要

本项目要避免的 AI slop 不是“用了 AI”本身，而是“看起来已经整理完成，但缺少来源、证据、验证、状态边界、复现能力和隐私处理”的低质量结果。

对智能数据整理平台来说，真正危险的 slop 是数据 slop：

- 生成了漂亮摘要，但不能回到原始记录。
- 标记为已完成，但实际有字段未校验或接口失败。
- 报告看起来完整，但不知道使用了哪个模型、哪个提示词、哪批输入。
- AI 计划看似合理，但参数没有被平台白名单、成本上限和用户确认约束。
- 导出 Excel 或 PDF 时，把待确认、部分成功、失败边界隐藏成“全部完成”。

因此产品设计要把 AI 能力收束到可追溯、可校验、可拒绝、可复现的工作流里。AI 可以参与需求解析、字段整理、摘要、分类、聚类和报告表达，但不能替代证据、状态、成本确认和人工审核。

模型接入也不能局限 OpenAI、Gemini、Claude 三家。更稳的设计是学习 WeSight 的 Provider Registry 思路：把“供应商”“协议格式”“模型”“Agent”“运行快照”“工具”拆开。OpenAI、Gemini、Claude 是内置官方供应商，但产品架构应从第一版就支持 DeepSeek、Qwen、Moonshot、OpenRouter、Ollama、自定义 OpenAI-compatible endpoint 等扩展能力。

## 2. 研究来源

### 2.1 OpenClaw

研究对象：

- GitHub 仓库：https://github.com/openclaw/openclaw
- 子代理研究版本：`main@1c46fe72c92d9d2fb5062cd20e3f5fd67ae4f5ba`

重点文件：

- `docs/concepts/personal-agent-benchmark-pack.md`
- `qa/scenarios/personal/no-fake-progress.yaml`
- `qa/scenarios/personal/task-followthrough-status.yaml`
- `qa/scenarios/personal/failure-recovery.yaml`
- `docs/concepts/qa-e2e-automation.md`
- `docs/maturity/scorecard.md`
- `docs/concepts/system-prompt.md`
- `docs/gateway/protocol.md`
- `src/agents/payload-redaction.ts`
- `.github/pull_request_template.md`
- `CONTRIBUTING.md`
- `SECURITY.md`

重要发现：

- 仓库没有直接用 `AI slop` 这个词做概念定义。
- 但它把 AI slop 拆成了可测试的失败模式：假进度、无证据完成声明、伪造工具执行、把阻塞说成完成、泄露敏感内容、没有真实验证就交付。
- 其中 `no-fake-progress` 场景非常适合本项目借鉴：没有证据 artifact 之前，不能宣称任务完成。

### 2.2 WeSight

研究对象：

- GitHub 仓库：https://github.com/freestylefly/wesight
- 子代理研究版本：`36af2b6494a4c8797d51c3114f3edf06a45b663c`

重点文件：

- `README_zh.md`
- `src/renderer/config.ts`
- `src/shared/providers/constants.ts`
- `src/main/libs/claudeSettings.ts`
- `src/main/libs/openclawConfigSync.ts`
- `src/main/libs/coworkOpenAICompatProxy.ts`
- `src/shared/cowork/runtimeSnapshot.ts`
- `src/shared/cowork/constants.ts`
- `src/main/coworkStore.ts`
- `src/main/mcpStore.ts`
- `src/main/libs/mcpServerManager.ts`
- `openclaw-extensions/mcp-bridge/index.ts`

重要发现：

- WeSight 明确不是三家模型平台架构。
- 它支持 OpenAI、Anthropic Claude、Google Gemini，也支持 DeepSeek、Qwen、Moonshot、Ollama、OpenRouter、GitHub Copilot、本地网关、私有 endpoint、自定义 OpenAI-compatible 接口。
- 它的关键思路是把模型配置拆成 Provider、API Format、Model、Agent Engine、Runtime Snapshot、MCP Tools。
- Provider Registry 是单一事实源，集中维护供应商 ID、默认 Base URL、协议格式、默认模型、区域、能力标签和下游 runtime 映射。

## 3. OpenClaw 学到的反 AI slop 规则

### 3.1 先定义失败模式，不只定义愿景

OpenClaw 的价值不在于口头说“高质量”，而是把低质量 AI 行为拆成场景：

- 没读证据就回复完成。
- 没有 artifact 就声称已经交付。
- 外部发送、上传、发布、同步其实没有发生，却在最终回复里说已经发生。
- 一部分失败了，却把整体状态说成完成。
- 遇到权限或工具边界时没有明确阻塞点。
- 日志、诊断、报告中泄露敏感内容。

本项目对应规则：

- 没有保存原始记录 ID、模型运行记录、提示词版本和字段校验状态，就不能把 AI 结论标为可交付。
- 没有生成 Excel 或 PDF 文件，就不能提示导出成功。
- 没有真实调用 TikHub API，就不能说数据已采集。
- 没有完成 Webhook 发送，就不能说已经同步到 n8n 或外部系统。
- 部分成功必须显示为部分成功，不能在报告里包装成成功。

### 3.2 完成状态必须有证据

OpenClaw 的 `no-fake-progress` 思路可以直接转成“no fake normalization”：

- 未校验字段不能标为已清洗。
- 未合并记录不能标为已合并。
- 未同步外部系统不能标为已同步。
- 未人工确认的高风险结论不能标为已确认。
- 未通过 Schema 的模型输出不能进入核心报告。

本项目完成门禁：

- 输出 artifact 已写入。
- Schema 校验通过。
- 冲突队列为空，或冲突被明确标记为待人工确认。
- 每条核心洞察至少有一个原始记录引用。
- 成本、供应商、模型、提示词版本和运行时间已记录。
- 导出文件通过基本完整性检查。

### 3.3 状态要诚实，不要混成“完成”

OpenClaw 不允许把完成、失败、阻塞混在一起。本项目任务状态也必须足够细：

- 草稿。
- 等待确认。
- 排队中。
- 运行中。
- 部分成功。
- 成功。
- 失败。
- 已取消。
- 待人工确认。

状态规则：

- `待人工确认` 不能汇总成 `成功`。
- `部分成功` 必须展示已完成范围和失败范围。
- `失败` 必须包含失败阶段、失败原因、可重试边界和下一步。
- `已取消` 后不得继续发起 TikHub 或模型供应商请求。

### 3.4 证据面板是产品能力，不是调试信息

为了避免数据 slop，每条 AI 结果旁边都应有“为什么是这个值”的证据入口。

字段级证据至少包含：

- 来源记录 ID。
- 原始链接。
- 参与判断的原文片段或字段摘要。
- 转换理由。
- 使用的规则或模型版本。
- 置信度。
- 校验状态。
- 人工修改历史。

报告级证据至少包含：

- 任务条件。
- 采集范围。
- 数据量。
- 排除范围。
- 模型供应商和模型 ID。
- 提示词版本。
- 生成时间。
- 免责声明。

### 3.5 安全默认值比提示词更可靠

OpenClaw 不只靠系统提示词约束 Agent，而是在协议、工具、诊断和 redaction 层做硬约束。

本项目应采用同样思路：

- API Key 永远不进入 AI prompt。
- AI 不直接持有 TikHub API Key。
- AI 不直接发起 TikHub 网络请求。
- 自然语言采集中，AI 只生成计划；应用校验后调用 TikHub REST API。
- 请求日志和错误日志默认脱敏。
- PDF、Excel、Webhook、备份默认不包含完整密钥和完整 Header。

## 4. WeSight 学到的模型与 Agent 配置

### 4.1 不要把模型平台写死成三家

正确的产品表达：

- 内置支持 OpenAI、Gemini、Claude。
- 同时支持 DeepSeek、Qwen、Moonshot、OpenRouter、Ollama 等扩展供应商。
- 支持自定义 OpenAI-compatible endpoint。
- 后续可支持 Anthropic-compatible、Gemini-compatible、本地模型网关和企业私有网关。

错误的产品表达：

- “只支持 OpenAI、Gemini、Claude 三家。”
- “一个模型配置表里写死三个 API Key 字段。”
- “用供应商名称判断所有协议细节。”

### 4.2 Provider 和 API Format 要分离

供应商是“谁提供服务”，协议格式是“怎么调用”。

示例：

- OpenAI 供应商通常使用 OpenAI Responses 或 Chat Completions。
- DeepSeek、Qwen、Moonshot、OpenRouter 可能使用 OpenAI-compatible 协议。
- Gemini 有自己的官方协议，也可能通过兼容层提供 OpenAI-compatible endpoint。
- 本地网关可能暴露 OpenAI-compatible endpoint，但实际模型来自 Ollama 或私有部署。

本项目建议：

```ts
Provider = {
  provider_id,
  display_name,
  enabled,
  auth_type,
  secret_ref,
  base_url,
  api_format,
  models,
  default_model,
  capabilities,
  region,
  cost_policy,
  rate_limit_policy,
  health_check
}
```

### 4.3 模型 ID 要带供应商命名空间

只保存 `model_id` 会有冲突风险，因为多个供应商可能都提供同名或相似模型。

建议使用：

```text
provider_id + model_id
```

例如：

```text
openai:gpt-4.1
openrouter:anthropic/claude-sonnet
ollama:qwen3
custom_company_gateway:model-a
```

### 4.4 Agent 配置和供应商配置要解耦

Agent 不应该直接等于某个 API Key。

Agent 负责：

- 名称。
- 身份。
- 系统提示词。
- 技能或工具权限。
- 默认模型偏好。
- 默认运行模式。

Provider 负责：

- API Key。
- Base URL。
- 协议格式。
- 模型列表。
- 连接测试。
- 成本和限流策略。

Runtime Snapshot 负责：

- 某一次任务实际使用了哪个 Agent。
- 实际使用了哪个供应商。
- 实际使用了哪个模型。
- 实际使用了哪个协议格式。
- 实际使用了哪个提示词版本。
- 当时的配置来源是什么。

### 4.5 运行快照是可复现的关键

如果用户后来切换了默认模型，历史任务仍然需要知道当时的真实配置。

每次 AI 运行至少保存：

- 任务 ID。
- Agent ID。
- 供应商 ID。
- 模型 ID。
- 协议格式。
- Base URL 类型：官方、自定义、本地网关。
- 提示词版本。
- 输出 Schema 版本。
- 输入记录集合 ID。
- Token 或等价计量。
- 成本估算。
- 首字延迟。
- 总耗时。
- 重试次数。
- 成功、失败或待确认状态。

### 4.6 MCP 是工具能力，不是 MVP 核心卖点

WeSight 的 MCP 配置很完整，支持 stdio、sse、http transport，能发现工具并桥接到不同 Agent engine。

本项目当前决策仍保持：

- MVP 不让 AI 通过 MCP 直接调用 TikHub。
- MVP 采用“AI 生成采集计划，应用校验，应用调用 TikHub REST API”。
- TikHub MCP 放到 V1 高级 Agent 模式。
- MCP 模式必须记录工具名、入参、出参、费用、权限确认和失败边界。

## 5. 本项目落地规则

### 5.1 自然语言采集

自然语言入口必须是：

```text
用户自然语言需求
  -> AI 解析为结构化采集计划
  -> 应用校验平台、数据类型、国家地区、字段、成本和白名单
  -> 用户确认或修改
  -> 应用调用 TikHub REST API
  -> 应用内预览数据
  -> 用户保存到本地
  -> 用户导出 Excel 或 PDF
```

禁止：

- AI 直接持有 TikHub API Key。
- AI 直接发起正式 TikHub 采集。
- 用户未确认时产生正式采集费用。
- AI 猜测缺失的平台、数据类型、国家地区或时间范围后直接执行。

### 5.2 提示词优化

只导入 AI API Key 不够，MVP 版本就必须做提示词优化。提示词优化不是 V1 或高级能力，而是自然语言采集和 AI 整理可靠性的基础能力。优化目标不是“写得更像人”，而是工程稳定性：

- 让自然语言需求稳定解析成固定 Schema。
- 让模型在缺少条件时追问，而不是猜测。
- 让输出带来源引用。
- 让字段缺失、证据不足、冲突和失败进入明确状态。
- 降低 Token 成本。
- 降低非 JSON 输出概率。
- 固定提示词版本，方便复现历史结果。

MVP 提示词优化必须覆盖：

- 自然语言采集计划生成。
- 缺失条件追问。
- AI 整理模板。
- JSON Schema 结构化输出。
- 原始记录 ID 和记录集合 ID 的证据引用。
- 证据不足、字段冲突、输入过少、模型无法判断时的失败边界。
- 多供应商模型的稳定输出，不依赖某一家模型的私有行为。

MVP 不需要做复杂的自动 prompt search 或在线 A/B 测试，但必须提供提示词模板、版本号、变更说明、回滚能力和基础回归样例。

提示词优化不能绕过：

- 参数白名单。
- 成本确认。
- 用户确认。
- Schema 校验。
- 人工审核。
- 密钥隔离。

### 5.3 Excel 和 PDF 导出

Excel 是明细交付格式，PDF 是报告交付格式。

两者都必须反 slop：

- 核心洞察必须有证据记录 ID。
- AI 字段必须有模型运行记录。
- 待确认字段必须明确标注。
- 部分成功任务必须明确说明缺失范围。
- 不允许在导出中隐藏失败、阻塞、待确认状态。
- 不包含完整 API Key、完整 Header、未脱敏错误日志。

### 5.4 n8n 处理

n8n 不是核心卖点，但不能忽略：

- MVP 保留 Webhook 摘要。
- 帮助文档提供 n8n 接入示例。
- 不把 n8n 放到核心导航和主转化路径。
- Webhook 必须记录成功、失败和重试边界。
- Webhook 未发送成功时，不得提示“已同步”。

## 6. 已写入 PRD 的变化

本次学习已同步到 `plan/智能数据整理平台_PRD.md`：

- 版本更新为 v0.5。
- 产品方向从“三模型智能整理”改为“多供应商模型智能整理”。
- MVP 模型接入从三家固定平台扩展为内置三家官方供应商，加自定义 OpenAI-compatible 和更多扩展供应商。
- 新增供应商配置原则、推荐配置字段、模型适配层、运行快照。
- 新增反数据 slop 门禁。
- 新增字段级来源证据实体。
- 明确 MVP 必须包含提示词优化和提示词版本管理，不能只做模型 API Key 导入。
- 更新验收测试、异常测试、安全测试、风险与应对、数据边界和最终摘要。

## 7. 后续实现建议

优先级从高到低：

1. 先实现 Provider Registry，不要在 UI 和调用层散落硬编码供应商 URL。
2. 先实现 OpenAI-compatible 自定义供应商，因为它能覆盖大量模型平台和私有网关。
3. 采集计划 Schema、字段证据 Schema、运行快照 Schema 要先定，再做 UI。
4. 每个 AI 模板都必须在 MVP 阶段绑定输出 Schema 和提示词版本，并准备基础回归样例。
5. 在 Excel 和 PDF 导出前做报告数据完整性检查，缺证据时阻止核心洞察导出或明确标记待确认。
6. V1 再做 TikHub MCP 和更完整的 Agent 模式，MVP 不要让 AI 直接调用 TikHub。

## 8. 一句话原则

AI 可以加速整理，但不能替代证据；模型可以扩展，但配置必须可控；报告可以好看，但不能把不确定性包装成确定结论。
