export const primaryNavigation = [
  { key: 'overview', label: '首页', icon: 'home' },
  { key: 'new-task', label: '新建任务', icon: 'new-task' },
  { key: 'tasks', label: '任务', icon: 'tasks' },
  { key: 'settings', label: '设置', icon: 'settings' },
] as const

export type PrimaryNavKey = (typeof primaryNavigation)[number]['key']
export type NavKey = PrimaryNavKey | 'guide'
