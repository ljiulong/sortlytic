import { describe, expect, it } from 'vitest'
import { pageMeta } from './page-meta'

describe('pageMeta', () => {
  it('为每个页面提供独立标题和用途说明', () => {
    expect(pageMeta).toEqual({
      overview: {
        title: '首页',
        description: '查看真实连接、数据资产与本地运行状态。',
        layout: 'split',
      },
      'new-task': {
        title: '新建任务',
        description: '定义采集目标、筛选条件并生成可执行计划。',
        layout: 'single',
      },
      tasks: {
        title: '任务',
        description: '编辑、确认运行、取消或导出每一条真实任务。',
        layout: 'single',
      },
      settings: {
        title: '设置',
        description: '管理数据连接、模型与应用更新。',
        layout: 'single',
      },
      guide: {
        title: '使用指南',
        description: '按真实工作流完成连接、采集、运行与导出。',
        layout: 'single',
      },
    })
  })
})
