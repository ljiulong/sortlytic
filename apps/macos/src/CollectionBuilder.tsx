import { type ReactNode, useEffect, useState } from 'react'
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
} from './collection-options'
import type { RuntimeCollectionPlan } from './use-workbench-backend'
import { platformOptions } from './workbench-data'

export type CollectionFormInput = z.input<typeof collectionFormSchema>
export type CollectionFormValues = z.output<typeof collectionFormSchema>

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
  const [plan, setPlan] = useState<RuntimeCollectionPlan>({
    platform: '小红书',
    dataType: '评论采集',
    regionCode: 'CN',
    keyword: '新能源汽车 女车主 安全感',
    range: '近 30 天',
    maxRecords: 1200,
    budget: 35,
    status: '等待确认',
    missing: [],
  })
  const [naturalText, setNaturalText] = useState(
    '分析中国小红书近 30 天新能源汽车女性车主评论，重点看安全感、补能和售后体验，成本控制在 35 美元以内。',
  )

  useEffect(() => {
    if (activePlan) {
      setPlan(activePlan)
    }
  }, [activePlan])

  const {
    register,
    handleSubmit,
    setValue,
    watch,
    formState: { errors },
  } = useForm<CollectionFormInput, unknown, CollectionFormValues>({
    resolver: zodResolver(collectionFormSchema),
    defaultValues: {
      platform: plan.platform,
      dataType: plan.dataType,
      dataTypes: ['keyword_search'],
      regionCode: plan.regionCode,
      keyword: plan.keyword,
      range: plan.range,
      maxRecords: plan.maxRecords,
      budget: plan.budget,
      ageRangeEnabled: false,
      genderFilterEnabled: false,
      genders: [],
    },
  })
  const selectedPlatform = watch('platform')
  const selectedDataTypes = watch('dataTypes') ?? []
  const ageRangeEnabled = watch('ageRangeEnabled')
  const genderFilterEnabled = watch('genderFilterEnabled')
  const regionEnabled = supportsRegionSelection(selectedPlatform, selectedDataTypes)

  useEffect(() => {
    if (!regionEnabled) setValue('regionCode', '')
  }, [regionEnabled, setValue])

  const submitForm = async (values: CollectionFormValues) => {
    const nextPlan = await onGenerateFormPlan(values)
    setPlan(nextPlan)
  }

  const submitNaturalText = async () => {
    const nextPlan = await onGenerateNaturalPlan(naturalText)
    setPlan(nextPlan)
  }

  return (
    <section className="glass-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">采集创建</p>
          <h2>表单式采集与自然语言计划</h2>
        </div>
        <StatusPill tone="warning" label="确认前不产生正式采集费用" />
      </div>

      <Tabs.Root className="tabs-root" defaultValue="form">
        <Tabs.List className="tabs-list" aria-label="采集入口">
          <Tabs.Trigger className="tabs-trigger" value="form">
            <Wrench size={15} aria-hidden="true" />
            表单式
          </Tabs.Trigger>
          <Tabs.Trigger className="tabs-trigger" value="natural">
            <MessageSquareText size={15} aria-hidden="true" />
            自然语言
          </Tabs.Trigger>
        </Tabs.List>

        <Tabs.Content className="tabs-content" value="form">
          <form className="collection-form" onSubmit={handleSubmit(submitForm)}>
            <Field label="平台">
              <select {...register('platform')}>
                {platformOptions.map((item) => (
                  <option key={item} value={item}>
                    {item}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="数据类型">
              <input type="hidden" {...register('dataType')} />
              <div className="collection-type-grid">
                {collectionDataTypeOptions.map((item) => (
                  <label className="collection-type-option" key={item.value}>
                    <input type="checkbox" value={item.value} {...register('dataTypes')} />
                    <span>
                      <strong>{item.label}</strong>
                      <small>{item.description}</small>
                    </span>
                  </label>
                ))}
              </div>
              {errors.dataTypes?.message ? <small>{errors.dataTypes.message}</small> : null}
            </Field>
            <Field error={errors.regionCode?.message} label="国家/地区">
              <input
                list="country-region-options"
                disabled={!regionEnabled}
                {...register('regionCode')}
                placeholder={regionEnabled ? '输入名称或两位代码搜索' : '所选目标不支持地区筛选'}
              />
              <datalist id="country-region-options">
                {countryRegionOptions.map((item) => (
                  <option key={item.code} value={item.code}>{item.label}</option>
                ))}
              </datalist>
            </Field>
            <Field error={errors.keyword?.message} label="关键词或账号">
              <input {...register('keyword')} />
            </Field>
            <Field error={errors.range?.message} label="时间范围">
              <input {...register('range')} />
            </Field>
            <Field label="年龄范围">
              <label className="inline-toggle">
                <input type="checkbox" {...register('ageRangeEnabled')} />
                仅保留明确公开年龄的账号
              </label>
              <div className="age-range-inputs">
                <input
                  aria-label="最小年龄"
                  disabled={!ageRangeEnabled}
                  min={AGE_RANGE_LIMITS.min}
                  max={AGE_RANGE_LIMITS.max}
                  placeholder="最小"
                  type="number"
                  {...register('ageMin', { valueAsNumber: true })}
                />
                <span>至</span>
                <input
                  aria-label="最大年龄"
                  disabled={!ageRangeEnabled}
                  min={AGE_RANGE_LIMITS.min}
                  max={AGE_RANGE_LIMITS.max}
                  placeholder="最大"
                  type="number"
                  {...register('ageMax', { valueAsNumber: true })}
                />
              </div>
              {errors.ageMin?.message ? <small>{errors.ageMin.message}</small> : null}
              {errors.ageMax?.message ? <small>{errors.ageMax.message}</small> : null}
            </Field>
            <Field label="性别">
              <label className="inline-toggle">
                <input type="checkbox" {...register('genderFilterEnabled')} />
                仅保留明确公开性别的账号
              </label>
              <div className="collection-type-grid">
                {genderFilterOptions.map((item) => (
                  <label className="collection-type-option" key={item.value}>
                    <input
                      disabled={!genderFilterEnabled}
                      type="checkbox"
                      value={item.value}
                      {...register('genders')}
                    />
                    <span><strong>{item.label}</strong></span>
                  </label>
                ))}
              </div>
              {errors.genders?.message ? <small>{errors.genders.message}</small> : null}
            </Field>
            <Field error={errors.maxRecords?.message} label="最大记录数">
              <input type="number" {...register('maxRecords', { valueAsNumber: true })} />
            </Field>
            <Field error={errors.budget?.message} label="成本上限">
              <input type="number" {...register('budget', { valueAsNumber: true })} />
            </Field>
            <button className="primary-button form-submit" disabled={isBusy} type="submit">
              <Gauge size={16} aria-hidden="true" />
              生成计划
            </button>
          </form>
        </Tabs.Content>

        <Tabs.Content className="tabs-content" value="natural">
          <div className="natural-input">
            <label htmlFor="intent">自然语言需求</label>
            <textarea
              id="intent"
              value={naturalText}
              onChange={(event) => setNaturalText(event.target.value)}
            />
            <div className="action-row">
              <button className="primary-button" disabled={isBusy} type="button" onClick={submitNaturalText}>
                <Sparkles size={16} aria-hidden="true" />
                解析为计划
              </button>
              <button className="ghost-button" disabled={isBusy} type="button" onClick={submitNaturalText}>
                <RefreshCcw size={16} aria-hidden="true" />
                重新生成
              </button>
            </div>
          </div>
        </Tabs.Content>
      </Tabs.Root>

      <CollectionPlanPreview
        actionMessage={actionMessage}
        isBusy={isBusy}
        onConfirmPlan={onConfirmPlan}
        plan={plan}
      />
    </section>
  )
}

function Field({
  label,
  error,
  children,
}: {
  label: string
  error?: string
  children: ReactNode
}) {
  return (
    <div className="field">
      <span>{label}</span>
      {children}
      {error ? <small>{error}</small> : null}
    </div>
  )
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
  const blocker = confirmationBlocker(plan, isBusy)
  const canConfirm = !blocker
  const isEnqueued = plan.status === '已排队' || plan.status === '运行中'
  const confirmLabel = isEnqueued ? '已入队' : plan.taskId ? '确认运行' : '先生成计划'
  const regionLabel = countryRegionOptions.find(({ code }) => code === plan.regionCode)?.label
  const ageLabel = plan.ageRangeEnabled && plan.ageMin !== undefined && plan.ageMax !== undefined
    ? `${plan.ageMin}–${plan.ageMax} 岁（闭区间，仅明确年龄）`
    : '未启用'
  const genderLabel = plan.genderFilterEnabled && plan.genders?.length
    ? plan.genders.map((gender) => genderFilterOptions.find(({ value }) => value === gender)?.label ?? gender).join('、')
    : '未启用'

  return (
    <div className="plan-preview">
      <div className="plan-header">
        <div>
          <p className="eyebrow">采集计划</p>
          <h3>{plan.keyword}</h3>
        </div>
        <StatusPill tone={plan.status === '待人工确认' ? 'warning' : 'info'} label={plan.status} />
      </div>
      <div className="plan-grid">
        <InfoLine label="平台" value={(plan.platforms?.length ? plan.platforms : [plan.platform]).join('、')} />
        <InfoLine label="数据类型" value={(plan.dataTypes?.length ? plan.dataTypes : [plan.dataType]).join('、')} />
        <InfoLine label="国家/地区" value={regionLabel ?? '未提供或目标不支持'} />
        <InfoLine label="年龄范围" value={ageLabel} />
        <InfoLine label="性别" value={genderLabel} />
        <InfoLine label="范围" value={plan.maxRecords > 0 ? `${plan.range}，最多 ${plan.maxRecords.toLocaleString()} 条` : plan.range} />
        <InfoLine
          label="成本"
          value={plan.budget > 0 ? `${plan.costEstimate ?? '尚无请求估算'}，金额上限 $${plan.budget}` : `${plan.costEstimate ?? '尚无请求估算'}，未设置金额上限`}
        />
        <InfoLine label="缺失条件" value={plan.missing.length ? plan.missing.join('、') : '无'} />
      </div>
      <p className="muted-text">{actionMessage}</p>
      {blocker ? (
        <p className="plan-blocker" id="plan-confirm-blocker" role="status">
          <AlertTriangle size={15} aria-hidden="true" />
          {plan.taskId ? `暂不能运行：${blocker}` : blocker}
        </p>
      ) : null}
      <div className="action-row">
        <button
          aria-describedby={blocker ? 'plan-confirm-blocker' : undefined}
          className="primary-button"
          disabled={!canConfirm || isBusy}
          type="button"
          onClick={() => {
            void onConfirmPlan()
          }}
        >
          <CheckCircle2 size={16} aria-hidden="true" />
          {confirmLabel}
        </button>
      </div>
    </div>
  )
}

function confirmationBlocker(plan: RuntimeCollectionPlan, isBusy: boolean) {
  if (isBusy) return '正在处理计划，请稍候'
  if (!plan.taskId || !plan.planId) return '请先生成并保存采集计划'
  if (plan.validationStatus !== 'valid') return plan.missing[0] ?? '计划校验未通过'
  if (plan.status !== '等待确认') {
    if (plan.status === '已排队' || plan.status === '运行中') return '任务已进入运行队列'
    return `计划状态为“${plan.status}”`
  }
  return undefined
}

function InfoLine({ label, value }: { label: string; value: string }) {
  return (
    <div className="info-line">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  )
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
