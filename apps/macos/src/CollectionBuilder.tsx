import { useEffect, useId, useMemo, useRef, useState } from 'react'
import * as Tabs from '@radix-ui/react-tabs'
import { zodResolver } from '@hookform/resolvers/zod'
import {
  AlertTriangle,
  CheckCircle2,
  Gauge,
  MessageSquareText,
  RefreshCcw,
  Sparkles,
  Wrench,
} from 'lucide-react'
import { Controller, useController, useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import type { z } from 'zod'
import AccountSourceFields from './AccountSourceFields'
import AppSelect from './AppSelect'
import type { AccountCollectionCapabilityView } from './backend-api'
import { accountSourceFilterCapabilities } from './account-source-rules'
import './CollectionBuilder.css'
import CollectionFilterFields from './CollectionFilterFields'
import NaturalParseFeedback from './NaturalParseFeedback'
import {
  collectionFormSchema,
  countryRegionOptions,
  createCollectionFormSchema,
  getGenderFilterOptions,
  type AccountSourceKey,
  type CollectionDataType,
} from './collection-options'
import {
  naturalIntentDefault,
  newCollectionFormDefaults,
  normalizeNaturalIntent,
} from './collection-form-defaults'
import { PlanFact, StatusPill } from './CollectionBuilderPrimitives'
import {
  countryRegionSelectOptions,
} from './collection-select-options'
import { useAccountCapabilities } from './use-account-capabilities'
import { i18n } from './i18n'
import {
  confirmationBlocker,
  formatNumber,
  localizedCostEstimate,
  localizedPlanStatus,
  localizeActionMessage,
  localizePlanMessage,
  localizePricingMessage,
  toneForPlanStatus,
} from './collection-plan-localization'
import type { RuntimeCollectionPlan } from './use-workbench-backend'
import type { NaturalParseState } from './natural-parse-state'
import type { DataType, Platform } from './workbench-data'

export { StatusPill }

export type CollectionFormInput = z.input<typeof collectionFormSchema>
export type CollectionFormValues = z.output<typeof collectionFormSchema>

const emptySelectedFields: string[] = []

function backendPlatformForUi(platform: Platform) {
  if (platform === 'TikTok') return 'tiktok'
  if (platform === '抖音') return 'douyin'
  return 'xiaohongshu'
}

const legacySourceTypes: Record<AccountSourceKey, {
  dataType: DataType
  dataTypeCode: CollectionDataType
}> = {
  user_search: { dataType: '搜索结果账号', dataTypeCode: 'keyword_search' },
  content_search_authors: { dataType: '搜索结果账号', dataTypeCode: 'keyword_search' },
  direct_account: { dataType: '账号公开信息', dataTypeCode: 'account_profile' },
  item_author: { dataType: '作品/笔记作者', dataTypeCode: 'item_detail' },
  comment_authors: { dataType: '评论用户', dataTypeCode: 'comments' },
  followers: { dataType: '账号公开信息', dataTypeCode: 'account_profile' },
  followings: { dataType: '账号公开信息', dataTypeCode: 'account_profile' },
  similar_accounts: { dataType: '账号公开信息', dataTypeCode: 'account_profile' },
}

export function CollectionBuilder({
  actionMessage,
  activePlan,
  isBusy,
  onConfirmPlan,
  onGenerateFormPlan,
  onGenerateNaturalPlan,
  onRetryNaturalPlan,
  naturalParseState,
  onOpenAiSettings,
  onViewParseDiagnostics,
}: {
  actionMessage: string
  activePlan?: RuntimeCollectionPlan
  isBusy: boolean
  onConfirmPlan: () => Promise<unknown>
  onGenerateFormPlan: (values: CollectionFormValues) => Promise<RuntimeCollectionPlan>
  onGenerateNaturalPlan: (intentText: string) => Promise<RuntimeCollectionPlan>
  onRetryNaturalPlan: (taskId: string, intentText: string) => Promise<unknown>
  naturalParseState?: NaturalParseState
  onOpenAiSettings?: () => void
  onViewParseDiagnostics?: () => void
}) {
  const { t } = useTranslation('collection', { i18n })
  const [naturalText, setNaturalText] = useState(naturalIntentDefault)
  const [activeMode, setActiveMode] = useState('form')
  const [accountCapability, setAccountCapability] = useState<AccountCollectionCapabilityView>()
  const planSubmissionInFlightRef = useRef(false)
  const restoredNaturalAttemptRef = useRef<string | undefined>(undefined)
  const formSchema = useMemo(() => createCollectionFormSchema(t), [t])
  const {
    control,
    register,
    handleSubmit,
    setValue,
    watch,
    formState: { errors },
  } = useForm<CollectionFormInput, unknown, CollectionFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: newCollectionFormDefaults,
  })
  const { field: platformField } = useController({ control, name: 'platform' })
  const { field: accountSourceField } = useController({ control, name: 'accountSource' })
  const { field: selectedFieldsField } = useController({ control, name: 'selectedFields' })
  const { field: dataTypeField } = useController({ control, name: 'dataType' })
  const { field: dataTypesField } = useController({ control, name: 'dataTypes' })
  const selectedPlatform = platformField.value
  const selectedFields = selectedFieldsField.value ?? emptySelectedFields
  const selectedRange = watch('range')
  const ageRangeEnabled = watch('ageRangeEnabled')
  const genderFilterEnabled = watch('genderFilterEnabled')
  const selectedBackendPlatform = selectedPlatform === 'TikTok'
    ? 'tiktok'
    : selectedPlatform === '抖音'
      ? 'douyin'
      : selectedPlatform === '小红书'
        ? 'xiaohongshu'
        : undefined
  const capabilityReady = Boolean(
    selectedBackendPlatform && accountCapability?.platform === selectedBackendPlatform,
  )
  const capabilitySubmittable = Boolean(
    capabilityReady
    && accountCapability
    && accountCapability.account_sources.length > 0
    && accountCapability.fields.length > 0,
  )
  const formCapabilityMatchesPlan = Boolean(
    activePlan && accountCapability?.platform === backendPlatformForUi(activePlan.platform),
  )
  const { capability: loadedPlanCapability } = useAccountCapabilities(
    activePlan && !formCapabilityMatchesPlan ? activePlan.platform : undefined,
  )
  const previewCapability = formCapabilityMatchesPlan
    ? accountCapability
    : loadedPlanCapability?.platform === (activePlan && backendPlatformForUi(activePlan.platform))
      ? loadedPlanCapability
      : undefined
  const ageFilterSupported = capabilityReady && accountCapability?.fields.some(
    (field) => field.key === 'age' && field.availability !== 'unsupported',
  ) === true
  const genderFilterSupported = capabilityReady && accountCapability?.fields.some(
    (field) => field.key === 'gender' && field.availability !== 'unsupported',
  ) === true
  const sourceFilters = useMemo(
    () => accountSourceFilterCapabilities(accountCapability, accountSourceField.value),
    [accountCapability, accountSourceField.value],
  )
  const timeRangeOptions = useMemo(() => sourceFilters.timeRanges.map((value) => ({
    value,
    label: t('options.timeRange.days', { count: Number(value) }),
    meta: `${value}d`,
  })), [sourceFilters.timeRanges, t])
  const regionEnabled = capabilityReady && sourceFilters.regionFilter !== 'unsupported'
  const timeRangeEnabled = capabilityReady
    && sourceFilters.timeRangeFilter !== 'unsupported'
    && sourceFilters.timeRanges.length > 0

  useEffect(() => {
    if (!regionEnabled) setValue('regionCode', '')
  }, [regionEnabled, setValue])

  useEffect(() => {
    if (!ageFilterSupported && ageRangeEnabled) setValue('ageRangeEnabled', false)
    if (!genderFilterSupported && genderFilterEnabled) setValue('genderFilterEnabled', false)
  }, [
    ageFilterSupported,
    ageRangeEnabled,
    genderFilterEnabled,
    genderFilterSupported,
    setValue,
  ])

  useEffect(() => {
    const requiredFields = [
      genderFilterEnabled && genderFilterSupported ? 'gender' : undefined,
      ageRangeEnabled && ageFilterSupported ? 'age' : undefined,
    ].filter((field): field is string => Boolean(field))
    const missingFields = requiredFields.filter((field) => !selectedFields.includes(field))
    if (missingFields.length === 0) return
    setValue('selectedFields', [...selectedFields, ...missingFields], {
      shouldDirty: true,
      shouldValidate: true,
    })
  }, [
    ageFilterSupported,
    ageRangeEnabled,
    genderFilterEnabled,
    genderFilterSupported,
    selectedFields,
    setValue,
  ])

  useEffect(() => {
    if (selectedRange && !sourceFilters.timeRanges.includes(selectedRange)) {
      setValue('range', '', { shouldValidate: true })
    }
  }, [selectedRange, setValue, sourceFilters.timeRanges])

  useEffect(() => {
    const state = naturalParseState
    if (!state) return
    const persistedText = state.intentText.trim()
    if (!persistedText) return
    const attemptKey = state.attemptId
      ?? `${state.taskId ?? 'unknown'}:${state.finishedAt ?? ''}`
    if (restoredNaturalAttemptRef.current === attemptKey) return
    restoredNaturalAttemptRef.current = attemptKey
    setNaturalText((current) => normalizeNaturalIntent(current) ? current : persistedText)
  }, [naturalParseState])

  const submitPlanOnce = async (submission: () => Promise<unknown>) => {
    if (planSubmissionInFlightRef.current) return
    planSubmissionInFlightRef.current = true
    try {
      await submission()
    } finally {
      planSubmissionInFlightRef.current = false
    }
  }

  const submitForm = async (values: CollectionFormValues) => {
    if (!capabilitySubmittable) return
    await submitPlanOnce(() => onGenerateFormPlan(values))
  }

  const setAccountSource = (source?: AccountSourceKey) => {
    accountSourceField.onChange(source)
    setValue('keyword', '', { shouldDirty: true, shouldValidate: true })
    const legacy = source ? legacySourceTypes[source] : undefined
    dataTypeField.onChange(legacy?.dataType)
    dataTypesField.onChange(legacy ? [legacy.dataTypeCode] : [])
  }

  const submitNaturalText = async () => {
    const intentText = normalizeNaturalIntent(naturalText)
      || normalizeNaturalIntent(naturalParseState?.intentText ?? '')
    if (!intentText) return
    const taskId = naturalParseState?.taskId
    await submitPlanOnce(() => taskId
      ? onRetryNaturalPlan(taskId, intentText)
      : onGenerateNaturalPlan(intentText))
  }

  return (
    <section className="collection-builder" aria-labelledby="collection-builder-heading">
      <header className="collection-builder__heading">
        <div>
          <p className="eyebrow">{t('header.eyebrow')}</p>
          <h2 id="collection-builder-heading">{t('header.title')}</h2>
          <p>{t('header.description')}</p>
        </div>
        <StatusPill tone="warning" label={t('header.noChargeBeforeConfirmation')} />
      </header>

      <Tabs.Root
        className="collection-builder__tabs"
        value={activeMode}
        onValueChange={setActiveMode}
      >
        <div className="collection-builder__mode-bar">
          <Tabs.List className="collection-builder__mode-list" aria-label={t('modes.ariaLabel')}>
            <Tabs.Trigger className="collection-builder__mode-trigger" value="form">
              <Wrench size={15} aria-hidden="true" />
              {t('modes.form')}
            </Tabs.Trigger>
            <Tabs.Trigger className="collection-builder__mode-trigger" value="natural">
              <MessageSquareText size={15} aria-hidden="true" />
              {t('modes.naturalLanguage')}
            </Tabs.Trigger>
          </Tabs.List>
          <p>{t('modes.description')}</p>
        </div>

        <Tabs.Content className="collection-builder__content" value="form">
          <form className="collection-builder__form" onSubmit={handleSubmit(submitForm)}>
            <FormGroup
              number="01"
              title={t('groups.source.title')}
              description={t('groups.source.description')}
            >
              <AccountSourceFields
                accountSource={accountSourceField.value}
                errors={{
                  accountSource: errors.accountSource?.message,
                  platform: errors.platform?.message,
                  sourceInput: errors.keyword?.message,
                }}
                onAccountSourceChange={setAccountSource}
                onCapabilityChange={setAccountCapability}
                onPlatformChange={platformField.onChange}
                onSelectedFieldsChange={selectedFieldsField.onChange}
                platform={selectedPlatform}
                selectedFields={selectedFields}
                sourceInputRegistration={register('keyword')}
              />
            </FormGroup>

            <FormGroup
              number="02"
              title={t('groups.scope.title')}
              description={t('groups.scope.description')}
            >
              <div className="collection-builder__range-fields">
                <FormField
                  error={errors.regionCode?.message}
                  errorId="region-code-error"
                  htmlFor="region-code"
                  label={t('fields.region')}
                  hint={t(`fields.filter.${sourceFilters.regionFilter}`)}
                >
                  <Controller
                    control={control}
                    name="regionCode"
                    render={({ field }) => (
                      <AppSelect
                        id="region-code"
                        ariaDescribedBy={errors.regionCode ? 'region-code-error' : undefined}
                        disabled={!regionEnabled}
                        emptyLabel={t('placeholders.regionEmpty')}
                        invalid={Boolean(errors.regionCode)}
                        onChange={field.onChange}
                        options={countryRegionSelectOptions}
                        placeholder={regionEnabled ? t('placeholders.region') : t('placeholders.regionUnavailable')}
                        searchable
                        searchPlaceholder={t('placeholders.regionSearch')}
                        value={field.value ?? ''}
                      />
                    )}
                  />
                </FormField>
                <FormField
                  error={errors.range?.message}
                  errorId="range-error"
                  htmlFor="range"
                  label={t('fields.range')}
                  hint={t(`fields.filter.${sourceFilters.timeRangeFilter}`)}
                >
                  <Controller
                    control={control}
                    name="range"
                    render={({ field }) => (
                      <AppSelect
                        id="range"
                        ariaDescribedBy={errors.range ? 'range-error' : undefined}
                        disabled={!timeRangeEnabled}
                        invalid={Boolean(errors.range)}
                        onChange={field.onChange}
                        options={timeRangeOptions}
                        placeholder={timeRangeEnabled
                          ? t('placeholders.range')
                          : t('placeholders.rangeUnavailable')}
                        value={field.value ?? ''}
                      />
                    )}
                  />
                </FormField>
              </div>
            </FormGroup>

            <FormGroup
              number="03"
              title={t('groups.volume.title')}
              description={t('groups.volume.description')}
            >
              <div className="collection-builder__limit-fields">
                <FormField
                  error={errors.maxRecords?.message}
                  errorId="max-records-error"
                  htmlFor="max-records"
                  label={t('fields.maxRecords')}
                  suffix={t('fields.recordsSuffix')}
                >
                  <input
                    id="max-records"
                    aria-describedby={errors.maxRecords ? 'max-records-error' : undefined}
                    aria-invalid={Boolean(errors.maxRecords)}
                    min="1"
                    placeholder={t('placeholders.maxRecords')}
                    type="number"
                    {...register('maxRecords', { valueAsNumber: true })}
                  />
                </FormField>
                <FormField
                  error={errors.budget?.message}
                  errorId="budget-error"
                  htmlFor="budget"
                  label={t('fields.budget')}
                  suffix={t('fields.budgetCurrency')}
                >
                  <input
                    id="budget"
                    aria-describedby={errors.budget ? 'budget-error' : undefined}
                    aria-invalid={Boolean(errors.budget)}
                    min="0.1"
                    placeholder={t('placeholders.budget')}
                    step="0.01"
                    type="number"
                    {...register('budget', { valueAsNumber: true })}
                  />
                </FormField>
              </div>
            </FormGroup>

            <FormGroup
              number="04"
              title={t('groups.filters.title')}
              description={t('groups.filters.description')}
            >
              <CollectionFilterFields
                ageFilterSupported={ageFilterSupported}
                ageRangeEnabled={ageRangeEnabled}
                capabilityReady={capabilityReady}
                errors={errors}
                genderFilterEnabled={genderFilterEnabled}
                genderFilterSupported={genderFilterSupported}
                platformSelected={Boolean(selectedPlatform)}
                register={register}
              />
            </FormGroup>

            <div className="collection-builder__form-footer">
              <p>{t('form.footer')}</p>
              <button
                className="primary-button"
                disabled={isBusy || Boolean(selectedPlatform && !capabilitySubmittable)}
                type="submit"
              >
                <Gauge size={16} aria-hidden="true" />
                {isBusy ? t('form.generating') : t('form.generatePlan')}
              </button>
            </div>
          </form>
        </Tabs.Content>

        <Tabs.Content className="collection-builder__content" value="natural">
          <div className="collection-builder__natural">
            <div>
              <p className="eyebrow">{t('natural.eyebrow')}</p>
              <h3>{t('natural.title')}</h3>
              <p>{t('natural.description')}</p>
            </div>
            <label htmlFor="intent">{t('fields.naturalIntent')}</label>
            <textarea
              id="intent"
              value={naturalText}
              placeholder={t('placeholders.naturalIntent')}
              onChange={(event) => setNaturalText(event.target.value)}
            />
            {naturalParseState && (
              <NaturalParseFeedback
                state={naturalParseState}
                onRetry={submitNaturalText}
                onOpenAiSettings={onOpenAiSettings}
                onSwitchToForm={() => setActiveMode('form')}
                onViewDiagnostics={onViewParseDiagnostics}
              />
            )}
            <div className="collection-builder__natural-footer">
              <p>{t('natural.footer')}</p>
              <button
                className="primary-button"
                disabled={isBusy || !normalizeNaturalIntent(naturalText)}
                type="button"
                onClick={submitNaturalText}
              >
                {activePlan
                  ? <RefreshCcw size={16} aria-hidden="true" />
                  : <Sparkles size={16} aria-hidden="true" />}
                {activePlan ? t('natural.reparse') : t('natural.parse')}
              </button>
            </div>
          </div>
        </Tabs.Content>
      </Tabs.Root>

      {activePlan ? (
        <CollectionPlanPreview
          accountCapability={previewCapability}
          actionMessage={actionMessage}
          isBusy={isBusy}
          onConfirmPlan={onConfirmPlan}
          plan={activePlan}
        />
      ) : (
        <div className="collection-plan-empty" role="status">
          <div>
            <p className="eyebrow">{t('empty.eyebrow')}</p>
            <h3>{t('empty.title')}</h3>
            <p>{t('empty.description')}</p>
          </div>
          <StatusPill tone="info" label={t('empty.status')} />
        </div>
      )}
    </section>
  )
}

