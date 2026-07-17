import { describe, expect, it } from 'vitest'
import { primaryNavigation } from './navigation'

describe('primaryNavigation', () => {
  it('将首页、新建任务、任务和设置拆成独立入口', () => {
    expect(primaryNavigation.map((item) => item.key)).toEqual([
      'overview',
      'new-task',
      'tasks',
      'settings',
    ])
    expect(primaryNavigation.map((item) => item.label)).toEqual([
      '首页',
      '新建任务',
      '任务',
      '设置',
    ])
  })
})
