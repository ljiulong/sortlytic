import { ChevronRight, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { StatusPill } from './CollectionBuilder'
import {
  activatePromptVersion,
  createPromptVersion,
  listPromptTemplates,
  listPromptVersions,
  type PromptTemplateView,
  type PromptVersionView,
} from './backend-api'
import { i18n, normalizeLanguage } from './i18n'

type PromptSettingsPhase = 'loading' | 'ready' | 'empty' | 'error'

const promptSettingsCopy = {
  'zh-CN': {
    eyebrow: 'AI 提示词',
    title: '自然语言采集计划',
    description: '管理发送给当前 AI 模型的计划生成指令。',
    manage: '管理提示词',
    dialogTitle: '管理 AI 提示词',
    dialogDescription: '查看版本、编辑正文，并通过真实模型回归后激活。',
    close: '关闭 AI 提示词弹窗',
    chain: '提示词 → AI 结构化计划 → Schema / 能力校验 → 用户确认 → TikHub 真实 API',
    boundary: '提示词不保存 API Key，也不能绕过预算校验和用户确认。',
    currentVersion: '当前启用版本',
    schema: '输出 Schema',
    regressionStatus: '回归 / 激活状态',
    versionLabel: '查看或编辑版本',
    editorLabel: '提示词正文',
    noteLabel: '版本说明',
    notePlaceholder: '说明本次修改目的，例如：补充地区与年龄约束',
    loading: '正在读取提示词',
    refreshing: '正在同步最新版本',
    loadError: '提示词读取失败，请确认本地后端可用后重试。',
    empty: '未找到自然语言采集提示词模板。',
    noActive: '尚无启用版本',
    active: '当前启用 v{{version}}',
    selected: '正在编辑：{{status}} v{{version}}',
    save: '保存为新版本',
    saving: '正在保存',
    activate: '激活当前草稿',
    activating: '正在激活',
    saved: '草稿 v{{version}} 已保存，确认内容后再激活。',
    activated: 'v{{version}} 已通过回归校验并激活。',
    saveError: '新版本保存失败，请检查正文和版本说明。',
    activateError: '激活失败，已同步回归结果。请修改内容并保存为新版本后再试。',
    status: {
      active: '已启用',
      draft: '草稿',
      archived: '已归档',
      failed_regression: '回归失败',
    },
  },
  'en-US': {
    eyebrow: 'AI prompt',
    title: 'Natural-language collection plan',
    description: 'Manage the planning instructions sent to the active AI model.',
    manage: 'Manage prompts',
    dialogTitle: 'Manage AI prompts',
    dialogDescription: 'Review versions, edit content, and activate only after live-model regressions pass.',
    close: 'Close the AI prompt dialog',
    chain: 'Prompt → AI structured plan → Schema / capability validation → User confirmation → Live TikHub API',
    boundary: 'Prompts never store API keys and cannot bypass budget validation or user confirmation.',
    currentVersion: 'Active version',
    schema: 'Output schema',
    regressionStatus: 'Regression / activation status',
    versionLabel: 'View or edit version',
    editorLabel: 'Prompt content',
    noteLabel: 'Version note',
    notePlaceholder: 'Describe the purpose, for example: add region and age constraints',
    loading: 'Loading prompts',
    refreshing: 'Syncing the latest versions',
    loadError: 'Prompts could not be loaded. Check the local backend and try again.',
    empty: 'The natural-language collection prompt template was not found.',
    noActive: 'No active version',
    active: 'Active v{{version}}',
    selected: 'Editing: {{status}} v{{version}}',
    save: 'Save as new version',
    saving: 'Saving',
    activate: 'Activate this draft',
    activating: 'Activating',
    saved: 'Draft v{{version}} was saved. Review it before activation.',
    activated: 'v{{version}} passed regression checks and is now active.',
    saveError: 'The new version could not be saved. Check the content and version note.',
    activateError: 'Activation failed and regression results were refreshed. Edit the prompt and save a new version before trying again.',
    status: {
      active: 'Active',
      draft: 'Draft',
      archived: 'Archived',
      failed_regression: 'Regression failed',
    },
  },
} as const

function PromptSettings() {
  const language = normalizeLanguage(i18n.resolvedLanguage)
  const copy = promptSettingsCopy[language]
  const [phase, setPhase] = useState<PromptSettingsPhase>('loading')
  const [template, setTemplate] = useState<PromptTemplateView | null>(null)
  const [versions, setVersions] = useState<PromptVersionView[]>([])
  const [selectedVersionId, setSelectedVersionId] = useState('')
  const [content, setContent] = useState('')
  const [changeNote, setChangeNote] = useState('')
  const [isOpen, setIsOpen] = useState(false)
  const [isRefreshing, setIsRefreshing] = useState(false)
  const [isSaving, setIsSaving] = useState(false)
  const [isActivating, setIsActivating] = useState(false)
  const [feedback, setFeedback] = useState('')
  const dialogRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let isCurrent = true

    void (async () => {
      try {
        const templates = await listPromptTemplates()
        const collectionTemplate = templates.find(
          (candidate) => candidate.template_key === 'collection_plan_from_text',
        )
        if (!isCurrent) return
        if (!collectionTemplate) {
          setPhase('empty')
          return
        }

        const promptVersions = await listPromptVersions(collectionTemplate.id)
        if (!isCurrent) return
        setTemplate(collectionTemplate)
        applyPromptVersions(promptVersions)
      } catch {
        if (isCurrent) setPhase('error')
      }
    })()

    return () => {
      isCurrent = false
    }
  }, [])

  const isMutating = isSaving || isActivating
  const activeVersion = versions.find((version) => version.status === 'active')
  const selectedVersion = versions.find((version) => version.id === selectedVersionId)
  const statusText = phase === 'loading'
    ? copy.loading
    : phase === 'error'
      ? copy.loadError
      : phase === 'empty'
        ? copy.empty
        : activeVersion
          ? interpolate(copy.active, { version: activeVersion.version })
          : copy.noActive
  const statusTone = phase === 'error'
    ? 'danger'
    : activeVersion
      ? 'success'
      : 'warning'

  function applyPromptVersions(promptVersions: PromptVersionView[]) {
    const initialVersion = promptVersions.find((version) => version.status === 'active')
      ?? promptVersions[0]
    setVersions(promptVersions)
    setSelectedVersionId(initialVersion?.id ?? '')
    setContent(initialVersion?.content ?? '')
    setChangeNote('')
    setPhase(promptVersions.length > 0 ? 'ready' : 'empty')
  }

  async function openDialog() {
    if (!template || phase !== 'ready') return
    setIsOpen(true)
    setIsRefreshing(true)
    setFeedback('')
    try {
      applyPromptVersions(await listPromptVersions(template.id))
    } catch {
      setFeedback(copy.loadError)
    } finally {
      setIsRefreshing(false)
    }
  }

  function closeDialog() {
    if (!isMutating) setIsOpen(false)
  }

  useEffect(() => {
    if (!isOpen) return
    const previouslyFocused = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null
    const frame = window.requestAnimationFrame(() => dialogRef.current?.focus())
    return () => {
      window.cancelAnimationFrame(frame)
      previouslyFocused?.focus()
    }
  }, [isOpen])

  useEffect(() => {
    if (!isOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        if (!isMutating) setIsOpen(false)
        return
      }
      if (event.key !== 'Tab') return
      const focusable = getFocusableElements(dialogRef.current)
      if (focusable.length === 0) {
        event.preventDefault()
        dialogRef.current?.focus()
        return
      }
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault()
        first.focus()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [isMutating, isOpen])

  async function saveVersion() {
    if (!template || !content.trim() || !changeNote.trim() || isMutating) return
    setIsSaving(true)
    setFeedback('')
    try {
      const version = await createPromptVersion({
        template_id: template.id,
        content: content.trim(),
        change_note: changeNote.trim(),
      })
      setVersions((current) => [
        version,
        ...current.filter((candidate) => candidate.id !== version.id),
      ])
      setSelectedVersionId(version.id)
      setContent(version.content)
      setChangeNote('')
      setFeedback(interpolate(copy.saved, { version: version.version }))
    } catch {
      setFeedback(copy.saveError)
    } finally {
      setIsSaving(false)
    }
  }

  async function activateVersion() {
    if (!template || !selectedVersion || selectedVersion.status !== 'draft' || isMutating) return
    setIsActivating(true)
    setFeedback('')
    try {
      const activated = await activatePromptVersion(selectedVersion.id)
      setVersions((current) => current.map((version) => {
        if (version.id === activated.id) return activated
        return version.status === 'active' ? { ...version, status: 'archived' } : version
      }))
      setSelectedVersionId(activated.id)
      setContent(activated.content)
      setFeedback(interpolate(copy.activated, { version: activated.version }))
    } catch {
      try {
        const promptVersions = await listPromptVersions(template.id)
        const refreshedSelection = promptVersions.find(
          (version) => version.id === selectedVersion.id,
        ) ?? promptVersions.find((version) => version.status === 'active')
          ?? promptVersions[0]
        setVersions(promptVersions)
        setSelectedVersionId(refreshedSelection?.id ?? '')
        setContent(refreshedSelection?.content ?? '')
        setChangeNote('')
        setPhase(promptVersions.length > 0 ? 'ready' : 'empty')
      } catch {
        // 保留当前页面数据，激活失败反馈仍需对用户可见。
      }
      setFeedback(copy.activateError)
    } finally {
      setIsActivating(false)
    }
  }

  return (
    <>
      <section
        className="workspace-settings prompt-settings-card"
        aria-labelledby="prompt-settings-heading"
      >
        <header>
          <div>
            <p className="eyebrow">{copy.eyebrow}</p>
            <h3 id="prompt-settings-heading">{copy.title}</h3>
            <p className="muted-text">{copy.description}</p>
          </div>
          <StatusPill tone={statusTone} label={statusText} />
        </header>

        {phase === 'ready' && template ? (
          <>
            <dl>
              <div>
                <dt>{copy.currentVersion}</dt>
                <dd>{activeVersion ? `v${activeVersion.version}` : copy.noActive}</dd>
              </div>
              <div>
                <dt>{copy.schema}</dt>
                <dd>{template.output_schema_id ?? copy.noActive}</dd>
              </div>
              <div>
                <dt>{copy.regressionStatus}</dt>
                <dd>{activeVersion
                  ? promptVersionStatus(activeVersion.status, copy.status)
                  : copy.noActive}</dd>
              </div>
            </dl>
            <footer className="prompt-settings-card__footer">
              <button
                aria-haspopup="dialog"
                className="ghost-button"
                data-prompt-action="manage"
                type="button"
                onClick={() => void openDialog()}
              >
                {copy.manage}
                <ChevronRight size={15} aria-hidden="true" />
              </button>
            </footer>
          </>
        ) : (
          <div className="prompt-settings-card__message">
            <p className="muted-text" role={phase === 'error' ? 'alert' : undefined}>
              {statusText}
            </p>
          </div>
        )}
      </section>

      {isOpen && template ? (
        <div
          className="prompt-settings-dialog__backdrop"
          data-prompt-backdrop
          role="presentation"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) closeDialog()
          }}
        >
          <div
            ref={dialogRef}
            aria-busy={isRefreshing || isMutating}
            aria-describedby="prompt-settings-dialog-description"
            aria-labelledby="prompt-settings-dialog-title"
            aria-modal="true"
            className="prompt-settings-dialog"
            data-prompt-dialog
            role="dialog"
            tabIndex={-1}
          >
            <header className="prompt-settings-dialog__header">
              <div>
                <p className="eyebrow">{copy.eyebrow}</p>
                <h2 id="prompt-settings-dialog-title">{copy.dialogTitle}</h2>
                <p id="prompt-settings-dialog-description">{copy.dialogDescription}</p>
              </div>
              <button
                aria-label={copy.close}
                className="prompt-settings-dialog__close"
                data-prompt-action="close"
                disabled={isMutating}
                type="button"
                onClick={closeDialog}
              >
                <X size={17} aria-hidden="true" />
              </button>
            </header>
            <div className="prompt-settings-dialog__body">
              <p className="prompt-settings-dialog__chain">
                <strong>{copy.chain}</strong>
                <span>{copy.boundary}</span>
              </p>
              <label className="field">
                <span>{copy.versionLabel}</span>
                <select
                  disabled={isRefreshing || isMutating}
                  value={selectedVersionId}
                  onChange={(event) => {
                    const version = versions.find(
                      (candidate) => candidate.id === event.currentTarget.value,
                    )
                    if (!version) return
                    setSelectedVersionId(version.id)
                    setContent(version.content)
                    setChangeNote('')
                    setFeedback('')
                  }}
                >
                  {versions.map((version) => (
                    <option key={version.id} value={version.id}>
                      {`v${version.version} · ${promptVersionStatus(version.status, copy.status)}`}
                    </option>
                  ))}
                </select>
              </label>
              {selectedVersion ? (
                <p className="muted-text" data-prompt-status>
                  {interpolate(copy.selected, {
                    status: promptVersionStatus(selectedVersion.status, copy.status),
                    version: selectedVersion.version,
                  })}
                </p>
              ) : null}
              <label className="field">
                <span>{copy.editorLabel}</span>
                <textarea
                  data-prompt-content
                  disabled={isRefreshing || isMutating}
                  value={content}
                  onChange={(event) => setContent(event.currentTarget.value)}
                />
              </label>
              <label className="field">
                <span>{copy.noteLabel}</span>
                <input
                  data-prompt-change-note
                  disabled={isRefreshing || isMutating}
                  value={changeNote}
                  placeholder={copy.notePlaceholder}
                  onChange={(event) => setChangeNote(event.currentTarget.value)}
                />
              </label>
              <p className="prompt-settings-dialog__feedback" aria-live="polite">
                {isRefreshing ? copy.refreshing : feedback}
              </p>
            </div>
            <footer className="prompt-settings-dialog__footer">
              <button
                className="ghost-button"
                type="button"
                disabled={isRefreshing || isMutating || !content.trim() || !changeNote.trim()}
                onClick={() => void saveVersion()}
              >
                {isSaving ? copy.saving : copy.save}
              </button>
              <button
                className="primary-button"
                type="button"
                disabled={isRefreshing || isMutating
                  || !selectedVersion || selectedVersion.status !== 'draft'}
                onClick={() => void activateVersion()}
              >
                {isActivating ? copy.activating : copy.activate}
              </button>
            </footer>
          </div>
        </div>
      ) : null}
    </>
  )
}

function promptVersionStatus(
  status: string,
  copy: typeof promptSettingsCopy['zh-CN']['status'] | typeof promptSettingsCopy['en-US']['status'],
) {
  return copy[status as keyof typeof copy] ?? status
}

function interpolate(template: string, values: Record<string, string | number>) {
  return Object.entries(values).reduce(
    (result, [key, value]) => result.replace(`{{${key}}}`, String(value)),
    template,
  )
}

function getFocusableElements(container: HTMLElement | null) {
  if (!container) return []
  return Array.from(container.querySelectorAll<HTMLElement>(
    'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
  ))
}

export default PromptSettings