function FormGroup({
  number,
  title,
  description,
  children,
}: {
  number: string
  title: string
  description: string
  children: React.ReactNode
}) {
  return (
    <section className="collection-builder__group">
      <header>
        <h3>{number} {title}</h3>
        <p>{description}</p>
      </header>
      <div className="collection-builder__group-body">{children}</div>
    </section>
  )
}

function FormField({
  htmlFor,
  label,
  hint,
  suffix,
  error,
  errorId,
  children,
}: {
  htmlFor: string
  label: string
  hint?: string
  suffix?: string
  error?: string
  errorId: string
  children: React.ReactNode
}) {
  return (
    <div className="collection-builder__field">
      <label htmlFor={htmlFor}>
        <span>{label}</span>
        {suffix ? <small>{suffix}</small> : null}
      </label>
      {children}
      {hint ? <p>{hint}</p> : null}
      <FormError id={errorId} message={error} />
    </div>
  )
}

function FormError({ id, message }: { id: string; message?: string }) {
  return message ? <p className="collection-builder__error" id={id}>{message}</p> : null
}

export function CollectionPlanPreview({
  accountCapability,
  actionMessage,
  isBusy,
  onConfirmPlan,
  plan,
}: {
  accountCapability?: AccountCollectionCapabilityView
  actionMessage: string
  isBusy: boolean
  onConfirmPlan: () => Promise<unknown>
  plan: RuntimeCollectionPlan
}) {
  const { t } = useTranslation('collection', { i18n })
  const blockerId = useId()
  const localizedGenderFilterOptions = useMemo(() => getGenderFilterOptions(t), [t])
  const blocker = confirmationBlocker(plan, isBusy, t)
  const pricingNotice = plan.pricingReady !== true && plan.pricingBlocker
    ? localizePricingMessage(t, plan.pricingBlocker)
    : undefined
  const isEnqueued = plan.status === '已排队' || plan.status === '运行中'
  const canConfirm = plan.status === '等待确认' && !blocker
  const showConfirmButton = plan.status === '等待确认' || plan.status === '待人工确认'
  const region = countryRegionOptions.find(({ code }) => code === plan.regionCode)
  const regionLabel = region
    ? t('preview.countryRegion', {
      code: region.code,
      nameEn: region.nameEn,
      nameZh: region.nameZh,
    })
    : undefined
  const ageLabel = plan.ageRangeEnabled && plan.ageMin !== undefined && plan.ageMax !== undefined
    ? t('preview.ageRangeEnabled', { max: plan.ageMax, min: plan.ageMin })
    : t('preview.filterDisabled')
  const genderLabel = plan.genderFilterEnabled && plan.genders?.length
    ? plan.genders.map((gender) => (
      localizedGenderFilterOptions.find(({ value }) => value === gender)?.label ?? gender
    )).join(t('preview.listSeparator'))
    : t('preview.filterDisabled')
  const platforms = (plan.platforms?.length ? plan.platforms : [plan.platform]).join(t('preview.listSeparator'))
  const dataTypes = (plan.dataTypes?.length ? plan.dataTypes : [plan.dataType]).join(t('preview.listSeparator'))
  const range = plan.maxRecords > 0
    ? t('preview.rangeWithLimit', {
      maxRecords: formatNumber(plan.maxRecords, i18n.language),
      range: plan.range,
    })
    : plan.range
  const budget = plan.budget > 0 ? `$${plan.budget}` : t('preview.budgetLimitUnset')
  const costEstimate = localizedCostEstimate(t, plan.costEstimate, i18n.language)
  const statusLabel = localizedPlanStatus(t, plan.status)
  const accountSourceLabel = plan.accountSource
    ? t(`accountSources.options.${plan.accountSource}.label`)
    : undefined
  const selectedFieldCount = plan.selectedFields?.length ?? 0
  const selectedFieldSet = new Set(plan.selectedFields ?? [])
  const matchingAccountCapability = accountCapability?.platform === backendPlatformForUi(plan.platform)
    ? accountCapability
    : undefined
  const sourceAwareEnrichmentGroups = matchingAccountCapability && plan.accountSource
    ? new Set(matchingAccountCapability.fields
      .filter((field) => selectedFieldSet.has(field.key)
        && field.required_operation_keys.length > 0
        && !field.covered_by_source_keys?.includes(plan.accountSource ?? ''))
      .map((field) => field.group))
    : undefined
  const enrichmentGroups = sourceAwareEnrichmentGroups
    ? (matchingAccountCapability?.field_groups ?? [])
      .filter((group) => sourceAwareEnrichmentGroups.has(group.key))
      .map((group) => t(`accountFieldGroups.${group.key}`, { defaultValue: group.display_name }))
    : []

  return (
    <section className="collection-plan" aria-labelledby="collection-plan-heading">
      <header className="collection-plan__header">
        <div>
          <p className="eyebrow">{t('preview.eyebrow')}</p>
          <h3 id="collection-plan-heading">{plan.keyword || t('preview.pendingTarget')}</h3>
          <p>{platforms} · {dataTypes}</p>
        </div>
        <StatusPill tone={toneForPlanStatus(plan.status)} label={statusLabel} />
      </header>

      <div className="collection-plan__body">
        <dl className="collection-plan__facts">
          <PlanFact label={t('preview.platform')} value={platforms} />
          <PlanFact label={t('preview.dataType')} value={dataTypes} />
          {accountSourceLabel ? (
            <PlanFact label={t('preview.accountSource')} value={accountSourceLabel} />
          ) : null}
          {plan.accountSource ? (
            <PlanFact
              label={t('preview.selectedFields')}
              value={t('preview.selectedFieldCount', { count: selectedFieldCount })}
            />
          ) : null}
          {plan.queryLocale ? (
            <PlanFact label={t('preview.queryLocale')} value={plan.queryLocale} />
          ) : null}
          <PlanFact
            label={t('preview.actualQuery')}
            value={plan.keyword || t('preview.pendingTarget')}
          />
          <PlanFact label={t('preview.region')} value={regionLabel ?? t('preview.regionUnavailable')} />
          <PlanFact label={t('preview.range')} value={range} />
        </dl>

        <div className="collection-plan__detail-grid">
          <section className="collection-plan__filters" aria-labelledby="collection-plan-filters-heading">
            <h4 id="collection-plan-filters-heading">{t('preview.filters')}</h4>
            <dl>
              <PlanFact label={t('preview.age')} value={ageLabel} />
              <PlanFact label={t('preview.gender')} value={genderLabel} />
            </dl>
          </section>
          <section
            className="collection-plan__pricing"
            data-tone="neutral"
            data-ready={plan.pricingReady === true}
            aria-labelledby="collection-plan-pricing-heading"
          >
            <div>
              <h4 id="collection-plan-pricing-heading">{t('preview.pricing')}</h4>
              <p>{plan.pricingReady === true ? t('preview.pricingReady') : t('preview.pricingPending')}</p>
            </div>
            <strong>{costEstimate}</strong>
            {plan.discoveryRequestCount !== undefined ? (
              <span>{t('preview.discoveryRequests', { count: plan.discoveryRequestCount })}</span>
            ) : null}
            {plan.enrichmentRequestCount !== undefined ? (
              <span>{t('preview.enrichmentRequests', { count: plan.enrichmentRequestCount })}</span>
            ) : null}
            <span>{t('preview.budgetLimit', { budget })}</span>
            {enrichmentGroups.length ? (
              <span>{t('preview.enrichmentGroups', {
                groups: enrichmentGroups.join(t('preview.listSeparator')),
              })}</span>
            ) : null}
            {plan.pricingEndpoints?.length ? (
              <span>{t('preview.pricingEndpoints', {
                endpoints: plan.pricingEndpoints.join(t('preview.listSeparator')),
              })}</span>
            ) : null}
          </section>
        </div>

        {plan.missing.length ? (
          <div className="collection-plan__missing">
            <AlertTriangle size={16} aria-hidden="true" />
            <div>
              <strong>{t('preview.missingTitle')}</strong>
              <p>{plan.missing.map((message) => localizePlanMessage(t, message)).join(t('preview.listSeparator'))}</p>
            </div>
          </div>
        ) : null}

        {blocker && !isEnqueued ? (
          <p className="collection-plan__blocker" id={blockerId} role="status">
            <AlertTriangle size={15} aria-hidden="true" />
            {plan.taskId ? t('preview.blockerPrefix', { blocker }) : blocker}
          </p>
        ) : pricingNotice && !isEnqueued ? (
          <p className="collection-plan__blocker" data-blocking="false" id={blockerId} role="status">
            <AlertTriangle size={15} aria-hidden="true" />
            {t('preview.pricingNoticePrefix', { notice: pricingNotice })}
          </p>
        ) : null}
      </div>

      <footer className="collection-plan__footer">
        <p>{isEnqueued ? t('preview.enqueuedFooter') : localizeActionMessage(t, actionMessage)}</p>
        {showConfirmButton ? (
          <button
            aria-describedby={blocker || pricingNotice ? blockerId : undefined}
            className="primary-button"
            disabled={!canConfirm || isBusy}
            type="button"
            onClick={() => void onConfirmPlan()}
          >
            <CheckCircle2 size={16} aria-hidden="true" />
            {plan.taskId ? t('preview.confirmRun') : t('preview.generateFirst')}
          </button>
        ) : (
          <span className="collection-plan__footer-state">{statusLabel}</span>
        )}
      </footer>
    </section>
  )
}
