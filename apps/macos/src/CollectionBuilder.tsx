import { useEffect, useId, useMemo, useState } from 'react'
import * as Tabs from '@radix-ui/react-tabs'
import { zodResolver } from '@hookform/resolvers/zod'
import {
  Activity,
  AlertTriangle,
  BadgeCheck,
  CheckCircle2,
  Gauge,
  MessageSquareText,
  RefreshCcw,
  Sparkles,
  Wrench,
} from 'lucide-react'
import { Controller, useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import type { z } from 'zod'
import AppSelect from './AppSelect'
import './CollectionBuilder.css'
import {
  AGE_RANGE_LIMITS,
  collectionFormSchema,
  countryRegionOptions,
  createCollectionFormSchema,
  getCollectionDataTypeOptions,
  getGenderFilterOptions,
  supportsRegionSelection,
  type CollectionTranslator,
  type CollectionDataType,
} from './collection-options'
import {
  naturalIntentDefault,
  newCollectionFormDefaults,
  normalizeNaturalIntent,
} from './collection-form-defaults'
import {
  countryRegionSelectOptions,
  platformSelectOptions,
} from './collection-select-options'
import { i18n } from './i18n'
import type { RuntimeCollectionPlan } from './use-workbench-backend'
import type { DataType } from './workbench-data'

export type CollectionFormInput = z.input<typeof collectionFormSchema>
export type CollectionFormValues = z.output<typeof collectionFormSchema>

const dataTypeLabels: Record<CollectionDataType, DataType> = {
  keyword_search: '搜索结果账号',
  item_detail: '作品/笔记作者',
  account_profile: '账号公开信息',
  account_posts: '账号作品所属账号',
  comments: '评论用户',
}

export function CollectionBuilder({
  actionMessage,
  activePlan,
  isBusy,
  onConfirmPlan,
  onGenerateFormPlan,
  onGenerateNaturalPlan,
}: {
  actionMessage: string
  activePlan?: RuntimeCollectionPlan
  isBusy: boolean
  onConfirmPlan: () => Promise<unknown>
  onGenerateFormPlan: (values: CollectionFormValues) => Promise<RuntimeCollectionPlan>
  onGenerateNaturalPlan: (intentText: string) => Promise<RuntimeCollectionPlan>
}) {
  const { t } = useTranslation('collection', { i18n })
  const [naturalText, setNaturalText] = useState(naturalIntentDefault)
  const formSchema = useMemo(() => createCollectionFormSchema(t), [t])
  const localizedDataTypeOptions = useMemo(() => getCollectionDataTypeOptions(t), [t])
  const localizedGenderFilterOptions = useMemo(() => getGenderFilterOptions(t), [t])
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
  const selectedPlatform = watch('platform')
  const selectedDataTypes = watch('dataTypes') ?? []
  const primaryDataType = selectedDataTypes[0]
  const ageRangeEnabled = watch('ageRangeEnabled')
  const genderFilterEnabled = watch('genderFilterEnabled')
  const regionEnabled = selectedPlatform
    ? supportsRegionSelection(selectedPlatform, selectedDataTypes)
    : false

  useEffect(() => {
    if (primaryDataType) setValue('dataType', dataTypeLabels[primaryDataType])
  }, [primaryDataType, setValue])

  useEffect(() => {
    if (!regionEnabled) setValue('regionCode', '')
  }, [regionEnabled, setValue])

  const submitForm = async (values: CollectionFormValues) => {
    await onGenerateFormPlan(values)
  }

  const submitNaturalText = async () => {
    const intentText = normalizeNaturalIntent(naturalText)
    if (!intentText) return
    await onGenerateNaturalPlan(intentText)
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

      <Tabs.Root className="collection-builder__tabs" defaultValue="form">
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
              <div className="collection-builder__source-fields">
                <FormField
                  error={errors.platform?.message}
                  errorId="platform-error"
                  htmlFor="platform"
                  label={t('fields.platform')}
                >
                  <Controller
                    control={control}
                    name="platform"
                    render={({ field }) => (
                      <AppSelect
                        id="platform"
                        ariaDescribedBy={errors.platform ? 'platform-error' : undefined}
                        invalid={Boolean(errors.platform)}
                        onChange={field.onChange}
                        options={platformSelectOptions}
                        placeholder={t('placeholders.platform')}
                        value={field.value ?? ''}
                      />
                    )}
                  />
                </FormField>

                <fieldset className="collection-builder__choice-fieldset">
                  <legend>{t('fields.dataTypes')}</legend>
                  <input type="hidden" {...register('dataType')} />
                  <div className="collection-builder__option-list">
                    {localizedDataTypeOptions.map((item) => (
                      <label
                        className="collection-builder__option-row"
                        data-selected={selectedDataTypes.includes(item.value)}
                        key={item.value}
                      >
                        <input type="checkbox" value={item.value} {...register('dataTypes')} />
                        <span>
                          <strong>{item.label}</strong>
                          <small>{item.description}</small>
                        </span>
                      </label>
                    ))}
                  </div>
                  <FormError id="data-types-error" message={errors.dataTypes?.message} />
                </fieldset>
              </div>
            </FormGroup>

            <FormGroup
              number="02"
              title={t('groups.scope.title')}
              description={t('groups.scope.description')}
            >
              <div className="collection-builder__range-fields">
                <FormField
                  error={errors.keyword?.message}
                  errorId="keyword-error"
                  htmlFor="keyword"
                  label={t('fields.keyword')}
                >
                  <input
                    id="keyword"
                    aria-describedby={errors.keyword ? 'keyword-error' : undefined}
                    aria-invalid={Boolean(errors.keyword)}
                    placeholder={t('placeholders.keyword')}
                    {...register('keyword')}
                  />
                </FormField>
                <FormField
                  error={errors.regionCode?.message}
                  errorId="region-code-error"
                  htmlFor="region-code"
                  label={t('fields.region')}
                  hint={regionEnabled ? t('fields.regionHintSupported') : t('fields.regionHintUnsupported')}
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
                >
                  <input
                    id="range"
                    aria-describedby={errors.range ? 'range-error' : undefined}
                    aria-invalid={Boolean(errors.range)}
                    placeholder={t('placeholders.range')}
                    {...register('range')}
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
                    min="0"
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
              <div className="collection-builder__filter-grid">
                <fieldset className="collection-builder__filter-block" data-enabled={ageRangeEnabled}>
                  <legend className="collection-builder__visually-hidden">{t('fields.ageRange')}</legend>
                  <label className="collection-builder__filter-toggle">
                    <input type="checkbox" {...register('ageRangeEnabled')} />
                    <span>
                      <strong>{t('fields.ageRange')}</strong>
                      <small>{t('fields.ageRangeDescription')}</small>
                    </span>
                  </label>
                  {ageRangeEnabled ? (
                    <div className="collection-builder__age-inputs">
                      <label>
                        <span>{t('fields.minimumAge')}</span>
                        <input
                          aria-describedby={errors.ageMin ? 'age-min-error' : undefined}
                          aria-invalid={Boolean(errors.ageMin)}
                          min={AGE_RANGE_LIMITS.min}
                          max={AGE_RANGE_LIMITS.max}
                          placeholder={t('placeholders.minimumAge')}
                          type="number"
                          {...register('ageMin', { valueAsNumber: true })}
                        />
                      </label>
                      <span aria-hidden="true">{t('fields.ageRangeSeparator')}</span>
                      <label>
                        <span>{t('fields.maximumAge')}</span>
                        <input
                          aria-describedby={errors.ageMax ? 'age-max-error' : undefined}
                          aria-invalid={Boolean(errors.ageMax)}
                          min={AGE_RANGE_LIMITS.min}
                          max={AGE_RANGE_LIMITS.max}
                          placeholder={t('placeholders.maximumAge')}
                          type="number"
                          {...register('ageMax', { valueAsNumber: true })}
                        />
                      </label>
                    </div>
                  ) : null}
                  <FormError id="age-min-error" message={errors.ageMin?.message} />
                  <FormError id="age-max-error" message={errors.ageMax?.message} />
                </fieldset>

                <fieldset className="collection-builder__filter-block" data-enabled={genderFilterEnabled}>
                  <legend className="collection-builder__visually-hidden">{t('fields.gender')}</legend>
                  <label className="collection-builder__filter-toggle">
                    <input type="checkbox" {...register('genderFilterEnabled')} />
                    <span>
                      <strong>{t('fields.gender')}</strong>
                      <small>{t('fields.genderDescription')}</small>
                    </span>
                  </label>
                  {genderFilterEnabled ? (
                    <div className="collection-builder__gender-options">
                      {localizedGenderFilterOptions.map((item) => (
                        <label key={item.value}>
                          <input type="checkbox" value={item.value} {...register('genders')} />
                          <span>{item.label}</span>
                        </label>
                      ))}
                    </div>
                  ) : null}
                  <FormError id="genders-error" message={errors.genders?.message} />
                </fieldset>
              </div>
            </FormGroup>

            <div className="collection-builder__form-footer">
              <p>{t('form.footer')}</p>
              <button className="primary-button" disabled={isBusy} type="submit">
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
  actionMessage,
  isBusy,
  onConfirmPlan,
  plan,
}: {
  actionMessage: string
  isBusy: boolean
  onConfirmPlan: () => Promise<unknown>
  plan: RuntimeCollectionPlan
}) {
  const { t } = useTranslation('collection', { i18n })
  const blockerId = useId()
  const localizedGenderFilterOptions = useMemo(() => getGenderFilterOptions(t), [t])
  const blocker = confirmationBlocker(plan, isBusy, t)
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
            data-ready={plan.pricingReady === true}
            aria-labelledby="collection-plan-pricing-heading"
          >
            <div>
              <h4 id="collection-plan-pricing-heading">{t('preview.pricing')}</h4>
              <p>{plan.pricingReady === true ? t('preview.pricingReady') : t('preview.pricingPending')}</p>
            </div>
            <strong>{costEstimate}</strong>
            <span>{t('preview.budgetLimit', { budget })}</span>
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
        ) : null}
      </div>

      <footer className="collection-plan__footer">
        <p>{isEnqueued ? t('preview.enqueuedFooter') : localizeActionMessage(t, actionMessage)}</p>
        {showConfirmButton ? (
          <button
            aria-describedby={blocker ? blockerId : undefined}
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

function PlanFact({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  )
}

const planStatusTranslationKeys: Record<RuntimeCollectionPlan['status'], string> = {
  '已排队': 'status.queued',
  '运行中': 'status.running',
  '等待确认': 'status.awaitingConfirmation',
  '部分成功': 'status.partialSuccess',
  '成功': 'status.success',
  '待人工确认': 'status.manualConfirmation',
  '失败': 'status.failed',
  '已取消': 'status.cancelled',
}

const planMessageTranslationKeys: Record<string, string> = {
  '年龄范围必须填写上下限': 'message.ageRangeBoundsRequired',
  '价格未知': 'message.priceUnknown',
  'TikHub 免费额度与充值余额合计不足': 'message.tikhubCreditsInsufficient',
  'TikHub 额度合计与免费额度、充值余额不一致': 'message.tikhubCreditsMismatch',
  '未提供时间范围': 'message.rangeMissing',
  '计划校验未通过': 'message.planValidationFailed',
  '计划校验未通过，无法确认运行': 'message.planValidationFailedCannotRun',
}

const actionMessageTranslationKeys: Record<string, string> = {
  '后端正在初始化本地工作区': 'action.initializing',
  '等待生成': 'action.waitingForPlan',
  '等待确认': 'status.awaitingConfirmation',
  '采集计划已保存到本地 SQLite，等待确认运行': 'action.formPlanSaved',
  '自然语言计划已生成，并保存了提示词运行快照': 'action.naturalPlanSaved',
  '任务已确认并加入本地队列': 'action.confirmed',
  '任务名称已更新': 'action.renamed',
  '任务已取消': 'action.cancelled',
  '任务已删除': 'action.deleted',
  '当前未连接本地后端，不展示预览数据；请打开打包后的 macOS 应用': 'action.backendUnavailable',
  '本地工作区已打开，后端可用': 'action.workspaceReady',
  '计划需要修正': 'action.planNeedsRevision',
  '后端调用失败': 'action.backendCallFailed',
}

const dynamicPlanMessageTranslationKeys: Array<[RegExp, string]> = [
  [/^region 尚未验证$/, 'message.regionNotVerified'],
  [/^time_range 不能为空$/, 'message.timeRangeRequired'],
]

function localizedPlanStatus(t: CollectionTranslator, status: RuntimeCollectionPlan['status']) {
  return t(planStatusTranslationKeys[status] ?? 'status.unknown', { defaultValue: status })
}

function localizePlanMessage(t: CollectionTranslator, message: string | undefined) {
  if (!message) return ''
  const key = planMessageTranslationKeys[message]
  if (key) return t(key)
  const dynamicKey = dynamicPlanMessageTranslationKeys.find(([pattern]) => pattern.test(message))?.[1]
  if (dynamicKey) return t(dynamicKey)
  return /[^\p{ASCII}]/u.test(message) ? t('message.unknown') : message
}

function localizeActionMessage(t: CollectionTranslator, message: string) {
  const exportMatch = /^(Excel|PDF) 已导出到本地工作区$/.exec(message)
  if (exportMatch) return t('action.exported', { format: exportMatch[1] })
  const key = actionMessageTranslationKeys[message]
  if (key) return t(key)
  return /[^\p{ASCII}]/u.test(message) ? t('action.unknown') : message
}

function formatNumber(value: number, language: string) {
  return value.toLocaleString(language)
}

function localizedCostEstimate(
  t: CollectionTranslator,
  costEstimate: string | undefined,
  language: string,
) {
  if (!costEstimate) return t('preview.noEstimate')
  const requestEstimate = /^(\d+) 次请求$/.exec(costEstimate)
  if (requestEstimate) {
    return t('preview.requestEstimate', {
      count: formatNumber(Number(requestEstimate[1]), language),
    })
  }
  const requestQuote = /^(\d+) 次请求，实时报价上限 \$([\d.]+)$/.exec(costEstimate)
  if (requestQuote) {
    return t('preview.requestQuote', {
      amount: requestQuote[2],
      count: formatNumber(Number(requestQuote[1]), language),
    })
  }
  return /[^\p{ASCII}]/u.test(costEstimate) ? t('preview.estimateUnavailable') : costEstimate
}

function confirmationBlocker(
  plan: RuntimeCollectionPlan,
  isBusy: boolean,
  t: CollectionTranslator,
) {
  if (isBusy) return t('blocker.busy')
  if (!plan.taskId || !plan.planId) return t('blocker.unsaved')
  if (plan.validationStatus !== 'valid') {
    return localizePlanMessage(t, plan.missing[0]) || t('blocker.validationFailed')
  }
  if (plan.pricingReady !== true) {
    return localizePlanMessage(t, plan.pricingBlocker) || t('blocker.pricingNotReady')
  }
  if (plan.status === '已排队' || plan.status === '运行中') return undefined
  if (plan.status !== '等待确认') {
    return t('blocker.status', { status: localizedPlanStatus(t, plan.status) })
  }
  return undefined
}

function toneForPlanStatus(status: RuntimeCollectionPlan['status']) {
  if (status === '成功') return 'success'
  if (status === '失败') return 'danger'
  if (status === '等待确认' || status === '待人工确认' || status === '部分成功') return 'warning'
  return 'info'
}

export function StatusPill({ tone, label }: { tone: string; label: string }) {
  return (
    <span className="status-pill" data-tone={tone}>
      {iconForTone(tone)}
      {label}
    </span>
  )
}

function iconForTone(tone: string) {
  if (tone === 'success') return <CheckCircle2 size={13} aria-hidden="true" />
  if (tone === 'danger') return <AlertTriangle size={13} aria-hidden="true" />
  if (tone === 'warning') return <Activity size={13} aria-hidden="true" />
  return <BadgeCheck size={13} aria-hidden="true" />
}
