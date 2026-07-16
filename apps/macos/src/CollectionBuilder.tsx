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
import { z } from 'zod'
import './CollectionBuilder.css'
import type { RuntimeCollectionPlan } from './use-workbench-backend'
import { dataTypeOptions, platformOptions } from './workbench-data'

const collectionFormSchema = z.object({
  platform: z.enum(platformOptions),
  dataType: z.enum(dataTypeOptions),
  regionCode: z.string().min(2, '国家/地区代码至少 2 位').max(12, '代码过长'),
  keyword: z.string().min(2, '请输入关键词或账号').max(80, '关键词过长'),
  range: z.string().min(4, '请选择时间范围'),
  maxRecords: z.coerce.number().min(10, '至少 10 条').max(5000, 'MVP 单任务上限为 5000 条'),
  budget: z.coerce.number().min(1, '请输入成本上限').max(500, 'MVP 单任务上限为 500'),
})

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
    formState: { errors },
  } = useForm<CollectionFormInput, unknown, CollectionFormValues>({
    resolver: zodResolver(collectionFormSchema),
    defaultValues: {
      platform: plan.platform,
      dataType: plan.dataType,
      regionCode: plan.regionCode,
      keyword: plan.keyword,
      range: plan.range,
      maxRecords: plan.maxRecords,
      budget: plan.budget,
    },
  })

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
              <select {...register('dataType')}>
                {dataTypeOptions.map((item) => (
                  <option key={item} value={item}>
                    {item}
                  </option>
                ))}
              </select>
            </Field>
            <Field error={errors.regionCode?.message} label="国家/地区">
              <input {...register('regionCode')} placeholder="CN" />
            </Field>
            <Field error={errors.keyword?.message} label="关键词或账号">
              <input {...register('keyword')} />
            </Field>
            <Field error={errors.range?.message} label="时间范围">
              <input {...register('range')} />
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
    <label className="field">
      <span>{label}</span>
      {children}
      {error ? <small>{error}</small> : null}
    </label>
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
  const canConfirm = Boolean(
    plan.taskId && plan.planId && plan.validationStatus === 'valid' && plan.status === '等待确认',
  )
  const isEnqueued = plan.status === '已排队' || plan.status === '运行中'
  const confirmLabel = isEnqueued ? '已入队' : plan.taskId ? '确认运行' : '先生成计划'

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
        <InfoLine label="国家/地区" value={plan.regionCode ? `${plan.regionCode}，以后端计划为准` : '未提供'} />
        <InfoLine label="范围" value={plan.maxRecords > 0 ? `${plan.range}，最多 ${plan.maxRecords.toLocaleString()} 条` : plan.range} />
        <InfoLine
          label="成本"
          value={plan.budget > 0 ? `${plan.costEstimate ?? '尚无请求估算'}，金额上限 $${plan.budget}` : `${plan.costEstimate ?? '尚无请求估算'}，未设置金额上限`}
        />
        <InfoLine label="缺失条件" value={plan.missing.length ? plan.missing.join('、') : '无'} />
      </div>
      <p className="muted-text">{actionMessage}</p>
      <div className="action-row">
        <button
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
