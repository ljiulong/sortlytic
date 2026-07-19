import { ChevronDown, Search, SlidersHorizontal } from 'lucide-react'
import { useId, useMemo, useState } from 'react'
import type {
  AccountCollectionCapabilityView,
  AccountFieldCapabilityView,
} from './backend-api'
import './AccountFieldPicker.css'

const baseFieldCount = 6

type AccountFieldPickerProps = {
  capability?: AccountCollectionCapabilityView
  error?: string
  isLoading?: boolean
  onChange: (fields: string[]) => void
  selectedFields: string[]
}

function searchableText(field: AccountFieldCapabilityView) {
  return `${field.display_name} ${field.key} ${field.description}`.toLocaleLowerCase()
}

function AccountFieldPicker({
  capability,
  error,
  isLoading = false,
  onChange,
  selectedFields,
}: AccountFieldPickerProps) {
  const contentId = useId()
  const [expanded, setExpanded] = useState(false)
  const [query, setQuery] = useState('')
  const [activeGroup, setActiveGroup] = useState('')
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set())
  const availableFields = useMemo(
    () => capability?.fields.filter((field) => field.availability !== 'unsupported') ?? [],
    [capability],
  )
  const selected = useMemo(() => new Set(selectedFields), [selectedFields])
  const defaultFields = useMemo(
    () => availableFields.filter((field) => field.default_selected).map((field) => field.key),
    [availableFields],
  )
  const effectiveGroup = activeGroup || capability?.field_groups[0]?.key || ''
  const normalizedQuery = query.trim().toLocaleLowerCase()
  const filteredFields = useMemo(() => capability?.fields.filter(
    (field) => !normalizedQuery || searchableText(field).includes(normalizedQuery),
  ) ?? [], [capability, normalizedQuery])
  const enrichmentCount = availableFields.filter(
    (field) => selected.has(field.key) && field.required_operation_keys.length > 0,
  ).length
  const isCorePreset = selectedFields.length === defaultFields.length
    && defaultFields.every((field) => selected.has(field))
  const unavailable = isLoading || Boolean(error) || !capability

  const emitSelection = (keys: Iterable<string>) => {
    const next = new Set(keys)
    onChange(availableFields.filter((field) => next.has(field.key)).map((field) => field.key))
  }

  const toggleField = (key: string) => {
    const next = new Set(selected)
    if (next.has(key)) next.delete(key)
    else next.add(key)
    emitSelection(next)
  }

  const toggleGroup = (groupKey: string) => {
    const fields = availableFields.filter((field) => field.group === groupKey)
    const allSelected = fields.length > 0 && fields.every((field) => selected.has(field.key))
    const next = new Set(selected)
    for (const field of fields) {
      if (allSelected) next.delete(field.key)
      else next.add(field.key)
    }
    emitSelection(next)
  }

  const toggleCollapsed = (groupKey: string) => {
    setCollapsedGroups((current) => {
      const next = new Set(current)
      if (next.has(groupKey)) next.delete(groupKey)
      else next.add(groupKey)
      return next
    })
  }

  return (
    <section className="account-field-picker" aria-label="结果字段">
      <div className="account-field-picker__summary">
        <div>
          <strong>结果字段</strong>
          <span>{baseFieldCount} 个基础字段 + {selectedFields.length} 个扩展字段</span>
        </div>
        <div className="account-field-picker__summary-meta">
          <span>{isCorePreset ? '核心字段预设' : '自定义字段'}</span>
          <span>{enrichmentCount > 0 ? `其中 ${enrichmentCount} 项需要补全请求` : '无需额外补全请求'}</span>
        </div>
        <button
          type="button"
          aria-controls={contentId}
          aria-expanded={expanded}
          disabled={unavailable}
          onClick={() => setExpanded((value) => !value)}
        >
          <SlidersHorizontal size={15} aria-hidden="true" />
          {expanded ? '收起字段' : '配置字段'}
        </button>
      </div>

      {isLoading ? <p className="account-field-picker__state" role="status">正在读取当前平台字段能力</p> : null}
      {error ? <p className="account-field-picker__state" data-tone="danger" role="alert">字段能力读取失败，请重试</p> : null}
      {!isLoading && !error && capability?.fields.length === 0 ? (
        <p className="account-field-picker__state" role="status">当前平台没有可配置的账号字段</p>
      ) : null}

      {expanded && capability ? (
        <div className="account-field-picker__content" id={contentId}>
          <div className="account-field-picker__toolbar">
            <label className="account-field-picker__search">
              <Search size={14} aria-hidden="true" />
              <span className="account-field-picker__visually-hidden">搜索字段</span>
              <input
                type="search"
                placeholder="搜索名称、字段代码或说明"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
            <div className="account-field-picker__quick-actions">
              <button type="button" onClick={() => emitSelection(defaultFields)}>恢复核心字段</button>
              <button type="button" onClick={() => emitSelection(availableFields.map((field) => field.key))}>
                选择全部可用字段
              </button>
            </div>
          </div>

          <div className="account-field-picker__layout">
            <nav className="account-field-picker__groups" aria-label="字段分类">
              {capability.field_groups.map((group) => {
                const fields = availableFields.filter((field) => field.group === group.key)
                const selectedCount = fields.filter((field) => selected.has(field.key)).length
                return (
                  <button
                    type="button"
                    aria-current={effectiveGroup === group.key ? 'true' : undefined}
                    key={group.key}
                    onClick={() => setActiveGroup(group.key)}
                  >
                    <span>{group.display_name}</span>
                    <small>{selectedCount}/{fields.length}</small>
                  </button>
                )
              })}
            </nav>

            <div className="account-field-picker__panels">
              {capability.field_groups.map((group) => {
                const allGroupFields = capability.fields.filter((field) => field.group === group.key)
                const availableGroupFields = availableFields.filter((field) => field.group === group.key)
                const visibleFields = filteredFields.filter((field) => field.group === group.key)
                const selectedCount = availableGroupFields.filter((field) => selected.has(field.key)).length
                const collapsed = collapsedGroups.has(group.key)
                return (
                  <section
                    className="account-field-picker__panel"
                    data-active={effectiveGroup === group.key}
                    data-collapsed={collapsed}
                    key={group.key}
                  >
                    <header className="account-field-picker__group-header">
                      <button
                        type="button"
                        aria-controls={`${contentId}-${group.key}`}
                        aria-expanded={!collapsed}
                        onClick={() => toggleCollapsed(group.key)}
                      >
                        <span>
                          <strong>{group.display_name}</strong>
                          <small>已选 {selectedCount}/{availableGroupFields.length}</small>
                        </span>
                        <ChevronDown size={15} aria-hidden="true" />
                      </button>
                      <button
                        type="button"
                        disabled={availableGroupFields.length === 0}
                        onClick={() => toggleGroup(group.key)}
                      >
                        {availableGroupFields.length > 0
                          && availableGroupFields.every((field) => selected.has(field.key))
                          ? '取消本组'
                          : '选择本组'}
                      </button>
                    </header>
                    <div className="account-field-picker__rows" id={`${contentId}-${group.key}`}>
                      {visibleFields.map((field) => {
                        const unsupported = field.availability === 'unsupported'
                        return (
                          <label
                            className="account-field-picker__row"
                            data-selected={selected.has(field.key)}
                            data-disabled={unsupported}
                            key={field.key}
                          >
                            <input
                              type="checkbox"
                              checked={selected.has(field.key)}
                              disabled={unsupported}
                              onChange={() => toggleField(field.key)}
                            />
                            <span className="account-field-picker__field-copy">
                              <span>
                                <strong>{field.display_name}</strong>
                                <code>{field.key}</code>
                              </span>
                              <small>{field.description}</small>
                            </span>
                            <FieldStatus field={field} />
                          </label>
                        )
                      })}
                      {visibleFields.length === 0 ? (
                        <p className="account-field-picker__empty">本分类没有匹配字段</p>
                      ) : null}
                      {allGroupFields.length === 0 ? (
                        <p className="account-field-picker__empty">本分类暂无字段</p>
                      ) : null}
                    </div>
                  </section>
                )
              })}
            </div>
          </div>
        </div>
      ) : null}
    </section>
  )
}

function FieldStatus({ field }: { field: AccountFieldCapabilityView }) {
  if (field.availability === 'enrichment') {
    return <span className="account-field-picker__status" data-tone="warning">需补全，会增加请求</span>
  }
  if (field.availability === 'conditional') {
    return <span className="account-field-picker__status" data-tone="info">接口可能不返回</span>
  }
  if (field.availability === 'unsupported') {
    return (
      <span className="account-field-picker__status" data-tone="muted">
        {field.missing_reason || '当前平台不支持'}
      </span>
    )
  }
  return <span className="account-field-picker__status">直接提供</span>
}

export default AccountFieldPicker
