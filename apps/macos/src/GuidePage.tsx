import {
  BadgeCheck,
  CheckCircle2,
  ExternalLink,
  KeyRound,
  MonitorCheck,
  ShieldCheck,
} from 'lucide-react'
import './GuidePage.css'

type GuidePageProps = {
  onOpenSettings: () => void
}

type GuideChapter = {
  id: string
  title: string
  summary: string
  procedures: Array<{ title: string; detail: string }>
  facts?: Array<{ label: string; value: string }>
  action?: 'settings'
}

const guideChapters: GuideChapter[] = [
  {
    id: 'workspace',
    title: '准备本地工作区',
    summary: '先确认本机边界和首次验证范围，避免一开始就创建大任务。',
    procedures: [
      {
        title: '确认运行环境',
        detail: '当前 MVP 只支持 macOS 单端本地工作区，不需要注册 Sortlytic 账号，也不会自动同步到远端。',
      },
      {
        title: '准备服务凭据',
        detail: '采集需要已验证的 TikHub 账号和 API Token。旧配置需要重新输入 TikHub Token 和 AI API Key 一次，应用不会读取或删除旧钥匙串条目。',
      },
      {
        title: '确定最小验证目标',
        detail: '第一次只选一个平台、一种数据类型和 10 到 50 条记录，确认结果字段与费用后再扩大范围。',
      },
    ],
    facts: [
      { label: '数据平台', value: 'TikTok、抖音、小红书' },
      { label: '本地数据', value: '任务、结果、配置引用和导出文件均留在当前 Mac。' },
      { label: '新建任务', value: '表单和自然语言入口初始均为空，不预填任何具体任务。' },
      { label: '首次样本', value: '建议 10 到 50 条，并设置明确成本上限。' },
    ],
  },
  {
    id: 'tikhub',
    title: '配置 TikHub 数据来源',
    summary: '完成账号验证、Token 保存和真实连通测试后，采集计划才能读取实时价格与额度。',
    procedures: [
      {
        title: '注册并验证账号',
        detail: '在 TikHub 注册页创建账号并完成邮箱验证；未验证账号可能无法正常调用接口。',
      },
      {
        title: '创建 API Token',
        detail: '登录用户中心后新建 Token 并立即复制。不要把完整 Token 放进文档、聊天、截图或任务名称。',
      },
      {
        title: '选择 API 域名',
        detail: '国际网络优先使用 api.tikhub.io，中国大陆网络可尝试 api.tikhub.dev；最终以真实连通测试为准。',
      },
      {
        title: '保存并测试',
        detail: '在设置中保存 Token 后执行测试，成功状态应返回真实账号、充值余额、免费额度与今日用量。',
      },
    ],
    facts: [
      { label: '请求头', value: 'Authorization' },
      { label: '值格式', value: 'Bearer YOUR_API_KEY' },
      { label: '保存方式', value: 'Token 以明文写入当前工作区私有 JSON，不进入数据库、日志、导出或 Webhook。' },
      { label: '费用依据', value: '不同端点价格可能不同，创建计划时读取实时计价与双额度。' },
    ],
    action: 'settings',
  },
  {
    id: 'model',
    title: '配置 AI 处理',
    summary: '本版本只保存、校验和切换 AI 配置，不发起真实模型请求。',
    procedures: [
      {
        title: '添加 AI 配置',
        detail: '填写唯一名称、供应商、API 格式、Base URL、默认模型 ID 和密钥。Ollama 可以不填密钥。',
      },
      {
        title: '执行本地校验',
        detail: '应用只检查协议、地址、模型和密钥是否完整，不调用供应商 API。未通过校验的配置不能设为当前配置。',
      },
      {
        title: '切换当前配置',
        detail: '可以保存多个同类供应商、账号或端点，每次只有一个当前 AI 配置。切换不会修改其他配置。',
      },
      {
        title: '了解当前边界',
        detail: '当前自然语言计划仍由本地规则引擎生成。未来接入模型执行时，再引入提示词版本、Schema 约束和来源证据。',
      },
    ],
    facts: [
      { label: '当前能力', value: '保存、查看、编辑、本地校验和切换 AI 配置。' },
      { label: '校验方式', value: '只校验配置完整性，不发起真实模型请求。' },
      { label: '任务边界', value: '当前规则引擎不会调用 AI 模型。' },
      { label: '密钥边界', value: 'AI API Key 与 TikHub Token 使用同一个工作区私有 JSON。' },
    ],
  },
  {
    id: 'create-task',
    title: '创建并校验任务',
    summary: '生成计划只做参数校验、实时计价和额度预检，不会自动入队或产生正式采集费用。',
    procedures: [
      {
        title: '选择任务入口',
        detail: '表单式适合逐项控制；自然语言适合先描述目标。两种入口最终生成同一种可审查计划。',
      },
      {
        title: '定义来源与目标',
        detail: '选择 TikTok、抖音或小红书，并勾选搜索结果账号、作品作者、账号公开信息、账号作品或评论用户。',
      },
      {
        title: '选择国家或地区',
        detail: '列表包含全部 249 个 ISO 两位代码，可搜索中文名、英文名或两位代码；目标接口不支持地区时控件会明确禁用。',
      },
      {
        title: '设置公开信息筛选',
        detail: '年龄和性别默认关闭。启用后只接受明确公开年龄与明确公开性别，未知、异常或推断值不会进入结果。',
      },
      {
        title: '限制数量与成本',
        detail: '填写时间范围、最大记录数和美元成本上限。首次任务保持小样本，确认请求估算后再扩大。',
      },
      {
        title: '生成并检查计划',
        detail: '逐项检查平台、数据类型、地区、公开筛选、请求估算、实时价格、余额和阻塞原因。',
      },
    ],
    facts: [
      { label: '计划生成', value: '只保存计划，不自动运行。' },
      { label: '地区值', value: '界面显示中英文名称，提交标准 ISO 两位代码。' },
      { label: '年龄口径', value: '启用后使用一个包含上下限的闭区间。' },
      { label: '性别口径', value: '禁止根据头像、姓名、简介或其他线索推断。' },
    ],
  },
  {
    id: 'run-task',
    title: '确认运行与管理任务',
    summary: '任务页集中处理编辑、确认运行、取消、删除和状态追踪，每个动作都有独立语义。',
    procedures: [
      {
        title: '编辑待确认任务',
        detail: '在运行前修改名称、平台或数据类型。范围变化会撤销旧确认，必须重新生成或确认有效计划。',
      },
      {
        title: '确认运行',
        detail: '核对计划与费用后点击确认运行。只有这一步完成后任务才进入运行队列并可能产生正式采集费用。',
      },
      {
        title: '读取状态与进度',
        detail: '卡片显示等待确认、排队、运行、成功、部分成功、失败或已取消，并显示进度、请求与结果数量。',
      },
      {
        title: '取消任务',
        detail: '取消用于停止尚未结束的任务并保留运行记录，取消确认不会等同于删除。',
      },
      {
        title: '删除任务',
        detail: '删除用于从任务列表和本地工作区移除任务及关联记录，必须二次确认；正在运行的任务应先取消，终态任务也可删除。',
      },
      {
        title: '处理失败任务',
        detail: '先查看阻塞原因、端点状态、额度与错误阶段，再决定修改计划、重试或删除，不要盲目重复运行。',
      },
    ],
    facts: [
      { label: '取消', value: '停止执行，保留任务与审计线索。' },
      { label: '删除', value: '二次确认后移除本地任务及关联数据。' },
      { label: '部分成功', value: '有合格结果但部分目标失败，可导出并同时查看失败证据。' },
      { label: '运行确认', value: '确认态与普通操作态保持同一稳定卡片高度。' },
    ],
  },
  {
    id: 'export',
    title: '按任务导出与复核',
    summary: '导出不设首页或全局门禁，每条任务独立选择格式并生成对应文件。',
    procedures: [
      {
        title: '选择导出格式',
        detail: '在具体任务卡中选择 Excel 或 PDF。表格型对外数据默认使用 Excel 工作簿，PDF 用于阅读型报告。',
      },
      {
        title: '生成文件',
        detail: '成功或部分成功任务可导出；生成状态、文件路径与失败原因应在当前任务上下文中显示。',
      },
      {
        title: '复核 Excel',
        detail: '检查工作表结构、字段类型、行数、国家地区代码、来源链接和缺失值，不把空值伪装成已验证结果。',
      },
      {
        title: '复核 PDF',
        detail: '检查标题、摘要、分页、来源证据与中文换行，确认报告只陈述有证据支持的结论。',
      },
      {
        title: '保存与备份',
        detail: '导出文件留在当前 Mac。需要迁移或归档时，由用户明确选择位置并自行纳入备份策略。',
      },
    ],
    facts: [
      { label: 'Excel', value: '适合结构化明细、筛选、计算和运营交付。' },
      { label: 'PDF', value: '适合固定版式报告、阅读和审阅。' },
      { label: '证据检查', value: '保留采集流程产生的来源证据；本版本不产生 AI 模型运行信息。' },
      { label: '导出边界', value: '每条任务独立选择一种格式，不强制同时生成两种文件。' },
    ],
  },
]

