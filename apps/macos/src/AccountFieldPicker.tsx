import { ChevronDown, Search, SlidersHorizontal } from 'lucide-react'
import { useId, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  AccountCollectionCapabilityView,
  AccountFieldCapabilityView,
} from './backend-api'
import './AccountFieldPicker.css'
import type { AccountSourceKey } from './collection-options'
import { i18n } from './i18n'

const baseFieldCount = 6

type AccountFieldPickerProps = {
  accountSource?: AccountSourceKey
  capability?: AccountCollectionCapabilityView
  error?: string
  isLoading?: boolean
  onChange: (fields: string[]) => void
  selectedFields: string[]
}

function AccountFieldPicker({
  accountSource,
  capability,
  error,
  isLoading = false,
  onChange,
  selectedFields,
}: AccountFieldPickerProps) {
  const { t } = useTranslation('collection', { i18n })
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
  const normalizedQuery = query.trim().toLocaleLowerCase()
  const fieldName = (field: AccountFieldCapabilityView) => String(t(
    `accountFields.${field.key}.label`,
    { defaultValue: field.label ?? field.display_name },
  ))
  const fieldDescription = (field: AccountFieldCapabilityView) => String(t(
    `accountFields.${field.key}.description`,
    { defaultValue: field.description },
  ))
  const groupName = (key: string, fallback: string) => String(t(
    `accountFieldGroups.${key}`,
    { defaultValue: fallback },
  ))
  const unsupportedDetail = (field: AccountFieldCapabilityView) => t(
    'accountFields.unsupportedDetail',
    {
      platforms: (field.supported_platforms ?? []).map((platform) => t(
        `accountSources.platform.${platform}`,
        { defaultValue: platform },
      )).join(t('preview.listSeparator')),
      reason: field.missing_reason || t('accountFields.status.unsupported'),
    },
  )
  const filteredFields = capability?.fields.filter(
    (field) => !normalizedQuery || [
      field.display_name,
      field.key,
      field.description,
      fieldName(field),
      fieldDescription(field),
    ].join(' ').toLocaleLowerCase().includes(normalizedQuery),
  ) ?? []
  const searchedGroup = normalizedQuery
    ? capability?.field_groups.find((group) => (
      filteredFields.some((field) => field.group === group.key)
    ))?.key
    : undefined
  const effectiveGroup = searchedGroup || activeGroup || capability?.field_groups[0]?.key || ''
  const enrichmentCount = availableFields.filter(
    (field) => selected.has(field.key)
      && field.required_operation_keys.length > 0
      && !field.covered_by_source_keys?.includes(accountSource ?? ''),
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
    <section className="account-field-picker" aria-label={t('accountFields.title')}>
      <div className="account-field-picker__summary">
        <div>
          <strong>{t('accountFields.title')}</strong>
          <span>{t('accountFields.summary', { base: baseFieldCount, count: selectedFields.length })}</span>
        </div>
        <div className="account-field-picker__summary-meta">
          <span>{t(isCorePreset ? 'accountFields.corePreset' : 'accountFields.customPreset')}</span>
          <span>{!accountSource
            ? t('accountFields.enrichmentPendingSource')
            : enrichmentCount > 0
              ? t('accountFields.enrichmentCount', { count: enrichmentCount })
              : t('accountFields.noEnrichment')}</span>
        </div>
        <button
          type="button"
          aria-controls={contentId}
          aria-expanded={expanded}
          disabled={unavailable}
          onClick={() => setExpanded((value) => !value)}
        >
          <SlidersHorizontal size={15} aria-hidden="true" />
          {t(expanded ? 'accountFields.collapse' : 'accountFields.configure')}
        </button>
      </div>

      {isLoading ? <p className="account-field-picker__state" role="status">{t('accountFields.loading')}</p> : null}
      {error ? <p className="account-field-picker__state" data-tone="danger" role="alert">{t('accountFields.loadFailed')}</p> : null}
      {!isLoading && !error && capability?.fields.length === 0 ? (
        <p className="account-field-picker__state" role="status">{t('accountFields.noFields')}</p>
      ) : null}

      {expanded && capability ? (
        <div className="account-field-picker__content" id={contentId}>
          <div className="account-field-picker__toolbar">
            <label className="account-field-picker__search">
              <Search size={14} aria-hidden="true" />
              <span className="account-field-picker__visually-hidden">{t('accountFields.searchLabel')}</span>
              <input
                type="search"
                placeholder={t('accountFields.searchPlaceholder')}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
            <div className="account-field-picker__quick-actions">
              <button type="button" onClick={() => emitSelection(defaultFields)}>{t('accountFields.restoreCore')}</button>
              <button type="button" onClick={() => emitSelection(availableFields.map((field) => field.key))}>
                {t('accountFields.selectAll')}
              </button>
            </div>
          </div>

          <div className="account-field-picker__layout">
            <nav className="account-field-picker__groups" aria-label={t('accountFields.groupNavigation')}>
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
                    <span>{groupName(group.key, group.display_name)}</span>
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
                          <strong>{groupName(group.key, group.display_name)}</strong>
                          <small>{t('accountFields.selectedCount', {
                            count: selectedCount,
                            total: availableGroupFields.length,
                          })}</small>
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
                          ? t('accountFields.clearGroup')
                          : t('accountFields.selectGroup')}
                      </button>
                    </header>
                    <div
                      className="account-field-picker__rows"
                      hidden={collapsed}
                      id={`${contentId}-${group.key}`}
                    >
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
                                <strong>{fieldName(field)}</strong>
                                <code>{field.key}</code>
                              </span>
                              <small>{fieldDescription(field)}</small>
                              {unsupported ? <small>{unsupportedDetail(field)}</small> : null}
                            </span>
                            <FieldStatus accountSource={accountSource} field={field} />
                          </label>
                        )
                      })}
                      {visibleFields.length === 0 ? (
                        <p className="account-field-picker__empty">{t('accountFields.noMatchingFields')}</p>
                      ) : null}
                      {allGroupFields.length === 0 ? (
                        <p className="account-field-picker__empty">{t('accountFields.emptyGroup')}</p>
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

function FieldStatus({
  accountSource,
  field,
}: {
  accountSource?: AccountSourceKey
  field: AccountFieldCapabilityView
}) {
  const { t } = useTranslation('collection', { i18n })
  const covered = field.covered_by_source_keys?.includes(accountSource ?? '') === true
  if (field.availability === 'enrichment' && !covered) {
    return <span className="account-field-picker__status" data-tone="warning">{t('accountFields.status.enrichment')}</span>
  }
  if (field.availability === 'conditional') {
    return <span className="account-field-picker__status" data-tone="info">{t('accountFields.status.conditional')}</span>
  }
  if (field.availability === 'unsupported') {
    return (
      <span className="account-field-picker__status" data-tone="muted">
        {t('accountFields.status.unsupported', { defaultValue: field.missing_reason || '当前平台不支持' })}
      </span>
    )
  }
  return <span className="account-field-picker__status">{t('accountFields.status.direct')}</span>
}

export default AccountFieldPicker
