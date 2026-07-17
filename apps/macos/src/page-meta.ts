import type { NavKey } from './navigation'

export type PageMeta = {
  title: string
  description: string
}

export const pageMeta: Record<NavKey, PageMeta> = {
  overview: {
    title: '首页',
    description: '查看真实连接、数据资产与本地运行状态。',
  },
  'new-task': {
    title: '新建任务',
    description: '定义采集目标、筛选条件并生成可执行计划。',
  },
  tasks: {
    title: '任务',
    description: '编辑、确认运行、取消或导出每一条真实任务。',
  },
  settings: {
    title: '设置',
    description: '管理数据连接、模型与应用更新。',
  },
  guide: {
    title: '使用指南',
    description: '按真实工作流完成连接、采集、运行与导出。',
  },
}