const safetyChecks = [
  '密钥以明文写入当前工作区私有 JSON，不进入数据库、日志、导出或 Webhook；目录权限为 0700，文件权限为 0600。',
  '年龄与性别筛选只使用接口或公开资料明确返回的值，禁止从头像、姓名或简介推断。',
  '生成计划不等于开始运行，必须完成费用与范围复核后再确认运行。',
  '删除任务会移除本地关联数据，确认前先完成需要的 Excel 或 PDF 导出。',
  '本地优先不等于自动备份；重要结果需要由用户纳入自己的备份方案。',
]

const troubleshootingItems = [
  {
    symptom: 'TikHub 测试失败',
    action: '依次检查邮箱验证、Token 是否完整、当前网络对应域名、账号额度和系统时间，再重新测试。',
  },
  {
    symptom: '地区控件不可用',
    action: '先选择平台和数据类型。只有目标接口明确支持地区参数时，249 个代码列表才会启用。',
  },
  {
    symptom: '计划不能确认运行',
    action: '查看计划底部第一条阻塞原因，补齐范围、实时计价或额度，不要重复点击确认。',
  },
  {
    symptom: '任务没有结果',
    action: '检查任务状态、失败阶段、请求数和公开筛选。过窄地区、年龄或性别条件可能得到真实空结果。',
  },
  {
    symptom: '导出按钮不可用',
    action: '确认任务已成功或部分成功，并且存在可导出的真实记录；失败、排队和运行中任务不能生成最终文件。',
  },
]

