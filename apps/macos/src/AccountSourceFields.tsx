import { useEffect, useMemo, useRef, useState } from 'react'
import type { UseFormRegisterReturn } from 'react-hook-form'
import AccountFieldPicker from './AccountFieldPicker'
import AppSelect from './AppSelect'
import { reconcileAccountFields, sourceInputCopy } from './account-source-rules'
import type { AccountSourceKey } from './collection-options'
import { platformSelectOptions } from './collection-select-options'
import {
  useAccountCapabilities,
  type AccountCapabilityLoader,
} from './use-account-capabilities'
import type { Platform } from './workbench-data'

type AccountSourceFieldsProps = {
  accountSource?: AccountSourceKey
  capabilityLoader?: AccountCapabilityLoader
  errors?: {
    accountSource?: string
    platform?: string
    sourceInput?: string
  }
  onAccountSourceChange: (source?: AccountSourceKey) => void
  onPlatformChange: (platform?: Platform) => void
  onSelectedFieldsChange: (fields: string[]) => void
  platform?: Platform
  selectedFields: string[]
  sourceInputRegistration: UseFormRegisterReturn
}

function AccountSourceFields({
  accountSource,
  capabilityLoader,
  errors,
  onAccountSourceChange,
  onPlatformChange,
  onSelectedFieldsChange,
  platform,
  selectedFields,
  sourceInputRegistration,
}: AccountSourceFieldsProps) {
  const { capability, error, isEmpty, isLoading } = useAccountCapabilities(
    platform,
    capabilityLoader,
  )
  const selectedFieldsRef = useRef(selectedFields)
  const accountSourceRef = useRef(accountSource)
  const onAccountSourceChangeRef = useRef(onAccountSourceChange)
  const onSelectedFieldsChangeRef = useRef(onSelectedFieldsChange)
  const customizedRef = useRef(false)
  const previousPlatformRef = useRef<string | undefined>(undefined)
  const [notice, setNotice] = useState('')
  selectedFieldsRef.current = selectedFields
  accountSourceRef.current = accountSource
  onAccountSourceChangeRef.current = onAccountSourceChange
  onSelectedFieldsChangeRef.current = onSelectedFieldsChange

  useEffect(() => {
    if (!capability) return
    const platformChanged = previousPlatformRef.current !== undefined
      && previousPlatformRef.current !== capability.platform
    const reconciled = reconcileAccountFields(
      capability,
      selectedFieldsRef.current,
      customizedRef.current && platformChanged,
    )
    onSelectedFieldsChangeRef.current(reconciled.fields)
    previousPlatformRef.current = capability.platform

    const currentSource = accountSourceRef.current
    const sourceSupported = capability.account_sources.some((source) => source.key === currentSource)
    if (currentSource && !sourceSupported) onAccountSourceChangeRef.current(undefined)
    const notices = []
    if (reconciled.removedCount > 0 && platformChanged) {
      notices.push(`已移除 ${reconciled.removedCount} 个当前平台不支持的字段`)
    }
    if (currentSource && !sourceSupported) notices.push('原账号来源在当前平台不可用，请重新选择')
    setNotice(notices.join('；'))
  }, [capability])

  const source = capability?.account_sources.find((item) => item.key === accountSource)
  const inputCopy = sourceInputCopy(source)
  const sourceOptions = useMemo(() => capability?.account_sources.map((item) => ({
    value: item.key,
    label: item.display_name,
    description: item.description,
    meta: item.pagination_mode === 'single' ? '单个账号' : `每页最多 ${item.max_page_size}`,
  })) ?? [], [capability])

  const handleFieldsChange = (fields: string[]) => {
    if (capability) {
      const defaults = reconcileAccountFields(capability, [], false).fields
      customizedRef.current = fields.length !== defaults.length
        || defaults.some((field) => !fields.includes(field))
    }
    onSelectedFieldsChange(fields)
  }

  return (
    <div className="account-source-fields">
      <div className="account-source-fields__selectors">
        <div className="collection-builder__field">
          <label htmlFor="platform">平台</label>
          <AppSelect
            id="platform"
            ariaDescribedBy={errors?.platform ? 'platform-error' : undefined}
            invalid={Boolean(errors?.platform)}
            onChange={(value) => onPlatformChange(value as Platform)}
            options={platformSelectOptions}
            placeholder="请选择平台"
            value={platform ?? ''}
          />
          {errors?.platform ? <small className="form-error" id="platform-error">{errors.platform}</small> : null}
        </div>

        <div className="collection-builder__field">
          <label htmlFor="account-source">账号来源</label>
          <AppSelect
            id="account-source"
            ariaDescribedBy={errors?.accountSource ? 'account-source-error' : undefined}
            disabled={!platform || isLoading || Boolean(error) || isEmpty}
            invalid={Boolean(errors?.accountSource || error)}
            onChange={(value) => onAccountSourceChange(value as AccountSourceKey)}
            options={sourceOptions}
            placeholder={isLoading ? '正在读取来源能力' : '请选择账号来源'}
            value={accountSource ?? ''}
          />
          {errors?.accountSource ? (
            <small className="form-error" id="account-source-error">{errors.accountSource}</small>
          ) : null}
        </div>
      </div>

      {source ? (
        <div className="collection-builder__field account-source-fields__input">
          <label htmlFor="source-input">{inputCopy.label}</label>
          <input
            id="source-input"
            aria-describedby={errors?.sourceInput ? 'source-input-error' : undefined}
            aria-invalid={Boolean(errors?.sourceInput)}
            placeholder={inputCopy.placeholder}
            {...sourceInputRegistration}
          />
          {errors?.sourceInput ? (
            <small className="form-error" id="source-input-error">{errors.sourceInput}</small>
          ) : null}
        </div>
      ) : null}

      {notice ? <p className="account-source-fields__notice" role="status">{notice}</p> : null}

      <AccountFieldPicker
        capability={capability}
        error={error}
        isLoading={isLoading}
        onChange={handleFieldsChange}
        selectedFields={selectedFields}
      />
    </div>
  )
}

export default AccountSourceFields
