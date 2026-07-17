import type { NavKey } from './navigation'

export type PageMeta = {
  context: string
  title: string
  description: string
  layout: 'single' | 'split'
}

export const pageMeta: Record<NavKey, PageMeta> = {
  overview: {
    context: '运行概览',
    title: '首页',
    description: '查看真实连接、数据资产与本地运行状态。',
    layout: 'split',
  },
  'new-task': {
    context: '采集创建',
    title: '新建任务',
    description: '定义采集目标、筛选条件并生成可执行计划。',
    layout: 'single',
  },
  tasks: {
    context: '任务管理',
    title: '任务',
    description: '编辑、确认运行、取消或导出每一条真实任务。',
    layout: 'single',
  },
  settings: {
    context: '本地配置',
    title: '设置',
    description: '管理数据连接、模型与应用更新。',
    layout: 'single',
  },
  guide: {
    context: '操作手册',
    title: '使用指南',
    description: '按真实工作流完成连接、采集、运行与导出。',
    layout: 'single',
  },
}
