export const platformOptions = ['TikTok', '抖音', '小红书'] as const
export const dataTypeOptions = ['关键词搜索', '账号公开信息', '评论采集', '笔记详情'] as const

export type Platform = (typeof platformOptions)[number]
export type DataType = (typeof dataTypeOptions)[number]
export type TaskStatus = '运行中' | '等待确认' | '部分成功' | '成功' | '待人工确认' | '失败'
export type NavKey = 'overview' | 'guide' | 'settings'
export type Tone = 'success' | 'warning' | 'danger' | 'info'
export type ConnectionIcon = 'key' | 'bot' | 'share'

export type SocialRecord = {
  id: string
  platform: Platform
  title: string
  author: string
  region: string
  status: '已校验' | '待人工确认' | '证据不足'
  sentiment: '正向' | '中性' | '负向'
  confidence: number
  engagement: number
  source: string
  insight: string
  evidence: string
}

export type CollectionPlan = {
  platform: Platform
  dataType: DataType
  regionCode: string
  keyword: string
  range: string
  maxRecords: number
  budget: number
  status: TaskStatus
  missing: string[]
}

export type WorkbenchSnapshot = typeof workspaceSnapshot

export const workspaceSnapshot = {
  workspace: {
    name: '本地研究工作区',
    storage: '18.6 GB',
    lastBackup: '2026-07-05 02:12',
    health: '可用',
  },
  connections: [
    {
      name: 'TikHub',
      detail: 'REST API',
      status: '已连接',
      tone: 'success',
      icon: 'key',
      meta: '尾号 sk_92',
    },
    {
      name: 'OpenAI-compatible',
      detail: '默认模型 qwen3',
      status: '已连接',
      tone: 'success',
      icon: 'bot',
      meta: '结构化输出',
    },
    {
      name: 'Webhook',
      detail: 'n8n 轻集成',
      status: '未启用',
      tone: 'warning',
      icon: 'share',
      meta: '仅发送摘要',
    },
  ],
  metrics: [
    { label: '今日任务', value: '12', delta: '3 个待确认', tone: 'info' },
    { label: '入库记录', value: '8,742', delta: '96.4% 已校验', tone: 'success' },
    { label: '预计成本', value: '$47.82', delta: '低于上限 28%', tone: 'warning' },
    { label: '证据覆盖', value: '100%', delta: '核心洞察有来源', tone: 'success' },
  ],
  tasks: [
    {
      id: 'demo-task-xhs-ev-comments',
      name: '小红书新能源汽车评论洞察',
      platform: '小红书',
      status: '运行中',
      source: '自然语言',
      progress: 68,
      records: 1480,
      cost: '$12.40',
    },
    {
      id: 'demo-task-tiktok-camping',
      name: 'TikTok camping gear trend',
      platform: 'TikTok',
      status: '等待确认',
      source: '表单式',
      progress: 32,
      records: 600,
      cost: '$7.20',
    },
    {
      id: 'demo-task-douyin-live-accounts',
      name: '抖音直播候选账号校验',
      platform: '抖音',
      status: '部分成功',
      source: '自然语言',
      progress: 84,
      records: 2310,
      cost: '$18.10',
    },
  ],
  records: [
    {
      id: 'rec-101',
      platform: '小红书',
      title: '城市通勤车主对续航的真实反馈',
      author: '南山观察员',
      region: 'CN',
      status: '已校验',
      sentiment: '中性',
      confidence: 0.91,
      engagement: 2841,
      source: 'https://example.local/xhs/101',
      insight: '续航焦虑集中在冬季与高速场景，评论区更关注补能体验。',
      evidence: '评论集合 cm-778 引用 46 条，命中主题：补能、冬季、价格。',
    },
    {
      id: 'rec-102',
      platform: 'TikTok',
      title: 'Compact EV road trip checklist',
      author: 'Maya Ortega',
      region: 'US',
      status: '已校验',
      sentiment: '正向',
      confidence: 0.88,
      engagement: 3914,
      source: 'https://example.local/tiktok/102',
      insight: '海外用户把露营配件和电动车储物能力放在同一决策链路。',
      evidence: '原始记录 raw-102、raw-119、raw-144 支持该主题。',
    },
    {
      id: 'rec-103',
      platform: '抖音',
      title: '智能座舱语音交互误触反馈',
      author: '车机体验笔记',
      region: 'CN',
      status: '待人工确认',
      sentiment: '负向',
      confidence: 0.67,
      engagement: 1266,
      source: 'https://example.local/douyin/103',
      insight: '负向情绪可能来自语音误触和方言识别失败，证据仍需抽检。',
      evidence: '证据记录 raw-203 缺少评论上下文，进入人工确认。',
    },
    {
      id: 'rec-104',
      platform: '小红书',
      title: '女性车主购车决策中的安全感表达',
      author: '栗子调研所',
      region: 'CN',
      status: '已校验',
      sentiment: '正向',
      confidence: 0.94,
      engagement: 4728,
      source: 'https://example.local/xhs/104',
      insight: '“安全感”被反复绑定到视野、辅助驾驶、售后响应和社区口碑。',
      evidence: '字段级证据 fp-441、fp-447、fp-452 已通过 Schema 校验。',
    },
    {
      id: 'rec-105',
      platform: 'TikTok',
      title: 'EV charging wait time discussion',
      author: 'Lars Eriksson',
      region: 'SE',
      status: '证据不足',
      sentiment: '中性',
      confidence: 0.58,
      engagement: 847,
      source: 'https://example.local/tiktok/105',
      insight: '充电等待时间被提及，但样本不足以支持地区级结论。',
      evidence: '仅 4 条记录命中主题，导出时标记为待确认。',
    },
  ],
  promptRuns: [
    { name: '采集计划 Schema', status: '通过', provider: 'OpenAI-compatible', diff: '0 个字段漂移' },
    { name: '缺少国家/地区追问', status: '通过', provider: 'Qwen', diff: '生成追问' },
    { name: '证据引用边界', status: '失败', provider: 'Gemini', diff: '1 条洞察缺少记录 ID' },
    { name: '非 JSON 输出处理', status: '通过', provider: 'Claude', diff: '保留原始输出' },
  ],
} satisfies {
  workspace: {
    name: string
    storage: string
    lastBackup: string
    health: string
  }
  connections: Array<{
    name: string
    detail: string
    status: string
    tone: Tone
    icon: ConnectionIcon
    meta: string
  }>
  metrics: Array<{ label: string; value: string; delta: string; tone: Tone }>
  tasks: Array<{
    id: string
    name: string
    platform: Platform
    status: TaskStatus
    source: string
    progress: number
    records: number
    cost: string
  }>
  records: SocialRecord[]
  promptRuns: Array<{ name: string; status: '通过' | '失败'; provider: string; diff: string }>
}