const officialResources = [
  { href: 'https://user.tikhub.io/register', label: '创建 TikHub 账号' },
  { href: 'https://user.tikhub.io/login', label: '登录用户中心' },
  { href: 'https://docs.tikhub.io/', label: '查看 API 文档' },
  { href: 'https://tikhub.io/getting-started', label: '首次调用指南' },
  { href: 'https://tikhub.io/pricing', label: '价格与免费额度' },
]

function GuidePage({ onOpenSettings }: GuidePageProps) {
  return (
    <section className="guide-page" aria-label="使用指南">
      <header className="guide-intro">
        <div className="guide-intro__copy">
          <p className="eyebrow">Sortlytic 本地操作手册</p>
          <h2>从首次配置到任务导出</h2>
          <p>按六个阶段完成真实配置、计划校验、人工确认、任务管理与文件复核。</p>
        </div>
        <span className="status-pill" data-tone="info">
          <MonitorCheck size={13} aria-hidden="true" />
          macOS 本地工作区
        </span>
        <dl className="guide-intro__facts">
          <div><dt>开始产生采集费用</dt><dd>确认运行之后</dd></div>
          <div><dt>地区代码</dt><dd>249 个 ISO 两位代码</dd></div>
          <div><dt>默认数据交付</dt><dd>Excel 工作簿</dd></div>
        </dl>
      </header>

      <nav className="guide-index" aria-label="使用指南章节">
        <div>
          <p className="eyebrow">六阶段工作流</p>
          <strong>按顺序执行</strong>
        </div>
        <ol>
          {guideChapters.map((chapter, index) => (
            <li key={chapter.id}>
              <a href={`#guide-${chapter.id}`}>
                <span>{String(index + 1).padStart(2, '0')}</span>
                {chapter.title}
              </a>
            </li>
          ))}
        </ol>
      </nav>

      <main className="guide-handbook">
        <ol className="guide-chapters">
          {guideChapters.map((chapter, index) => (
            <li className="guide-chapter" id={`guide-${chapter.id}`} key={chapter.id}>
              <div className="guide-chapter__rail" aria-hidden="true">
                <span>{String(index + 1).padStart(2, '0')}</span>
              </div>
              <article className="guide-chapter__content">
                <header>
                  <p className="eyebrow">第 {index + 1} 阶段</p>
                  <h2>{chapter.title}</h2>
                  <p>{chapter.summary}</p>
                </header>
                <ol className="guide-procedure">
                  {chapter.procedures.map((procedure, procedureIndex) => (
                    <li key={procedure.title}>
                      <span>{procedureIndex + 1}</span>
                      <strong>{procedure.title}</strong>
                      <p>{procedure.detail}</p>
                    </li>
                  ))}
                </ol>
                {chapter.facts ? (
                  <dl className="guide-facts">
                    {chapter.facts.map((fact) => (
                      <div key={fact.label}>
                        <dt>{fact.label}</dt>
                        <dd>{fact.value}</dd>
                      </div>
                    ))}
                  </dl>
                ) : null}
                {chapter.action === 'settings' ? (
                  <button className="ghost-button" type="button" onClick={onOpenSettings}>
                    <KeyRound size={16} aria-hidden="true" />
                    打开设置
                  </button>
                ) : null}
              </article>
            </li>
          ))}
        </ol>
      </main>

      <section className="guide-troubleshooting" aria-labelledby="guide-troubleshooting-heading">
        <header>
          <p className="eyebrow">常见问题</p>
          <h2 id="guide-troubleshooting-heading">按症状定位阻塞点</h2>
        </header>
        <dl>
          {troubleshootingItems.map((item) => (
            <div key={item.symptom}>
              <dt>{item.symptom}</dt>
              <dd>{item.action}</dd>
            </div>
          ))}
        </dl>
      </section>

      <div className="guide-reference-grid">
        <section className="guide-checklist" aria-labelledby="guide-checklist-heading">
          <header>
            <div>
              <p className="eyebrow">运行前确认</p>
              <h2 id="guide-checklist-heading">安全与成本边界</h2>
            </div>
            <ShieldCheck size={19} aria-hidden="true" />
          </header>
          <ul>
            {safetyChecks.map((item) => (
              <li key={item}>
                <CheckCircle2 size={16} aria-hidden="true" />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </section>

        <section className="guide-resources" aria-labelledby="guide-resources-heading">
          <header>
            <div>
              <p className="eyebrow">官方资源</p>
              <h2 id="guide-resources-heading">注册、文档与计价</h2>
            </div>
            <BadgeCheck size={18} aria-hidden="true" />
          </header>
          <nav aria-label="TikHub 官方资源">
            {officialResources.map((resource) => (
              <GuideLink {...resource} key={resource.href} />
            ))}
          </nav>
        </section>
      </div>
    </section>
  )
}

function GuideLink({ href, label }: { href: string; label: string }) {
  return (
    <a className="guide-resource-link" href={href} rel="noreferrer" target="_blank">
      <span>{label}</span>
      <ExternalLink size={15} aria-hidden="true" />
    </a>
  )
}

export default GuidePage
