import type { WorkspaceHealthCheckView } from './backend-api'

export function describeWorkspaceHealth(health: WorkspaceHealthCheckView) {
  const issues: string[] = []
  if (health.database_quick_check.toLowerCase() !== 'ok') issues.push('数据库完整性检查未通过')
  if (!health.foreign_keys_enabled) issues.push('外键约束未启用')
  if (health.journal_mode.toLowerCase() !== 'wal') issues.push('数据库未使用 WAL 模式')
  if (health.missing_directories.length > 0) {
    issues.push(`缺少目录：${health.missing_directories.join('、')}`)
  }
  if (!health.database_writable) issues.push('数据库写入验证失败')
  return issues.length > 0
    ? { passed: false, message: `工作区健康检查发现异常：${issues.join('；')}` }
    : { passed: true, message: '工作区健康检查通过' }
}
