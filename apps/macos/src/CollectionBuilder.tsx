import { useEffect, useId, useState } from 'react'
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
import { useForm } from 'react-hook-form'
import type { z } from 'zod'
import './CollectionBuilder.css'
import {
  AGE_RANGE_LIMITS,
  collectionFormSchema,
  collectionDataTypeOptions,
  countryRegionOptions,
  genderFilterOptions,
  supportsRegionSelection,
  type CollectionDataType,
} from './collection-options'
import {
  naturalIntentDefault,
  newCollectionFormDefaults,
  normalizeNaturalIntent,
} from './collection-form-defaults'
import type { RuntimeCollectionPlan } from './use-workbench-backend'
import { type DataType, platformOptions } from './workbench-data'

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
  const [naturalText, setNaturalText] = useState(naturalIntentDefault)
  const {
    register,
    handleSubmit,
    setValue,
    watch,
    formState: { errors },
  } = useForm<CollectionFormInput, unknown, CollectionFormValues>({
    resolver: zodResolver(collectionFormSchema),
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
          <p className="eyebrow">采集创建</p>
          <h2 id="collection-builder-heading">定义一条可执行的采集任务</h2>
          <p>先完成参数校验和实时计价，再由你确认是否进入运行队列。</p>
        </div>
        <StatusPill tone="warning" label="确认前不产生正式采集费用" />
      </header>

      <Tabs.Root className="collection-builder__tabs" defaultValue="form">
        <div className="collection-builder__mode-bar">
          <Tabs.List className="collection-builder__mode-list" aria-label="采集入口">
            <Tabs.Trigger className="collection-builder__mode-trigger" value="form">
              <Wrench size={15} aria-hidden="true" />
              表单式
            </Tabs.Trigger>
            <Tabs.Trigger className="collection-builder__mode-trigger" value="natural">
              <MessageSquareText size={15} aria-hidden="true" />
              自然语言
            </Tabs.Trigger>
          </Tabs.List>
          <p>两种方式都会生成同一种计划，生成计划不会自动运行。</p>
        </div>

        <Tabs.Content className="collection-builder__content" value="form">
          <form className="collection-builder__form" onSubmit={handleSubmit(submitForm)}>
            <FormGroup
              number="01"
              title="来源与目标"
              description="选择数据平台和需要获取的公开数据类型。"
            >
              <div className="collection-builder__source-fields">
                <FormField
                  error={errors.platform?.message}
                  errorId="platform-error"
                  htmlFor="platform"
                  label="平台"
                >
                  <select
                    id="platform"
                    aria-describedby={errors.platform ? 'platform-error' : undefined}
                    aria-invalid={Boolean(errors.platform)}
                    {...register('platform')}
                  >
                    <option value="">请选择平台</option>
                    {platformOptions.map((item) => (
                      <option key={item} value={item}>{item}</option>
                    ))}
                  </select>
                </FormField>

                <fieldset className="collection-builder__choice-fieldset">
                  <legend>数据类型</legend>
                  <input type="hidden" {...register('dataType')} />
                  <div className="collection-builder__option-list">
                    {collectionDataTypeOptions.map((item) => (
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
              title="采集范围"
              description="说明任务目标、地区和需要覆盖的时间。"
            >
              <div className="collection-builder__range-fields">
                <FormField
                  error={errors.keyword?.message}
                  errorId="keyword-error"
                  htmlFor="keyword"
                  label="关键词或账号"
                >
                  <input
                    id="keyword"
                    aria-describedby={errors.keyword ? 'keyword-error' : undefined}
                    aria-invalid={Boolean(errors.keyword)}
                    placeholder="输入关键词或公开账号 ID"
                    {...register('keyword')}
                  />
                </FormField>
                <FormField
                  error={errors.regionCode?.message}
                  errorId="region-code-error"
                  htmlFor="region-code"
                  label="国家/地区"
                  hint={regionEnabled ? '显示名称，提交标准两位代码。' : '所选平台或数据类型暂不支持地区筛选。'}
                >
                  <select
                    id="region-code"
                    aria-describedby={errors.regionCode ? 'region-code-error' : undefined}
                    aria-invalid={Boolean(errors.regionCode)}
                    disabled={!regionEnabled}
                    {...register('regionCode')}
                  >
                    <option value="">{regionEnabled ? '请选择国家/地区' : '当前不可用'}</option>
                    {countryRegionOptions.map((item) => (
                      <option key={item.code} value={item.code}>{item.label}</option>
                    ))}
                  </select>
                </FormField>
                <FormField
                  error={errors.range?.message}
                  errorId="range-error"
                  htmlFor="range"
                  label="时间范围"
                >
                  <input
                    id="range"
                    aria-describedby={errors.range ? 'range-error' : undefined}
                    aria-invalid={Boolean(errors.range)}
                    placeholder="输入平台支持的时间范围"
                    {...register('range')}
                  />
                </FormField>
              </div>
            </FormGroup>

            <FormGroup
              number="03"
              title="数量与成本"
              description="设置结果上限和本次任务可接受的金额上限。"
            >
              <div className="collection-builder__limit-fields">
                <FormField
                  error={errors.maxRecords?.message}
                  errorId="max-records-error"
                  htmlFor="max-records"
                  label="最大记录数"
                  suffix="条"
                >
                  <input
                    id="max-records"
                    aria-describedby={errors.maxRecords ? 'max-records-error' : undefined}
                    aria-invalid={Boolean(errors.maxRecords)}
                    min="1"
                    placeholder="输入记录上限"
                    type="number"
                    {...register('maxRecords', { valueAsNumber: true })}
                  />
                </FormField>
                <FormField
                  error={errors.budget?.message}
                  errorId="budget-error"
                  htmlFor="budget"
                  label="成本上限"
                  suffix="USD"
                >
                  <input
                    id="budget"
                    aria-describedby={errors.budget ? 'budget-error' : undefined}
                    aria-invalid={Boolean(errors.budget)}
                    min="0"
                    placeholder="输入金额上限"
                    step="0.01"
                    type="number"
                    {...register('budget', { valueAsNumber: true })}
                  />
                </FormField>
              </div>
            </FormGroup>

            <FormGroup
              number="04"
              title="公开信息筛选"
              description="筛选默认关闭；启用后只接受接口或公开资料明确返回的值。"
            >
              <div className="collection-builder__filter-grid">
                <fieldset className="collection-builder__filter-block" data-enabled={ageRangeEnabled}>
                  <legend className="collection-builder__visually-hidden">年龄范围</legend>
                  <label className="collection-builder__filter-toggle">
                    <input type="checkbox" {...register('ageRangeEnabled')} />
                    <span>
                      <strong>年龄范围</strong>
                      <small>单一闭区间，不接收未知、异常或推断年龄。</small>
                    </span>
                  </label>
                  {ageRangeEnabled ? (
                    <div className="collection-builder__age-inputs">
                      <label>
                        <span>最小年龄</span>
                        <input
                          aria-describedby={errors.ageMin ? 'age-min-error' : undefined}
                          aria-invalid={Boolean(errors.ageMin)}
                          min={AGE_RANGE_LIMITS.min}
                          max={AGE_RANGE_LIMITS.max}
                          placeholder="最小"
                          type="number"
                          {...register('ageMin', { valueAsNumber: true })}
                        />
                      </label>
                      <span aria-hidden="true">至</span>
                      <label>
                        <span>最大年龄</span>
                        <input
                          aria-describedby={errors.ageMax ? 'age-max-error' : undefined}
                          aria-invalid={Boolean(errors.ageMax)}
                          min={AGE_RANGE_LIMITS.min}
                          max={AGE_RANGE_LIMITS.max}
                          placeholder="最大"
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
                  <legend className="collection-builder__visually-hidden">性别筛选</legend>
                  <label className="collection-builder__filter-toggle">
                    <input type="checkbox" {...register('genderFilterEnabled')} />
                    <span>
                      <strong>性别筛选</strong>
                      <small>不根据头像、姓名或简介推断，仅使用明确公开性别。</small>
                    </span>
                  </label>
                  {genderFilterEnabled ? (
                    <div className="collection-builder__gender-options">
                      {genderFilterOptions.map((item) => (
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
              <p>生成计划只执行参数校验、实时计价和额度预检，不会自动入队。</p>
              <button className="primary-button" disabled={isBusy} type="submit">
                <Gauge size={16} aria-hidden="true" />
                {isBusy ? '正在生成' : '生成计划'}
              </button>
            </div>
          </form>
        </Tabs.Content>

        <Tabs.Content className="collection-builder__content" value="natural">
          <div className="collection-builder__natural">
            <div>
              <p className="eyebrow">自然语言任务</p>
              <h3>描述目标，不预填任何任务实例</h3>
              <p>写明平台、对象、范围和限制；系统会先解析为同一套可检查计划。</p>
            </div>
            <label htmlFor="intent">任务需求</label>
            <textarea
              id="intent"
              value={naturalText}
              placeholder="描述平台、采集对象、范围和成本限制"
              onChange={(event) => setNaturalText(event.target.value)}
            />
            <div className="collection-builder__natural-footer">
              <p>输入内容只用于生成本次计划，不会直接运行任务。</p>
              <button
                className="primary-button"
                disabled={isBusy || !normalizeNaturalIntent(naturalText)}
                type="button"
                onClick={submitNaturalText}
              >
                {activePlan
                  ? <RefreshCcw size={16} aria-hidden="true" />
                  : <Sparkles size={16} aria-hidden="true" />}
                {activePlan ? '重新解析' : '解析为计划'}
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
            <p className="eyebrow">采集计划</p>
            <h3>尚未生成计划</h3>
            <p>完成本次参数后，系统会先校验执行范围并读取实时计价。</p>
          </div>
          <StatusPill tone="info" label="等待生成" />
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
  const blockerId = useId()
  const blocker = confirmationBlocker(plan, isBusy)
  const isEnqueued = plan.status === '已排队' || plan.status === '运行中'
  const canConfirm = plan.status === '等待确认' && !blocker
  const showConfirmButton = plan.status === '等待确认' || plan.status === '待人工确认'
  const regionLabel = countryRegionOptions.find(({ code }) => code === plan.regionCode)?.label
  const ageLabel = plan.ageRangeEnabled && plan.ageMin !== undefined && plan.ageMax !== undefined
    ? `${plan.ageMin}–${plan.ageMax} 岁（闭区间）`
    : '未启用'
  const genderLabel = plan.genderFilterEnabled && plan.genders?.length
    ? plan.genders.map((gender) => (
      genderFilterOptions.find(({ value }) => value === gender)?.label ?? gender
    )).join('、')
    : '未启用'
  const platforms = (plan.platforms?.length ? plan.platforms : [plan.platform]).join('、')
  const dataTypes = (plan.dataTypes?.length ? plan.dataTypes : [plan.dataType]).join('、')
  const range = plan.maxRecords > 0
    ? `${plan.range}，最多 ${plan.maxRecords.toLocaleString()} 条`
    : plan.range
  const budget = plan.budget > 0 ? `$${plan.budget}` : '未设置金额上限'

  return (
    <section className="collection-plan" aria-labelledby="collection-plan-heading">
      <header className="collection-plan__header">
        <div>
          <p className="eyebrow">采集计划</p>
          <h3 id="collection-plan-heading">{plan.keyword || '待补充任务目标'}</h3>
          <p>{platforms} · {dataTypes}</p>
        </div>
        <StatusPill tone={toneForPlanStatus(plan.status)} label={plan.status} />
      </header>

      <div className="collection-plan__body">
        <dl className="collection-plan__facts">
          <PlanFact label="平台" value={platforms} />
          <PlanFact label="数据类型" value={dataTypes} />
          <PlanFact label="国家/地区" value={regionLabel ?? '未提供或目标不支持'} />
          <PlanFact label="采集范围" value={range} />
        </dl>

        <div className="collection-plan__detail-grid">
          <section className="collection-plan__filters" aria-labelledby="collection-plan-filters-heading">
            <h4 id="collection-plan-filters-heading">公开信息筛选</h4>
            <dl>
              <PlanFact label="年龄范围" value={ageLabel} />
              <PlanFact label="性别" value={genderLabel} />
            </dl>
          </section>
          <section
            className="collection-plan__pricing"
            data-ready={plan.pricingReady === true}
            aria-labelledby="collection-plan-pricing-heading"
          >
            <div>
              <h4 id="collection-plan-pricing-heading">请求与成本</h4>
              <p>{plan.pricingReady === true ? '实时计价与额度预检已完成' : '等待实时计价与额度预检'}</p>
            </div>
            <strong>{plan.costEstimate ?? '尚无请求估算'}</strong>
            <span>金额上限 {budget}</span>
          </section>
        </div>

        {plan.missing.length ? (
          <div className="collection-plan__missing">
            <AlertTriangle size={16} aria-hidden="true" />
            <div>
              <strong>仍需补充</strong>
              <p>{plan.missing.join('、')}</p>
            </div>
          </div>
        ) : null}

        {blocker && !isEnqueued ? (
          <p className="collection-plan__blocker" id={blockerId} role="status">
            <AlertTriangle size={15} aria-hidden="true" />
            {plan.taskId ? `暂不能运行：${blocker}` : blocker}
          </p>
        ) : null}
      </div>

      <footer className="collection-plan__footer">
        <p>{isEnqueued ? '任务已进入运行队列，请前往任务页查看进度。' : actionMessage}</p>
        {showConfirmButton ? (
          <button
            aria-describedby={blocker ? blockerId : undefined}
            className="primary-button"
            disabled={!canConfirm || isBusy}
            type="button"
            onClick={() => void onConfirmPlan()}
          >
            <CheckCircle2 size={16} aria-hidden="true" />
            {plan.taskId ? '确认运行' : '先生成计划'}
          </button>
        ) : (
          <span className="collection-plan__footer-state">{plan.status}</span>
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

function confirmationBlocker(plan: RuntimeCollectionPlan, isBusy: boolean) {
  if (isBusy) return '正在处理计划，请稍候'
  if (!plan.taskId || !plan.planId) return '请先生成并保存采集计划'
  if (plan.validationStatus !== 'valid') return plan.missing[0] ?? '计划校验未通过'
  if (plan.pricingReady !== true) {
    return plan.pricingBlocker ?? '实时计价或 TikHub 双额度尚未完成校验'
  }
  if (plan.status === '已排队' || plan.status === '运行中') return undefined
  if (plan.status !== '等待确认') return `计划状态为“${plan.status}”`
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
