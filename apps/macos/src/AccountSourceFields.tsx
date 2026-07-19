import { useEffect, useMemo, useRef, useState } from 'react'
import type { UseFormRegisterReturn } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import AccountFieldPicker from './AccountFieldPicker'
import AppSelect from './AppSelect'
import type { AccountCollectionCapabilityView } from './backend-api'
import { reconcileAccountFields, sourceInputCopy } from './account-source-rules'
import type { AccountSourceKey } from './collection-options'
import { platformSelectOptions } from './collection-select-options'
import { i18n } from './i18n'
import {
  useAccountCapabilities,
  type AccountCapabilityLoader,
} from './use-account-capabilities'
import type { Platform } from './workbench-data'

const platformTranslationKeys: Record<Platform, string> = {
  TikTok: 'accountSources.platform.tiktok',
  抖音: 'accountSources.platform.douyin',
  小红书: 'accountSources.platform.xiaohongshu',
}

type AccountSourceFieldsProps = {
  accountSource?: AccountSourceKey
  capabilityLoader?: AccountCapabilityLoader
  errors?: {
    accountSource?: string
    platform?: string
    sourceInput?: string
  }
  onAccountSourceChange: (source?: AccountSourceKey) => void
  onCapabilityChange?: (capability?: AccountCollectionCapabilityView) => void
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
  onCapabilityChange,
  onPlatformChange,
  onSelectedFieldsChange,
  platform,
  selectedFields,
  sourceInputRegistration,
}: AccountSourceFieldsProps) {
  const { t } = useTranslation('collection', { i18n })
  const { capability, error, isEmpty, isLoading } = useAccountCapabilities(
    platform,
    capabilityLoader,
  )
  const selectedFieldsRef = useRef(selectedFields)
  const accountSourceRef = useRef(accountSource)
  const onAccountSourceChangeRef = useRef(onAccountSourceChange)
  const onCapabilityChangeRef = useRef(onCapabilityChange)
  const onSelectedFieldsChangeRef = useRef(onSelectedFieldsChange)
  const customizedRef = useRef(false)
  const previousPlatformRef = useRef<string | undefined>(undefined)
  const [notice, setNotice] = useState({ removedCount: 0, sourceRemoved: false })
  selectedFieldsRef.current = selectedFields
  accountSourceRef.current = accountSource
  onAccountSourceChangeRef.current = onAccountSourceChange
  onCapabilityChangeRef.current = onCapabilityChange
  onSelectedFieldsChangeRef.current = onSelectedFieldsChange

  useEffect(() => {
    onCapabilityChangeRef.current?.(capability)
  }, [capability])

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
    setNotice({
      removedCount: platformChanged ? reconciled.removedCount : 0,
      sourceRemoved: Boolean(currentSource && !sourceSupported),
    })
  }, [capability])

  const source = capability?.account_sources.find((item) => item.key === accountSource)
  const fallbackInputCopy = sourceInputCopy(source)
  const inputType = !source
    ? 'default'
    : source.input_kind === 'keyword'
      ? 'keyword'
      : source.input_kind === 'item'
        ? 'item'
        : ['followers', 'followings', 'similar_accounts'].includes(source.key)
          ? 'seedAccount'
          : 'account'
  const inputCopy = {
    label: t(`accountSources.input.${inputType}.label`, { defaultValue: fallbackInputCopy.label }),
    placeholder: t(`accountSources.input.${inputType}.placeholder`, {
      defaultValue: fallbackInputCopy.placeholder,
    }),
  }
  const localizedPlatformOptions = useMemo(() => platformSelectOptions.map((item) => ({
    ...item,
    label: t(platformTranslationKeys[item.value as Platform], { defaultValue: item.label }),
  })), [t])
  const sourceOptions = useMemo(() => capability?.account_sources.map((item) => ({
    value: item.key,
    label: t(`accountSources.options.${item.key}.label`, { defaultValue: item.display_name }),
    description: t(`accountSources.options.${item.key}.description`, {
      defaultValue: item.description,
    }),
    meta: item.pagination_mode === 'single'
      ? t('accountSources.singleAccount')
      : t('accountSources.pageSize', { count: item.max_page_size }),
  })) ?? [], [capability, t])

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
          <label htmlFor="platform">{t('fields.platform')}</label>
          <AppSelect
            id="platform"
            ariaDescribedBy={errors?.platform ? 'platform-error' : undefined}
            invalid={Boolean(errors?.platform)}
            onChange={(value) => onPlatformChange(value as Platform)}
            options={localizedPlatformOptions}
            placeholder={t('placeholders.platform')}
            value={platform ?? ''}
          />
          {errors?.platform ? <small className="form-error" id="platform-error">{errors.platform}</small> : null}
        </div>

        <div className="collection-builder__field">
          <label htmlFor="account-source">{t('accountSources.label')}</label>
          <AppSelect
            id="account-source"
            ariaDescribedBy={errors?.accountSource ? 'account-source-error' : undefined}
            disabled={!platform || isLoading || Boolean(error) || isEmpty}
            invalid={Boolean(errors?.accountSource || error)}
            onChange={(value) => onAccountSourceChange(value as AccountSourceKey)}
            options={sourceOptions}
            placeholder={isLoading ? t('accountSources.loading') : t('accountSources.placeholder')}
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

      {notice.removedCount > 0 || notice.sourceRemoved ? (
        <p className="account-source-fields__notice" role="status">
          {[
            notice.removedCount > 0
              ? t('accountSources.fieldsRemoved', { count: notice.removedCount })
              : '',
            notice.sourceRemoved ? t('accountSources.sourceRemoved') : '',
          ].filter(Boolean).join(t('preview.listSeparator'))}
        </p>
      ) : null}

      <AccountFieldPicker
        accountSource={accountSource}
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
